#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod graphics_device;
mod preview;
use paladinscat_launcher::{engine, ime, launch, performance, sources};
use sources::{LOCALE_REPOSITORY as REPOSITORY, catalog_url};

use egui::{Color32, FontFamily, FontId, RichText};
use engine::{Action, Cached, Release};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
};

const BG: Color32 = Color32::from_rgb(27, 27, 31);
const PANEL: Color32 = Color32::from_rgb(32, 33, 39);
const TEXT: Color32 = Color32::from_rgb(223, 223, 214);
const MUTED: Color32 = Color32::from_rgb(152, 152, 159);
const ACCENT: Color32 = Color32::from_rgb(100, 186, 177);

#[derive(Clone, Serialize, Deserialize)]
struct Settings {
    game_folder: String,
    repository: String,
    catalog_url: String,
    #[serde(default)]
    prepared_translation: String,
    #[serde(default)]
    prepared_ime: String,
    #[serde(default)]
    prepared_performance: String,
    #[serde(default)]
    performance_enabled: bool,
    #[serde(default)]
    rollback_input: bool,
    #[serde(default = "default_cli")]
    tempest_cli: String,
    #[serde(default = "default_language")]
    launch_language: String,
    #[serde(default = "default_homedir")]
    homedir: String,
}
fn default_cli() -> String {
    launch::default_tempest_cli().display().to_string()
}
fn default_language() -> String {
    launch::DEFAULT_LANGUAGE.into()
}
fn default_homedir() -> String {
    launch::DEFAULT_PROFILE.into()
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            game_folder: String::new(),
            repository: REPOSITORY.into(),
            catalog_url: catalog_url(REPOSITORY),
            prepared_translation: String::new(),
            prepared_ime: String::new(),
            prepared_performance: String::new(),
            performance_enabled: false,
            rollback_input: false,
            tempest_cli: default_cli(),
            launch_language: default_language(),
            homedir: default_homedir(),
        }
    }
}
enum Event {
    Progress(String),
    Done(std::result::Result<Work, String>),
}
enum Work {
    Startup(Startup),
    Catalog(engine::Catalog),
    Package(Box<Cached>),
    ImePackage(Box<ime::Cached>),
    PerformancePackage(Box<performance::Cached>),
    Message(String),
}
struct Startup {
    catalog: engine::Result<engine::Catalog>,
    translation: Option<engine::Result<Box<Cached>>>,
    ime: Option<engine::Result<Box<ime::Cached>>>,
    performance: Option<engine::Result<Box<performance::Cached>>>,
}
impl Startup {
    fn load(settings: &Settings, cache: &std::path::Path, progress: engine::Progress<'_>) -> Self {
        let translation = (!settings.prepared_translation.is_empty()).then(|| {
            engine::import(
                &PathBuf::from(&settings.prepared_translation),
                cache,
                progress,
            )
            .map(Box::new)
        });
        let ime = (!settings.prepared_ime.is_empty())
            .then(|| ime::import(&PathBuf::from(&settings.prepared_ime), cache).map(Box::new));
        let catalog = engine::check_catalog(&settings.source_url());
        let performance = (!settings.prepared_performance.is_empty()).then(|| {
            performance::import(&PathBuf::from(&settings.prepared_performance), cache).map(Box::new)
        });
        Self {
            catalog,
            translation,
            ime,
            performance,
        }
    }
}
impl Settings {
    fn load() -> Self {
        std::fs::read(engine::data_dir().join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    fn source_url(&self) -> String {
        if self.catalog_url.trim().is_empty() {
            catalog_url(&self.repository)
        } else {
            self.catalog_url.trim().into()
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
enum PackageTab {
    Translation,
    Ime,
    Performance,
}
enum ImeInstallState {
    NoGame,
    Disabled,
    Enabled(Box<ime::Package>),
    Invalid(String),
}
impl ImeInstallState {
    fn inspect(folder: &str) -> Self {
        if folder.is_empty() {
            return Self::NoGame;
        }
        match ime::installed_package(&PathBuf::from(folder)) {
            Ok(Some(package)) => Self::Enabled(Box::new(package)),
            Ok(None) => Self::Disabled,
            Err(error) => Self::Invalid(error),
        }
    }
    fn label(&self) -> String {
        match self {
            Self::NoGame => "No game folder selected".into(),
            Self::Disabled => "Disabled".into(),
            Self::Enabled(p) => format!("Enabled · {} · {}", p.version, p.game_build),
            Self::Invalid(error) => format!("Unavailable: {error}"),
        }
    }
    fn enabled(&self) -> bool {
        matches!(self, Self::Enabled(_))
    }
    fn can_toggle(&self, prepared: bool, recovery: bool) -> bool {
        !recovery && (self.enabled() || (matches!(self, Self::Disabled) && prepared))
    }
    fn toggle_action(
        &self,
        requested: bool,
        prepared: bool,
        recovery: bool,
    ) -> Option<ime::Action> {
        if !self.can_toggle(prepared, recovery) {
            return None;
        }
        match (self, requested) {
            (Self::Disabled, true) => Some(ime::Action::Apply),
            (Self::Enabled(_), false) => Some(ime::Action::Remove),
            _ => None,
        }
    }
    fn update_available(&self, prepared: Option<&ime::Cached>) -> bool {
        matches!((self, prepared), (Self::Enabled(installed), Some(prepared)) if **installed != prepared.package)
    }
}
struct App {
    settings: Settings,
    releases: Vec<Release>,
    release_index: usize,
    language: String,
    selected: Option<Cached>,
    receiver: Option<Receiver<Event>>,
    status: String,
    installed: String,
    ime_releases: Vec<ime::Release>,
    ime_index: usize,
    ime_selected: Option<ime::Cached>,
    ime_installed: ImeInstallState,
    error: bool,
    logo: egui::TextureHandle,
    preview: bool,
    tab: PackageTab,
    catalog_status: String,
    catalog_connected: bool,
    performance_selected: Option<performance::Cached>,
}

fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        egui::FontData::from_static(include_bytes!("../assets/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "Inter Bold".into(),
        egui::FontData::from_static(include_bytes!("../assets/Inter-Bold.ttf")).into(),
    );
    let mut fallback = Vec::new();
    // Windows supplies Korean glyphs; Inter remains the primary face.
    let windows_root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Windows"));
    if let Ok(data) = std::fs::read(windows_root.join("Fonts/malgun.ttf")) {
        fonts
            .font_data
            .insert("Korean".into(), egui::FontData::from_owned(data).into());
        fallback.push("Korean".to_owned());
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".into());
    proportional.extend(fallback.clone());
    let mut bold = vec!["Inter Bold".into()];
    bold.extend(fallback);
    fonts
        .families
        .insert(FontFamily::Name("InterBold".into()), bold);
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = Color32::from_rgb(22, 22, 24);
    style.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 42, 48);
    style.visuals.widgets.inactive.bg_stroke =
        egui::Stroke::new(1_f32, Color32::from_rgb(60, 63, 68));
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(53, 59, 63);
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1_f32, ACCENT);
    style.spacing.item_spacing = egui::vec2(8., 10.);
    style.spacing.button_padding = egui::vec2(14., 10.);
    for key in [egui::TextStyle::Body, egui::TextStyle::Button] {
        style.text_styles.insert(key, FontId::proportional(14.));
    }
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, bold_font(20.));
    ctx.set_style_of(egui::Theme::Dark, style);
}
fn bold_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("InterBold".into()))
}
fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .corner_radius(10)
        .inner_margin(18)
        .stroke(egui::Stroke::new(1_f32, Color32::from_rgb(60, 63, 68)))
}
impl App {
    fn new(ctx: &egui::Context, preview: bool) -> Self {
        configure(ctx);
        let image = image::load_from_memory(include_bytes!("../assets/paladinscat.png"))
            .expect("Embedded logo")
            .to_rgba8();
        let logo = ctx.load_texture(
            "PaladinsCat",
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );
        let settings = if preview {
            Settings::default()
        } else {
            Settings::load()
        };
        let mut app = Self {
            settings,
            releases: Vec::new(),
            release_index: 0,
            language: "KOR".into(),
            selected: None,
            receiver: None,
            status: "Ready".into(),
            installed: if preview {
                "Not inspected in CPU preview".into()
            } else {
                "No game folder selected".into()
            },
            ime_releases: Vec::new(),
            ime_index: 0,
            ime_selected: None,
            // Offscreen previews use a disabled fixture, never a live receipt.
            ime_installed: if preview {
                ImeInstallState::Disabled
            } else {
                ImeInstallState::NoGame
            },
            error: false,
            logo,
            preview,
            tab: PackageTab::Translation,
            catalog_status: "Catalog not checked".into(),
            catalog_connected: false,
            performance_selected: None,
        };
        if !preview {
            app.refresh_installed();
            app.startup(ctx);
        }
        app
    }
    fn persist(&mut self) {
        if self.preview {
            return;
        }
        let dir = engine::data_dir();
        let result = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|_| engine::save_settings(&dir.join("settings.json"), &self.settings));
        if let Err(error) = result {
            self.error = true;
            self.status = format!("Could not save settings: {error}");
        }
    }
    fn refresh_installed(&mut self) {
        if self.settings.game_folder.is_empty() {
            self.installed = "No game folder selected".into();
            self.ime_installed = ImeInstallState::NoGame;
            return;
        }
        self.installed = engine::installed(&PathBuf::from(&self.settings.game_folder))
            .unwrap_or_else(|e| format!("Receipt unavailable: {e}"));
        self.ime_installed = ImeInstallState::inspect(&self.settings.game_folder);
    }
    fn work(
        &mut self,
        ctx: &egui::Context,
        task: impl FnOnce(&dyn Fn(String)) -> engine::Result<Work> + Send + 'static,
    ) {
        if self.receiver.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.error = false;
        self.status = "Preparing…".into();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let progress = |message| {
                let _ = sender.send(Event::Progress(message));
                repaint.request_repaint();
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task(&progress)))
                .unwrap_or_else(|_| {
                    Err("Operation interrupted; use Recover if a transaction is pending".into())
                });
            let _ = sender.send(Event::Done(result));
            repaint.request_repaint();
        });
    }
    fn receive(&mut self) {
        let events: Vec<_> = self
            .receiver
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Progress(message) => self.status = message,
                Event::Done(result) => {
                    self.receiver = None;
                    match result {
                        Ok(Work::Startup(startup)) => {
                            let mut failures = Vec::new();
                            match startup.catalog {
                                Ok(catalog) => self.accept_catalog(catalog),
                                Err(error) => {
                                    self.catalog_status = "Catalog unavailable".into();
                                    self.catalog_connected = false;
                                    failures.push(format!("GitHub catalog: {error}"));
                                }
                            }
                            if let Some(result) = startup.translation {
                                match result {
                                    Ok(package) => self.accept_translation(*package),
                                    Err(error) => {
                                        failures.push(format!("Saved translation: {error}"))
                                    }
                                }
                            }
                            if let Some(result) = startup.ime {
                                match result {
                                    Ok(package) => self.accept_ime(*package),
                                    Err(error) => failures.push(format!("Saved IME: {error}")),
                                }
                            }
                            if let Some(result) = startup.performance {
                                match result {
                                    Ok(package) => self.accept_performance(*package),
                                    Err(error) => {
                                        failures.push(format!("Saved performance package: {error}"))
                                    }
                                }
                            }
                            self.error = !failures.is_empty();
                            self.status = if failures.is_empty() {
                                "Ready".into()
                            } else {
                                failures.join("; ")
                            };
                        }
                        Ok(Work::Catalog(catalog)) => {
                            self.accept_catalog(catalog);
                            self.status = "Catalog refreshed".into();
                        }
                        Ok(Work::Package(package)) => {
                            self.accept_translation(*package);
                            self.status =
                                "Package downloaded/imported and verified. Ready to apply.".into();
                        }
                        Ok(Work::ImePackage(package)) => {
                            self.accept_ime(*package);
                            self.status = "IME package verified. Ready to apply.".into();
                        }
                        Ok(Work::PerformancePackage(package)) => {
                            self.accept_performance(*package);
                            self.status = "Performance package ready".into();
                        }
                        Ok(Work::Message(message)) => self.status = message,
                        Err(error) => {
                            if self.catalog_status == "Connecting to GitHub…" {
                                self.catalog_status = "Catalog unavailable".into();
                                self.catalog_connected = false;
                            }
                            self.error = true;
                            self.status = error;
                        }
                    }
                    if !self.preview {
                        self.refresh_installed();
                    }
                }
            }
        }
    }
    fn accept_catalog(&mut self, catalog: engine::Catalog) {
        self.catalog_connected = true;
        self.releases = catalog.packages;
        self.ime_releases = catalog.ime_packages;
        self.release_index = 0;
        self.ime_index = 0;
        if let Some(release) = self.releases.first() {
            self.language = release.locale.clone();
        }
        self.catalog_status = if self.releases.is_empty() && self.ime_releases.is_empty() {
            "Connected · no downloadable packages published".into()
        } else {
            format!(
                "Connected · {} translations · {} IME packages",
                self.releases.len(),
                self.ime_releases.len()
            )
        };
    }
    fn accept_translation(&mut self, package: Cached) {
        self.language = package.package.locale.clone();
        self.settings.prepared_translation =
            package.directory.join("bundle.json").display().to_string();
        self.selected = Some(package);
        self.persist();
    }
    fn accept_ime(&mut self, package: ime::Cached) {
        self.settings.prepared_ime = package
            .directory
            .join("ime-package.json")
            .display()
            .to_string();
        self.ime_selected = Some(package);
        self.persist();
    }
    fn startup(&mut self, ctx: &egui::Context) {
        let settings = self.settings.clone();
        let cache = engine::data_dir();
        self.catalog_status = "Connecting to GitHub…".into();
        self.catalog_connected = false;
        self.work(ctx, move |progress| {
            Ok(Work::Startup(Startup::load(&settings, &cache, progress)))
        });
    }
    fn accept_performance(&mut self, package: performance::Cached) {
        self.settings.prepared_performance =
            performance::manifest_path(&package).display().to_string();
        self.performance_selected = Some(package);
        self.persist();
    }
    fn launch_options(&self) -> launch::Options {
        launch::Options {
            game_root: PathBuf::from(&self.settings.game_folder),
            tempest_cli: PathBuf::from(&self.settings.tempest_cli),
            language: self.settings.launch_language.clone(),
            homedir: self.settings.homedir.clone(),
            performance: self
                .settings
                .performance_enabled
                .then(|| self.performance_selected.clone())
                .flatten(),
            rollback_input: self.settings.performance_enabled && self.settings.rollback_input,
        }
    }
    fn play(&mut self, ctx: &egui::Context) {
        if self.settings.performance_enabled && self.performance_selected.is_none() {
            self.error = true;
            self.status = "Download the selected performance package before launch".into();
            return;
        }
        let options = self.launch_options();
        let logs = engine::data_dir().join("launch-logs");
        self.work(ctx, move |_| {
            launch::start(&options, &logs).map(|result| Work::Message(result.status))
        });
    }
    fn source_url(&self) -> String {
        self.settings.source_url()
    }
    fn action(&mut self, ctx: &egui::Context, action: Action) {
        let game = PathBuf::from(&self.settings.game_folder);
        let selected = self.selected.clone();
        let cache = engine::data_dir();
        self.work(ctx, move |progress| {
            engine::operate(action, &game, selected, &cache, progress).map(Work::Message)
        });
    }
    fn ime_action(&mut self, ctx: &egui::Context, action: ime::Action) {
        let game = PathBuf::from(&self.settings.game_folder);
        let selected = self.ime_selected.clone();
        self.work(ctx, move |progress| {
            ime::operate(action, &game, selected, progress).map(Work::Message)
        });
    }
    fn check_updates(&mut self, ctx: &egui::Context) {
        let url = self.source_url();
        self.catalog_status = "Connecting to GitHub…".into();
        self.catalog_connected = false;
        self.persist();
        self.work(ctx, move |_| engine::check_catalog(&url).map(Work::Catalog));
    }
    fn invalidate_catalog(&mut self) {
        self.catalog_connected = false;
        self.catalog_status = "Catalog not checked".into();
        self.releases.clear();
        self.ime_releases.clear();
        self.release_index = 0;
        self.ime_index = 0;
    }
    fn tab_source(&self, tab: PackageTab) -> (String, String, String) {
        if tab == PackageTab::Performance {
            return (
                performance::REPOSITORY.into(),
                performance::RELEASE_URL.into(),
                format!("Pinned release · v{}", performance::VERSION),
            );
        }
        let status = if self.catalog_connected {
            let (count, kind) = match tab {
                PackageTab::Translation => (self.releases.len(), "localization"),
                PackageTab::Ime => (self.ime_releases.len(), "IME"),
                PackageTab::Performance => unreachable!(),
            };
            if count == 0 {
                format!("Connected · no {kind} packages published")
            } else {
                format!(
                    "Connected · {count} {kind} package{}",
                    if count == 1 { "" } else { "s" }
                )
            }
        } else {
            self.catalog_status.clone()
        };
        (self.settings.repository.clone(), self.source_url(), status)
    }
    fn repository_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, busy: bool) {
        let (repository, source, status) = self.tab_source(self.tab);
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to(&repository, sources::repository_url(&repository));
            ui.label(RichText::new(status).size(12.).color(MUTED));
            if self.tab == PackageTab::Performance {
                ui.hyperlink_to("Release", &source);
            } else if ui
                .add_enabled(!busy, egui::Button::new("Refresh"))
                .clicked()
            {
                self.check_updates(ctx);
            }
        });
        egui::CollapsingHeader::new("Repository settings")
            .id_salt(match self.tab {
                PackageTab::Translation => "localization-source",
                PackageTab::Ime => "ime-source",
                PackageTab::Performance => "performance-source",
            })
            .show(ui, |ui| {
                if self.tab == PackageTab::Performance {
                    ui.hyperlink_to("Manifest", sources::performance_asset_url("manifest.json"));
                } else {
                    ui.add_enabled_ui(!busy, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("GitHub repository");
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.repository)
                                    .hint_text(REPOSITORY),
                            );
                            if response.changed() {
                                self.settings.catalog_url = catalog_url(&self.settings.repository);
                                self.invalidate_catalog();
                                self.persist();
                            }
                            if response.lost_focus() {
                                self.check_updates(ctx);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Catalog URL");
                            let response = ui.text_edit_singleline(&mut self.settings.catalog_url);
                            if response.changed() {
                                self.invalidate_catalog();
                                self.persist();
                            }
                            if response.lost_focus() {
                                self.check_updates(ctx);
                            }
                        });
                        ui.hyperlink_to("Catalog", source);
                    });
                }
            });
    }
    fn render(&mut self, root_ui: &mut egui::Ui) {
        let ctx = &root_ui.ctx().clone();
        self.receive();
        let busy = self.receiver.is_some();
        egui::Panel::top("brand")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(22, 22, 24))
                    .inner_margin(egui::Margin::symmetric(24, 18)),
            )
            .show(root_ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(28., 28.)));
                    ui.label(RichText::new("PaladinsCat Launcher").font(bold_font(20.)));
                });
            });
        egui::Panel::bottom("footer")
            .frame(egui::Frame::new().fill(BG).inner_margin(12))
            .show(root_ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if busy {
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(&self.status)
                            .color(if self.error {
                                Color32::from_rgb(240, 144, 144)
                            } else {
                                MUTED
                            })
                            .size(12.),
                    );
                });
            });
        egui::CentralPanel::default().show(root_ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                card().show(ui, |ui| {
                    ui.set_min_width(ui.available_width()); ui.label(RichText::new("Game installation").font(bold_font(16.)));
                    ui.add_enabled_ui(!busy, |ui| { ui.horizontal(|ui| {
                        let response = ui.add_sized([ui.available_width() - 100., 32.], egui::TextEdit::singleline(&mut self.settings.game_folder).hint_text("Select the folder containing ChaosGame"));
                        if response.lost_focus() { self.persist(); self.refresh_installed(); }
                        if ui.button("Browse…").clicked() && let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.settings.game_folder = path.display().to_string(); self.persist(); self.refresh_installed();
                        }
                    }); });
                    ui.label(RichText::new(format!("Installed: {}", self.installed)).size(12.).color(MUTED));
                });
                ui.add_space(12.);
                card().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add_enabled_ui(!busy, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Game language");
                            let response = ui.add(egui::TextEdit::singleline(&mut self.settings.launch_language).desired_width(55.));
                            if response.changed() { self.persist(); }
                            ui.label("Profile");
                            let response = ui.add(egui::TextEdit::singleline(&mut self.settings.homedir).desired_width(150.));
                            if response.changed() { self.persist(); }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add_enabled(!self.settings.game_folder.is_empty() && (!self.settings.performance_enabled || self.performance_selected.is_some()), egui::Button::new(RichText::new("Play").color(BG)).fill(ACCENT)).clicked() { self.play(ctx); }
                            });
                        });
                        egui::CollapsingHeader::new("Launch settings").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Tempest CLI");
                                let response = ui.text_edit_singleline(&mut self.settings.tempest_cli);
                                if response.changed() { self.persist(); }
                                if ui.button("Browse…").clicked() && let Some(path) = rfd::FileDialog::new().add_filter("Tempest CLI", &["exe"]).pick_file() {
                                    self.settings.tempest_cli = path.display().to_string(); self.persist();
                                }
                            });
                        });
                    });
                });
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, PackageTab::Translation, "Localization");
                    ui.selectable_value(&mut self.tab, PackageTab::Ime, "Korean IME patch");
                    ui.selectable_value(&mut self.tab, PackageTab::Performance, "Performance");
                });
                ui.add_space(6.);
                self.repository_panel(ui, ctx, busy);
                ui.add_space(12.);
                if self.tab == PackageTab::Translation {
                card().show(ui, |ui| {
                    ui.set_min_width(ui.available_width()); ui.label(RichText::new("Translation package").font(bold_font(16.)));
                    ui.add_enabled_ui(!busy, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Language");
                            let mut languages: Vec<_> = self.releases.iter().map(|r| r.locale.clone()).collect();
                            languages.push(self.language.clone()); languages.sort(); languages.dedup();
                            egui::ComboBox::from_id_salt("language").selected_text(&self.language).show_ui(ui, |ui| {
                                for language in languages { if ui.selectable_value(&mut self.language, language.clone(), language).changed() {
                                    self.release_index = self.releases.iter().position(|r| r.locale == self.language).unwrap_or(0);
                                }}
                            });
                            if ui.button("Import package…").clicked() && let Some(path) = rfd::FileDialog::new().add_filter("Package manifest", &["json"]).pick_file() {
                                let cache = engine::data_dir(); self.work(ctx, move |p| engine::import(&path, &cache, p).map(|package| Work::Package(Box::new(package))));
                            }
                        });
                        egui::ComboBox::from_id_salt("version").width(ui.available_width()).selected_text(self.releases.get(self.release_index)
                            .filter(|r| r.locale == self.language).map(|r| format!("{} · {}", r.bundle_id, r.game_build))
                            .or_else(|| self.selected.as_ref().map(|p|format!("Local · {}",p.package.bundle_id))).unwrap_or_else(|| "No published package selected".into()))
                            .show_ui(ui, |ui| { for (i, release) in self.releases.iter().enumerate().filter(|(_, r)| r.locale == self.language) {
                                ui.selectable_value(&mut self.release_index, i, format!("{} · {}", release.bundle_id, release.game_build));
                            }});
                        ui.horizontal(|ui| {
                            let release = self.releases.get(self.release_index).filter(|r| r.locale == self.language).cloned();
                            if ui.add_enabled(release.is_some(), egui::Button::new("Download package")).clicked() && let Some(release) = release {
                                let cache = engine::data_dir(); self.work(ctx, move |p| engine::download(&release, &cache, p).map(|package| Work::Package(Box::new(package))));
                            }
                            if let Some(selected) = &self.selected { ui.label(RichText::new(format!("Prepared: {}", selected.package.bundle_id)).color(ACCENT).size(12.)); }
                        });
                    });
                    if let Some(notes) = self.selected.as_ref().map(|p| p.package.notes.as_str()).filter(|n| !n.is_empty())
                        .or_else(|| self.releases.get(self.release_index).map(|r| r.notes.as_str()).filter(|n|!n.is_empty())) {
                        ui.label(RichText::new(notes).size(12.).color(MUTED));
                    }
                    if self.releases.is_empty() && self.selected.is_none() {
                        ui.label(RichText::new("No localization release is published in this catalog.").size(12.).color(MUTED));
                    }
                });
                ui.add_space(12.);
                card().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("Apply & recovery").font(bold_font(16.)));
                    ui.label(RichText::new("Close Paladins before applying or restoring files.").size(12.).color(MUTED));
                    ui.add_enabled_ui(!busy && !self.settings.game_folder.is_empty(), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let ready = self.selected.as_ref().is_some_and(|p| p.package.locale == self.language);
                            if ui.add_enabled(ready, egui::Button::new(RichText::new("Apply / Update").color(BG)).fill(ACCENT)).clicked() { self.action(ctx, Action::Apply); }
                            if ui.button("Previous version").clicked() { self.action(ctx, Action::Rollback); }
                            if ui.button("Restore originals").clicked() { self.action(ctx, Action::Restore); }
                            if engine::recovery_pending(&PathBuf::from(&self.settings.game_folder)) && ui.button("Recover interrupted operation").clicked() { self.action(ctx, Action::Recover); }
                        });
                    });
                });
                }
                ui.add_space(12.);
                if self.tab == PackageTab::Ime {
                card().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("Korean IME compatibility").font(bold_font(16.)));
                    ui.label(RichText::new(format!("Configuration: {}", self.ime_installed.label())).size(12.).color(MUTED));
                    ui.add_enabled_ui(!busy, |ui| {
                        egui::ComboBox::from_id_salt("ime-version").width(ui.available_width())
                            .selected_text(self.ime_releases.get(self.ime_index).map(|r|format!("{} · {}",r.version,r.game_build))
                                .or_else(|| self.ime_selected.as_ref().map(|c|format!("Local{} · {}",if c.package.draft {" draft"} else {""},c.package.version))).unwrap_or_else(||"No published IME package selected".into()))
                            .show_ui(ui, |ui| { for (i,r) in self.ime_releases.iter().enumerate() { ui.selectable_value(&mut self.ime_index,i,format!("{} · {}",r.version,r.game_build)); } });
                        ui.horizontal_wrapped(|ui| {
                            let release = self.ime_releases.get(self.ime_index).cloned();
                            if ui.add_enabled(release.is_some(),egui::Button::new("Download IME package")).clicked() && let Some(release)=release {
                                let cache=engine::data_dir(); self.work(ctx,move |p|ime::download(&release,&cache,p).map(|c|Work::ImePackage(Box::new(c))));
                            }
                            if ui.button("Import IME package…").clicked() && let Some(path)=rfd::FileDialog::new().add_filter("IME package manifest", &["json"]).pick_file() {
                                let cache=engine::data_dir(); self.work(ctx,move |_|ime::import(&path,&cache).map(|c|Work::ImePackage(Box::new(c))));
                            }
                        });
                        if let Some(c)=&self.ime_selected { ui.label(RichText::new(format!("Prepared: {}{}",c.package.version,if c.package.draft {" · local draft"} else {""})).size(12.).color(ACCENT)); }
                        if self.ime_releases.is_empty() && self.ime_selected.is_none() { ui.label(RichText::new("No IME release is published in this catalog.").size(12.).color(MUTED)); }
                        if let Some(c)=&self.ime_selected { ui.label(RichText::new(&c.package.notes).size(12.).color(MUTED)); }
                        ui.add_enabled_ui(!self.settings.game_folder.is_empty(),|ui| { ui.horizontal_wrapped(|ui| {
                            let recovery = !self.preview && ime::recovery_pending(&PathBuf::from(&self.settings.game_folder));
                            let prepared = self.ime_selected.is_some();
                            let mut enabled = self.ime_installed.enabled();
                            let response = ui.add_enabled(self.ime_installed.can_toggle(prepared, recovery), egui::Checkbox::new(&mut enabled, "Enable Korean IME fix"));
                            if response.changed() && let Some(action) = self.ime_installed.toggle_action(enabled, prepared, recovery) { self.ime_action(ctx, action); }
                            if self.ime_installed.update_available(self.ime_selected.as_ref()) && ui.add_enabled(!recovery, egui::Button::new("Update IME package")).clicked() { self.ime_action(ctx, ime::Action::Apply); }
                            if recovery && ui.button("Recover IME operation").clicked() { self.ime_action(ctx,ime::Action::Recover); }
                        }); });
                    });
                    ui.label(RichText::new("Close Paladins to change this setting. Play loads the enabled fix on the next launch.").size(12.).color(MUTED));
                });
                }
                if self.tab == PackageTab::Performance {
                    card().show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(RichText::new("Performance mod").font(bold_font(16.)));
                        ui.add_enabled_ui(!busy, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Download performance package").clicked() {
                                    let cache = engine::data_dir(); self.work(ctx, move |p| performance::download(&cache, p).map(|c| Work::PerformancePackage(Box::new(c))));
                                }
                                if ui.button("Import performance package…").clicked() && let Some(path) = rfd::FileDialog::new().add_filter("Performance manifest", &["json"]).pick_file() {
                                    let cache = engine::data_dir(); self.work(ctx, move |_| performance::import(&path, &cache).map(|c| Work::PerformancePackage(Box::new(c))));
                                }
                            });
                            if let Some(package) = &self.performance_selected { ui.label(RichText::new(format!("Prepared: v{}", package.package.version)).color(ACCENT).size(12.)); }
                            if ui.add_enabled(self.performance_selected.is_some() || self.settings.performance_enabled, egui::Checkbox::new(&mut self.settings.performance_enabled, "Enable performance mod on next launch")).changed() { self.persist(); }
                            if ui.add_enabled(self.settings.performance_enabled, egui::Checkbox::new(&mut self.settings.rollback_input, "Use previous input version")).changed() { self.persist(); }
                        });
                        ui.label(RichText::new("Performance gains and combined IME compatibility remain unverified.").size(12.).color(MUTED));
                    });
                }
                ui.add_space(12.);
            });
        });
    }
}
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.render(ui);
    }
}
fn main() -> eframe::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h") {
        println!(
            "PaladinsCat Launcher\n  --preview|--preview-ime|--preview-performance OUTPUT.png\n  --inspect-startup OUTPUT.json\nUse paladinscat-launcher-cli for explicit download, --dry-run and --launch operations."
        );
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--inspect-startup" {
        let settings = Settings::load();
        let startup = Startup::load(&settings, &engine::data_dir(), &|_| {});
        let report = serde_json::json!({
            "repository": settings.repository,
            "catalog_url": settings.source_url(),
            "catalog": startup.catalog.as_ref().map(|c|serde_json::json!({"translations":c.packages.len(),"ime_packages":c.ime_packages.len()})),
            "translation": startup.translation.as_ref().map(|p|p.as_ref().map(|p|p.package.bundle_id.clone())),
            "ime": startup.ime.as_ref().map(|p|p.as_ref().map(|p|serde_json::json!({"version":p.package.version,"draft":p.package.draft,"dll_sha256":p.package.dll_sha256}))),
            "performance": startup.performance.as_ref().map(|p|p.as_ref().map(|p|p.package.version.clone())),
            "feature_sources": {
                "localization": {"repository": settings.repository, "url": settings.source_url(), "packages": startup.catalog.as_ref().map(|c|c.packages.len())},
                "ime": {"repository": settings.repository, "url": settings.source_url(), "packages": startup.catalog.as_ref().map(|c|c.ime_packages.len())},
                "performance": {"repository": performance::REPOSITORY, "url": performance::RELEASE_URL, "version": performance::VERSION}
            },
            "cpu_only": true,
            "game_files_changed": false,
        });
        engine::save_settings(&PathBuf::from(&args[2]), &report)
            .map_err(|e| eframe::Error::AppCreation(std::io::Error::other(e).into()))?;
        return Ok(());
    }
    if args.len() == 3
        && matches!(
            args[1].as_str(),
            "--preview"
                | "--preview-ime"
                | "--preview-performance"
                | "--preview-prepared"
                | "--preview-ime-prepared"
                | "--preview-performance-prepared"
        )
    {
        preview::render(
            &PathBuf::from(&args[2]),
            if args[1].contains("-performance") {
                PackageTab::Performance
            } else if args[1].contains("-ime") {
                PackageTab::Ime
            } else {
                PackageTab::Translation
            },
            args[1].ends_with("-prepared"),
        )
        .expect("Headless GUI preview");
        return Ok(());
    }
    if args.len() != 1 {
        return Err(eframe::Error::AppCreation(
            std::io::Error::other("Unknown launcher arguments; use --help").into(),
        ));
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/paladinscat.png"))
        .map_err(|e| eframe::Error::AppCreation(e.into()))?;
    let (wgpu_options, selected) = graphics_device::configuration()
        .map_err(|e| eframe::Error::AppCreation(std::io::Error::other(e).into()))?;
    eframe::run_native(
        "PaladinsCat Launcher",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            wgpu_options,
            viewport: egui::ViewportBuilder::default()
                .with_icon(icon)
                .with_inner_size([880., 960.])
                .with_min_inner_size([720., 640.]),
            ..Default::default()
        },
        Box::new(move |cc| {
            graphics_device::verify_renderer(cc, &selected).map_err(std::io::Error::other)?;
            Ok(Box::new(App::new(&cc.egui_ctx, false)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_patcher_settings_preserve_selected_packages_without_enabling_new_mods() {
        let settings: Settings = serde_json::from_str(r#"{"game_folder":"C:/Paladins","repository":"PaladinsCat/PaladinsCat-locales","catalog_url":"","prepared_translation":"bundle.json","prepared_ime":"ime-package.json"}"#).unwrap();
        assert_eq!(settings.prepared_translation, "bundle.json");
        assert_eq!(settings.prepared_ime, "ime-package.json");
        assert!(!settings.performance_enabled);
        assert!(!settings.rollback_input);
        assert_eq!(settings.launch_language, "INT");
        assert_eq!(settings.homedir, "PaladinsCat");
        assert_eq!(settings.source_url(), catalog_url(REPOSITORY));

        // A profile change selects a different game home, including its replays.
        // New defaults must not redirect an explicitly saved existing profile.
        let mut saved = serde_json::to_value(&settings).unwrap();
        saved["launch_language"] = serde_json::json!("KOR");
        saved["homedir"] = serde_json::json!("existing-profile");
        let restored: Settings = serde_json::from_value(saved).unwrap();
        assert_eq!(restored.launch_language, "KOR");
        assert_eq!(restored.homedir, "existing-profile");
        assert_eq!(restored.prepared_translation, "bundle.json");
        assert_eq!(restored.prepared_ime, "ime-package.json");
    }

    #[test]
    fn disabled_performance_does_not_pass_a_stale_rollback_selection() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, true);
        app.settings.rollback_input = true;
        let options = app.launch_options();
        assert!(options.performance.is_none());
        assert!(!options.rollback_input);
    }

    fn ime_package(version: &str) -> ime::Cached {
        ime::Cached {
            package: ime::Package {
                schema_version: 1,
                package_id: format!("paladinscat-ime-{}", version.replace('.', "-")),
                version: version.into(),
                game_build: "8.1".into(),
                platform: "Win64".into(),
                locale: "KOR".into(),
                native_abi: [4, 9, 7, 1],
                game_fingerprint: engine::Fingerprint {
                    path: "Binaries/Win64/Paladins.exe".into(),
                    sha256: "a".repeat(64),
                },
                dll_sha256: "b".repeat(64),
                dll_bytes: 2048,
                source_commit: "c".repeat(40),
                draft: false,
                notes: String::new(),
                download: None,
            },
            directory: PathBuf::new(),
        }
    }

    #[test]
    fn ime_toggle_uses_configuration_instead_of_download_presence() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, true);
        app.accept_ime(ime_package("0.1.0"));
        assert!(!app.ime_installed.enabled());
        assert!(matches!(
            app.ime_installed.toggle_action(true, true, false),
            Some(ime::Action::Apply)
        ));
        // A request does not optimistically change the displayed configuration.
        assert!(!app.ime_installed.enabled());
        app.ime_installed =
            ImeInstallState::Enabled(Box::new(app.ime_selected.take().unwrap().package));
        // A verified enabled mod can be disabled even without a prepared download.
        assert!(matches!(
            app.ime_installed.toggle_action(false, false, false),
            Some(ime::Action::Remove)
        ));
        assert!(
            app.ime_installed
                .toggle_action(true, false, false)
                .is_none()
        );
    }

    #[test]
    fn unknown_and_interrupted_ime_configurations_disable_the_toggle() {
        let invalid = ImeInstallState::Invalid("modified DLL".into());
        assert!(!invalid.can_toggle(true, false));
        assert!(invalid.toggle_action(true, true, false).is_none());
        assert!(!ImeInstallState::NoGame.can_toggle(true, false));
        assert!(!ImeInstallState::Disabled.can_toggle(false, false));
        let enabled = ImeInstallState::Enabled(Box::new(ime_package("0.1.0").package));
        assert!(!enabled.can_toggle(true, true));
        assert!(enabled.toggle_action(false, true, true).is_none());
        assert!(
            ImeInstallState::Disabled
                .toggle_action(true, true, true)
                .is_none()
        );
    }

    #[test]
    fn preparing_another_version_does_not_disable_or_implicitly_update_the_mod() {
        let old = ime_package("0.1.0");
        let enabled = ImeInstallState::Enabled(Box::new(old.package.clone()));
        assert!(!enabled.update_available(Some(&old)));
        assert!(enabled.update_available(Some(&ime_package("0.2.0"))));
        assert!(enabled.enabled());
        assert!(enabled.toggle_action(true, true, false).is_none());
        assert!(!ImeInstallState::Disabled.update_available(Some(&old)));
    }

    #[test]
    fn configured_catalog_is_used_with_repository_link_retained() {
        let mut settings = Settings {
            catalog_url: "https://github.com/PaladinsCat/PaladinsCat-locales/releases/download/preview/catalog.json".into(),
            ..Settings::default()
        };
        assert_eq!(settings.source_url(), settings.catalog_url);
        assert_eq!(settings.repository, REPOSITORY);
        settings.catalog_url.clear();
        assert_eq!(settings.source_url(), catalog_url(REPOSITORY));
    }

    #[test]
    fn empty_and_failed_catalogs_have_distinct_visible_statuses() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, true);
        let deliver = |app: &mut App, result| {
            let (sender, receiver) = mpsc::channel();
            sender.send(Event::Done(result)).unwrap();
            app.receiver = Some(receiver);
            app.receive();
        };
        deliver(
            &mut app,
            Ok(Work::Catalog(
                engine::parse_catalog(br#"{"schema_version":1,"packages":[],"ime_packages":[]}"#)
                    .unwrap(),
            )),
        );
        assert_eq!(
            app.catalog_status,
            "Connected · no downloadable packages published"
        );
        assert!(!app.error);
        assert_eq!(
            app.tab_source(PackageTab::Translation).2,
            "Connected · no localization packages published"
        );
        assert_eq!(
            app.tab_source(PackageTab::Ime).2,
            "Connected · no IME packages published"
        );
        app.catalog_status = "Connecting to GitHub…".into();
        deliver(&mut app, Err("offline".into()));
        assert_eq!(app.catalog_status, "Catalog unavailable");
        assert_eq!(app.status, "offline");
        assert!(app.error);
        assert_eq!(
            app.tab_source(PackageTab::Translation).2,
            "Catalog unavailable"
        );
        assert_eq!(
            app.tab_source(PackageTab::Performance).2,
            "Pinned release · v0.1.0"
        );
    }

    fn catalog_fixture(translation: bool, ime: bool) -> engine::Catalog {
        let mut value = serde_json::json!({"schema_version":1,"packages":[],"ime_packages":[]});
        if translation {
            value["packages"] = serde_json::json!([{"bundle_id":"kor-fixture","locale":"KOR","game_build":"8.1","manifest_url":"https://github.com/PaladinsCat/PaladinsCat-locales/releases/download/fixture/locale-package.json","manifest_sha256":"a".repeat(64)}]);
        }
        if ime {
            value["ime_packages"] = serde_json::json!([{"package_id":"ime-fixture","version":"0.1.0","game_build":"8.1","manifest_url":"https://github.com/PaladinsCat/PaladinsCat-locales/releases/download/fixture/ime-package.json","manifest_sha256":"b".repeat(64)}]);
        }
        engine::parse_catalog(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    #[test]
    fn feature_counts_are_independent_and_downloads_do_not_hide_releases() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, true);
        app.accept_catalog(catalog_fixture(true, false));
        assert_eq!(
            app.tab_source(PackageTab::Translation).2,
            "Connected · 1 localization package"
        );
        assert_eq!(
            app.tab_source(PackageTab::Ime).2,
            "Connected · no IME packages published"
        );
        app.accept_catalog(catalog_fixture(false, true));
        assert_eq!(
            app.tab_source(PackageTab::Translation).2,
            "Connected · no localization packages published"
        );
        assert_eq!(
            app.tab_source(PackageTab::Ime).2,
            "Connected · 1 IME package"
        );
        app.accept_ime(ime_package("0.1.0"));
        assert_eq!(app.ime_releases.len(), 1);
        assert_eq!(
            app.tab_source(PackageTab::Ime).2,
            "Connected · 1 IME package"
        );
    }

    #[test]
    fn changing_catalog_source_invalidates_only_its_published_lists() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, true);
        app.accept_catalog(catalog_fixture(true, true));
        app.accept_ime(ime_package("0.1.0"));
        app.settings.repository = "example/custom-locales".into();
        app.settings.catalog_url = catalog_url(&app.settings.repository);
        app.invalidate_catalog();
        assert!(app.releases.is_empty() && app.ime_releases.is_empty());
        assert!(app.ime_selected.is_some());
        let (repository, url, status) = app.tab_source(PackageTab::Ime);
        assert_eq!(repository, "example/custom-locales");
        assert!(url.contains("example/custom-locales/main/"));
        assert_eq!(status, "Catalog not checked");
        assert_eq!(
            app.tab_source(PackageTab::Performance).0,
            performance::REPOSITORY
        );
    }

    #[test]
    fn catalog_completion_after_tab_switch_keeps_the_performance_source() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, true);
        let (sender, receiver) = mpsc::channel();
        app.receiver = Some(receiver);
        app.tab = PackageTab::Performance;
        sender
            .send(Event::Done(Ok(Work::Catalog(catalog_fixture(true, true)))))
            .unwrap();
        app.receive();
        assert_eq!(app.tab_source(app.tab).0, performance::REPOSITORY);
        assert_eq!(app.tab_source(app.tab).2, "Pinned release · v0.1.0");
        assert_eq!(app.status, "Catalog refreshed");
        assert_eq!(app.releases.len(), 1);
        assert_eq!(app.ime_releases.len(), 1);
    }

    #[test]
    fn repository_is_rendered_below_tabs_and_only_for_the_selected_feature() {
        fn text(shape: &egui::epaint::Shape, result: &mut String) {
            match shape {
                egui::epaint::Shape::Text(value) => {
                    result.push_str(value.galley.text());
                    result.push('\n');
                }
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        text(shape, result);
                    }
                }
                _ => {}
            }
        }
        for tab in [
            PackageTab::Translation,
            PackageTab::Ime,
            PackageTab::Performance,
        ] {
            let ctx = egui::Context::default();
            let mut app = App::new(&ctx, true);
            app.tab = tab;
            app.accept_catalog(catalog_fixture(true, true));
            let mut result = String::new();
            for _ in 0..3 {
                result.clear();
                let mut frame = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(880., 960.),
                        )),
                        ..Default::default()
                    },
                    |ui| app.render(ui),
                );
                for shape in frame.shapes {
                    text(&shape.shape, &mut result);
                }
                // This CPU assertion inspects text only, without a texture renderer.
                frame.textures_delta.clear();
            }
            let repository = if tab == PackageTab::Performance {
                performance::REPOSITORY
            } else {
                REPOSITORY
            };
            assert_eq!(result.matches(repository).count(), 1, "{result}");
            assert!(
                result.find("Performance").unwrap() < result.find(repository).unwrap(),
                "{result}"
            );
            if tab == PackageTab::Performance {
                assert!(!result.contains(REPOSITORY), "{result}");
                assert!(!result.contains("no downloadable"), "{result}");
            }
        }
    }
}
