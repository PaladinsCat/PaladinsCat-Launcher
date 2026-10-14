//! Frontend LanguageMenu geometry/colors, drawn with Rust egui primitives.
//! Globe outline: Lucide Earth/Globe2, ISC; see licenses/Lucide-ISC.txt.
use crate::{ACCENT, MUTED, PANEL, TEXT, bold_font, i18n::UiLanguage};
use egui::{Color32, Id, Pos2, Sense, Stroke, WidgetInfo, WidgetType, vec2};

pub fn button_id() -> Id {
    Id::new("launcher-ui-language")
}
pub fn popup_id() -> Id {
    Id::new("launcher-ui-language-popup")
}
pub fn option_id(language: UiLanguage) -> Id {
    Id::new(("launcher-ui-language-option", language.code()))
}

pub fn show(ui: &mut egui::Ui, language: UiLanguage) -> Option<UiLanguage> {
    let (rect, _) = ui.allocate_exact_size(vec2(84., 36.), Sense::hover());
    let response = ui.interact(rect, button_id(), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            language.text("Interface language"),
        )
    });
    let active = response.hovered() || response.has_focus();
    let color = if active { ACCENT } else { MUTED };
    if active {
        ui.painter().rect_filled(rect, 8, PANEL);
    }
    if response.has_focus() {
        ui.painter()
            .rect_stroke(rect, 8, Stroke::new(1., ACCENT), egui::StrokeKind::Inside);
    }
    globe(ui.painter(), rect.min + vec2(8., 10.), color);
    ui.painter().text(
        rect.min + vec2(30., 18.),
        egui::Align2::LEFT_CENTER,
        language.code(),
        bold_font(12.),
        color,
    );
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id());
    let center = rect.min + vec2(70., 18.);
    let direction = if open { -1. } else { 1. };
    ui.painter().add(egui::Shape::line(
        vec![
            center + vec2(-3., -1.5 * direction),
            center + vec2(0., 1.5 * direction),
            center + vec2(3., -1.5 * direction),
        ],
        Stroke::new(1., color),
    ));
    let mut selected = None;
    egui::Popup::menu(&response)
        .id(popup_id())
        .align(egui::RectAlign::BOTTOM_END)
        .gap(8.)
        .width(240.)
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(22, 22, 24))
                .corner_radius(12)
                .inner_margin(4)
                .stroke(Stroke::new(1., Color32::from_rgb(60, 63, 68)))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 12,
                    spread: 0,
                    color: Color32::from_black_alpha(65),
                }),
        )
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            for option in [UiLanguage::English, UiLanguage::Korean] {
                let (row, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 36.), Sense::hover());
                let row_response = ui.interact(row, option_id(option), Sense::click());
                row_response.widget_info(|| {
                    let mut info = WidgetInfo::labeled(
                        WidgetType::SelectableLabel,
                        true,
                        option.native_name(),
                    );
                    info.selected = Some(option == language);
                    info
                });
                let current = option == language;
                let hovered = row_response.hovered() || row_response.has_focus();
                if current || hovered {
                    ui.painter().rect_filled(row, 8, PANEL);
                }
                let name_color = if current {
                    ACCENT
                } else if hovered {
                    TEXT
                } else {
                    MUTED
                };
                ui.painter().text(
                    row.left_center() + vec2(12., 0.),
                    egui::Align2::LEFT_CENTER,
                    option.native_name(),
                    egui::FontId::proportional(14.),
                    name_color,
                );
                ui.painter().text(
                    row.right_center() - vec2(12., 0.),
                    egui::Align2::RIGHT_CENTER,
                    option.code(),
                    egui::FontId::proportional(12.),
                    Color32::from_rgb(106, 106, 113),
                );
                if row_response.clicked() {
                    selected = Some(option);
                    ui.close();
                }
            }
        });
    selected
}

fn globe(painter: &egui::Painter, origin: Pos2, color: Color32) {
    let point = |x, y| origin + vec2(x, y) * (16. / 24.);
    let stroke = Stroke::new(2. * (16. / 24.), color);
    painter.circle_stroke(point(12., 12.), 10. * (16. / 24.), stroke);
    let mut path = vec![point(21.54, 15.), point(17., 15.)];
    curve(
        &mut path,
        [
            point(17., 15.),
            point(15.89543, 15.),
            point(15., 15.89543),
            point(15., 17.),
        ],
    );
    path.push(point(15., 21.54));
    painter.add(egui::Shape::line(path, stroke));
    let mut path = vec![point(7., 3.34), point(7., 5.)];
    for coords in [
        [(7., 5.), (7., 6.65685), (8.34315, 8.), (10., 8.)],
        [(10., 8.), (11.10457, 8.), (12., 8.89543), (12., 10.)],
        [(12., 10.), (12., 11.1), (12.9, 12.), (14., 12.)],
        [(14., 12.), (15.10457, 12.), (16., 11.10457), (16., 10.)],
        [(16., 10.), (16., 8.9), (16.9, 8.), (18., 8.)],
    ] {
        curve(&mut path, coords.map(|(x, y)| point(x, y)));
    }
    path.push(point(21.17, 8.));
    painter.add(egui::Shape::line(path, stroke));
    let mut path = vec![point(11., 21.95), point(11., 18.)];
    curve(
        &mut path,
        [
            point(11., 18.),
            point(11., 16.89543),
            point(10.10457, 16.),
            point(9., 16.),
        ],
    );
    curve(
        &mut path,
        [
            point(9., 16.),
            point(7.89543, 16.),
            point(7., 15.10457),
            point(7., 14.),
        ],
    );
    path.push(point(7., 13.));
    curve(
        &mut path,
        [
            point(7., 13.),
            point(7., 11.89543),
            point(6.10457, 11.),
            point(5., 11.),
        ],
    );
    path.push(point(2.05, 11.));
    painter.add(egui::Shape::line(path, stroke));
}
fn curve(path: &mut Vec<Pos2>, p: [Pos2; 4]) {
    for step in 1..=8 {
        let t = step as f32 / 8.;
        let u = 1. - t;
        let v = p[0].to_vec2() * u.powi(3)
            + p[1].to_vec2() * (3. * u * u * t)
            + p[2].to_vec2() * (3. * u * t * t)
            + p[3].to_vec2() * t.powi(3);
        path.push(v.to_pos2());
    }
}
