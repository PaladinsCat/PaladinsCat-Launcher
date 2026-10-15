//! Read installed-game records, never recursively scan drives or modify a store.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

const PALADINS_APP_ID: &str = "444090";
const MAX_RECORD_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Store {
    Steam,
    Epic,
    Tempest,
}
impl Store {
    pub fn label(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Epic => "Epic Games",
            Self::Tempest => "Tempest",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Installation {
    pub path: PathBuf,
    pub store: Store,
}

/// Detection is deliberately weaker than Play's compatibility/fingerprint gates.
/// Finding an installation never authorizes a package install or game launch.
pub fn is_game_root(path: &Path) -> bool {
    path.is_absolute()
        && ["Binaries", "Engine", "ChaosGame"]
            .iter()
            .all(|dir| path.join(dir).is_dir())
        && path.join("Binaries/Win64/Paladins.exe").is_file()
}

/// Preserve an existing valid selection. With several installs, require a choice.
pub fn automatic_selection<'a>(
    saved: &str,
    installs: &'a [Installation],
) -> Option<&'a Installation> {
    if !is_game_root(Path::new(saved)) && installs.len() == 1 && is_game_root(&installs[0].path) {
        installs.first()
    } else {
        None
    }
}

#[derive(Default)]
struct Locations {
    steam: Vec<PathBuf>,
    epic: Vec<PathBuf>,
    tempest: Vec<PathBuf>,
}

pub fn discover() -> Vec<Installation> {
    discover_at(&system_locations())
}

fn record(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_RECORD_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn path_identity(path: &Path) -> Option<String> {
    let value = path.canonicalize().ok()?.to_string_lossy().into_owned();
    #[cfg(windows)]
    let value = value.to_lowercase();
    Some(value)
}

fn discover_at(locations: &Locations) -> Vec<Installation> {
    let mut installs = Vec::new();
    let mut identities = BTreeSet::new();
    let mut add = |path: PathBuf, store| {
        if is_game_root(&path)
            && let Some(identity) = path_identity(&path)
            && identities.insert(identity)
        {
            installs.push(Installation { path, store });
        }
    };
    let mut libraries = BTreeSet::new();
    for steam in &locations.steam {
        libraries.insert(steam.clone());
        // Both locations have been used by Steam; never depend on drive letters.
        for relative in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
            if let Some(text) = record(&steam.join(relative)) {
                libraries.extend(steam_libraries(&text));
            }
        }
    }
    for library in libraries {
        let apps = library.join("steamapps");
        if let Some(text) = record(&apps.join(format!("appmanifest_{PALADINS_APP_ID}.acf")))
            && let Some(directory) = steam_install_dir(&text)
        {
            add(apps.join("common").join(directory), Store::Steam);
        }
    }
    for directory in &locations.epic {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        // Bounded, deterministic inventory of manifests only, not game folders.
        let mut files: Vec<_> = entries
            .take(2048)
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("item"))
            })
            .collect();
        files.sort();
        for file in files {
            if let Some(text) = record(&file)
                && let Ok(manifest) = serde_json::from_str::<EpicManifest>(&text)
                && !manifest.incomplete
            {
                // File/layout validation identifies Paladins even if Epic changes
                // its opaque AppName. Never treat unrelated games as candidates.
                add(PathBuf::from(manifest.install_location), Store::Epic);
            }
        }
    }
    for settings in &locations.tempest {
        if let Some(text) = record(settings) {
            for path in tempest_paths(&text) {
                add(path, Store::Tempest);
            }
        }
    }
    installs
}

#[derive(Deserialize)]
struct EpicManifest {
    #[serde(rename = "InstallLocation")]
    install_location: String,
    #[serde(default, rename = "bIsIncompleteInstall")]
    incomplete: bool,
}

fn tempest_paths(text: &str) -> Vec<PathBuf> {
    #[derive(Deserialize)]
    struct Instance {
        path: PathBuf,
        state: State,
        #[serde(rename = "appId")]
        app_id: Option<u64>,
    }
    #[derive(Deserialize)]
    struct State {
        r#type: String,
    }
    let Ok(settings) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text)
    else {
        return Vec::new();
    };
    settings
        .into_iter()
        .filter_map(|(key, value)| {
            if !key.starts_with("instances:") {
                return None;
            }
            // Tempest's persistedMapState stores JSON-encoded instance strings.
            let instance: Instance = serde_json::from_str(value.as_str()?).ok()?;
            (instance.state.r#type == "prepared" && instance.app_id.is_none_or(|id| id == 444090))
                .then_some(instance.path)
        })
        .collect()
}

// A small bounded KeyValues reader handles modern objects, legacy string paths,
// comments and escaped Windows paths without matching unrelated nested strings.
enum Value {
    Text(String),
    Object(Vec<(String, Value)>),
}
impl Value {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Object(values) => values
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Self::Text(_) => None,
        }
    }
    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }
}

struct KeyValues<'a> {
    input: &'a [u8],
    offset: usize,
}
impl<'a> KeyValues<'a> {
    fn parse(text: &'a str) -> Option<Value> {
        let mut reader = Self {
            input: text.trim_start_matches('\u{feff}').as_bytes(),
            offset: 0,
        };
        reader.object(0, false)
    }
    fn whitespace(&mut self) {
        loop {
            while self
                .input
                .get(self.offset)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.offset += 1;
            }
            if self.input.get(self.offset..self.offset + 2) != Some(b"//") {
                break;
            }
            while self.input.get(self.offset).is_some_and(|c| *c != b'\n') {
                self.offset += 1;
            }
        }
    }
    fn string(&mut self) -> Option<String> {
        self.whitespace();
        let first = *self.input.get(self.offset)?;
        if matches!(first, b'{' | b'}') {
            return None;
        }
        let quoted = first == b'"';
        if quoted {
            self.offset += 1;
        }
        let mut bytes = Vec::new();
        while let Some(&byte) = self.input.get(self.offset) {
            if quoted && byte == b'"' {
                self.offset += 1;
                return String::from_utf8(bytes).ok();
            }
            if !quoted && (byte.is_ascii_whitespace() || matches!(byte, b'{' | b'}')) {
                return String::from_utf8(bytes).ok();
            }
            self.offset += 1;
            if quoted
                && byte == b'\\'
                && self
                    .input
                    .get(self.offset)
                    .is_some_and(|c| matches!(c, b'\\' | b'"'))
            {
                bytes.push(self.input[self.offset]);
                self.offset += 1;
            } else {
                bytes.push(byte);
            }
        }
        (!quoted).then(|| String::from_utf8(bytes).ok()).flatten()
    }
    fn object(&mut self, depth: usize, nested: bool) -> Option<Value> {
        if depth > 32 {
            return None;
        }
        let mut values = Vec::new();
        loop {
            self.whitespace();
            match self.input.get(self.offset) {
                None => return (!nested).then_some(Value::Object(values)),
                Some(b'}') => {
                    self.offset += 1;
                    return nested.then_some(Value::Object(values));
                }
                _ => {}
            }
            let key = self.string()?;
            self.whitespace();
            let value = if self.input.get(self.offset) == Some(&b'{') {
                self.offset += 1;
                self.object(depth + 1, true)?
            } else {
                Value::Text(self.string()?)
            };
            values.push((key, value));
        }
    }
}

fn steam_libraries(text: &str) -> Vec<PathBuf> {
    let Some(Value::Object(entries)) = KeyValues::parse(text).and_then(|root| match root {
        Value::Object(mut entries) => entries
            .iter()
            .position(|(k, _)| k.eq_ignore_ascii_case("libraryfolders"))
            .map(|i| entries.swap_remove(i).1),
        _ => None,
    }) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|(key, value)| {
            if key.parse::<u32>().is_err() {
                return None;
            }
            let path = value.text().or_else(|| value.get("path")?.text())?;
            let path = PathBuf::from(path);
            path.is_absolute().then_some(path)
        })
        .collect()
}

fn steam_install_dir(text: &str) -> Option<PathBuf> {
    let root = KeyValues::parse(text)?;
    let app = root.get("AppState")?;
    if app.get("appid")?.text()? != PALADINS_APP_ID {
        return None;
    }
    let directory = app.get("installdir")?.text()?;
    // Steam's installdir is relative to steamapps/common. Reject traversal and
    // foreign absolute-path syntax even on non-Windows fixture runners.
    let path = PathBuf::from(directory);
    if directory.is_empty()
        || directory.contains([':', '\\', '/'])
        || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return None;
    }
    Some(path)
}

#[cfg(windows)]
fn system_locations() -> Locations {
    use windows::Win32::UI::Shell::{
        FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86,
        FOLDERID_RoamingAppData,
    };
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let mut locations = Locations::default();
    for (key, subkey, name) in [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ] {
        if let Some(path) = registry_path(key, subkey, name) {
            locations.steam.push(path);
        }
    }
    for folder in [&FOLDERID_ProgramFilesX86, &FOLDERID_ProgramFiles] {
        if let Some(path) = known_folder(folder) {
            locations.steam.push(path.join("Steam"));
        }
    }
    if let Some(path) = known_folder(&FOLDERID_ProgramData) {
        locations
            .epic
            .push(path.join("Epic/EpicGamesLauncher/Data/Manifests"));
    }
    if let Some(path) = known_folder(&FOLDERID_RoamingAppData) {
        locations
            .tempest
            .push(path.join("com.lowrezstudio.tempest/settings.json"));
    }
    locations
}

#[cfg(windows)]
fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{KF_FLAG_NO_PACKAGE_REDIRECTION, SHGetKnownFolderPath},
    };
    let value = unsafe { SHGetKnownFolderPath(id, KF_FLAG_NO_PACKAGE_REDIRECTION, None) }.ok()?;
    let path = PathBuf::from(OsString::from_wide(unsafe { value.as_wide() }));
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    Some(path)
}

#[cfg(windows)]
fn registry_path(
    key: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
    name: &str,
) -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::System::Registry::{RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW};
    let subkey: Vec<_> = subkey.encode_utf16().chain(Some(0)).collect();
    let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
    let mut buffer = vec![0u16; 32768];
    let mut bytes = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            key,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if result != 0 {
        return None;
    }
    let length = buffer.iter().position(|c| *c == 0)?;
    (length > 0).then(|| PathBuf::from(OsString::from_wide(&buffer[..length])))
}

#[cfg(not(windows))]
fn system_locations() -> Locations {
    Locations::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-game-discovery")
                .join(format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn file(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }
        fn game(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            for directory in ["Binaries/Win64", "Engine", "ChaosGame"] {
                fs::create_dir_all(path.join(directory)).unwrap();
            }
            fs::write(
                path.join("Binaries/Win64/Paladins.exe"),
                b"fixture, never executed",
            )
            .unwrap();
            path
        }
        fn steam(&self, library: &str, directory: &str) -> PathBuf {
            let game = self.game(&format!("{library}/steamapps/common/{directory}"));
            self.file(
                &format!("{library}/steamapps/appmanifest_444090.acf"),
                &format!(r#""AppState" {{ "appid" "444090" "installdir" "{directory}" }}"#),
            );
            game
        }
        fn epic(&self, name: &str, game: &Path, incomplete: bool) {
            self.file(
                &format!("epic/{name}.item"),
                &serde_json::json!({
                    "AppName": "opaque-catalog-id", "DisplayName": "Paladins",
                    "InstallLocation": game, "bIsIncompleteInstall": incomplete,
                })
                .to_string(),
            );
        }
    }

    #[test]
    fn steam_follows_modern_and_legacy_libraries_with_spaces_unicode_and_escapes() {
        let f = Fixture::new();
        let first = f.steam("client", "Paladins");
        let second = f.steam("other drive/게임", "팔라딘");
        let legacy = f.steam("legacy", "Season 8.1");
        let quote = |path: &Path| path.to_string_lossy().replace('\\', "\\\\");
        f.file(
            "client/steamapps/libraryfolders.vdf",
            &format!(
                r#"
            // Steam metadata includes unrelated nested numbers.
            "libraryfolders" {{
                "0" {{ "path" "{}" "apps" {{ "444090" "123456" }} }}
                "1" {{ "path" "{}" }}
                "2" "{}"
                "TimeNextStatsReport" "123456"
            }}"#,
                quote(&f.0.join("client")),
                quote(&f.0.join("other drive/게임")),
                quote(&f.0.join("legacy"))
            ),
        );
        let installs = discover_at(&Locations {
            steam: vec![f.0.join("client")],
            ..Default::default()
        });
        assert_eq!(installs.len(), 3);
        for path in [first, second, legacy] {
            assert!(
                installs
                    .iter()
                    .any(|install| install.path == path && install.store == Store::Steam)
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn steam_library_paths_keep_c_d_e_drives_and_unc_without_scanning_them() {
        let paths = steam_libraries(
            r#""libraryfolders" {
            "0" { "path" "C:\\Program Files (x86)\\Steam" }
            "1" { "path" "D:\\SteamLibrary" }
            "2" "E:\\Games\\Steam"
            "3" { "path" "\\\\server\\share\\Steam" }
        }"#,
        );
        assert_eq!(
            paths,
            [
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary"),
                PathBuf::from(r"E:\Games\Steam"),
                PathBuf::from(r"\\server\share\Steam"),
            ]
        );
    }

    #[test]
    fn steam_supports_old_config_location_and_rejects_unrelated_or_unsafe_manifests() {
        let f = Fixture::new();
        let game = f.steam("second", "Paladins");
        f.file(
            "client/config/libraryfolders.vdf",
            &format!(
                "\"LibraryFolders\" {{ \"1\" {} }}",
                serde_json::to_string(&f.0.join("second")).unwrap()
            ),
        );
        let installs = discover_at(&Locations {
            steam: vec![f.0.join("client")],
            ..Default::default()
        });
        assert_eq!(installs[0].path, game);
        for directory in [
            "../escape",
            "..\\escape",
            "C:\\Elsewhere",
            "/absolute",
            "",
            ".",
            "..",
        ] {
            assert!(
                steam_install_dir(&format!(
                    r#""AppState" {{ "appid" "444090" "installdir" "{directory}" }}"#
                ))
                .is_none()
            );
        }
        assert!(
            steam_install_dir(r#""AppState" { "appid" "9999" "installdir" "Paladins" }"#).is_none()
        );
        assert!(
            steam_install_dir(r#""AppState" { "appid" "444090" "installdir" "Paladins" "#)
                .is_none()
        );
        assert!(steam_libraries(r#""libraryfolders" { "0" { "path" "relative" } }"#).is_empty());
    }

    #[test]
    fn epic_requires_complete_existing_paladins_and_ignores_bad_records() {
        let f = Fixture::new();
        let game = f.game("Epic Games/Paladins");
        f.epic("complete", &game, false);
        f.epic("incomplete", &f.game("incomplete"), true);
        f.epic("removed", &f.0.join("removed"), false);
        f.epic("other-game", &f.0, false);
        f.file("epic/broken.item", "{truncated");
        let installs = discover_at(&Locations {
            epic: vec![f.0.join("epic")],
            ..Default::default()
        });
        assert_eq!(
            installs,
            [Installation {
                path: game,
                store: Store::Epic
            }]
        );
    }

    #[test]
    fn tempest_reads_only_prepared_instance_paths_without_using_other_settings() {
        let f = Fixture::new();
        let game = f.game("Tempest/Season 8.1");
        let instance = |path: &Path, state: &str, id: Option<u64>| {
            serde_json::json!({
                "path": path, "state": {"type": state}, "appId": id,
            })
            .to_string()
        };
        let settings = f.file("tempest/settings.json", &serde_json::json!({
            "instances:one": instance(&game, "prepared", Some(444090)),
            "instances:legacy": instance(&game, "prepared", None),
            "instances:downloading": instance(&f.game("partial"), "downloading", Some(444090)),
            "instances:other": instance(&f.game("other"), "prepared", Some(1)),
            "instances:invalid": "{broken", "instances:removed": instance(&f.0.join("removed"), "prepared", None),
            "unrelatedSetting": instance(&f.game("unrelated"), "prepared", Some(444090)),
            "defaultInstancePath": f.0,
        }).to_string());
        let installs = discover_at(&Locations {
            tempest: vec![settings],
            ..Default::default()
        });
        assert_eq!(
            installs,
            [Installation {
                path: game,
                store: Store::Tempest
            }]
        );
    }

    #[test]
    fn duplicates_across_stores_collapse_and_broken_steam_does_not_hide_epic() {
        let f = Fixture::new();
        let game = f.steam("steam", "Paladins");
        f.file("steam/steamapps/libraryfolders.vdf", "{bad");
        f.epic("duplicate", &game, false);
        let other = f.game("EpicOnly");
        f.epic("other", &other, false);
        let installs = discover_at(&Locations {
            steam: vec![f.0.join("steam"), f.0.join("steam/.")],
            epic: vec![f.0.join("epic")],
            ..Default::default()
        });
        assert_eq!(installs.len(), 2);
        assert!(installs.contains(&Installation {
            path: game,
            store: Store::Steam
        }));
        assert!(installs.contains(&Installation {
            path: other,
            store: Store::Epic
        }));
    }

    #[test]
    fn saved_selection_is_preserved_single_install_autofills_and_multiple_require_choice() {
        let f = Fixture::new();
        let saved = f.game("custom");
        let installs = vec![Installation {
            path: f.game("steam"),
            store: Store::Steam,
        }];
        assert!(automatic_selection(&saved.display().to_string(), &installs).is_none());
        assert_eq!(automatic_selection("", &installs), installs.first());
        assert_eq!(
            automatic_selection("X:/no-longer-installed", &installs),
            installs.first()
        );
        let mut multiple = installs.clone();
        multiple.push(Installation {
            path: saved,
            store: Store::Tempest,
        });
        assert!(automatic_selection("", &multiple).is_none());
        assert!(automatic_selection("", &[]).is_none());
    }

    #[test]
    fn missing_executable_and_oversized_metadata_are_not_installations() {
        let f = Fixture::new();
        for directory in ["Binaries", "Engine", "ChaosGame"] {
            fs::create_dir_all(f.0.join(directory)).unwrap();
        }
        assert!(!is_game_root(&f.0));
        assert!(!is_game_root(Path::new(".")));
        let path = f.file("oversized.item", "{}");
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_RECORD_BYTES + 1)
            .unwrap();
        assert!(record(&path).is_none());
        assert!(discover_at(&Locations::default()).is_empty());
    }
}
