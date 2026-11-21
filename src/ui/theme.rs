use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontFamily, FontId, Frame, Margin, Pos2, Rect,
    RichText, Sense, Stroke, Ui, Vec2, WidgetText,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(18, 18, 24);
pub const RAIL: Color32 = Color32::from_rgb(23, 22, 30);
pub const PANEL: Color32 = Color32::from_rgb(29, 28, 38);
pub const TILE: Color32 = Color32::from_rgb(37, 35, 48);
pub const EDGE: Color32 = Color32::from_rgb(52, 49, 66);
pub const TEXT: Color32 = Color32::from_rgb(247, 245, 252);
pub const MUTED: Color32 = Color32::from_rgb(172, 168, 193);
pub const ACCENT: Color32 = Color32::from_rgb(187, 170, 255);
pub const ACCENT_FILL: Color32 = Color32::from_rgb(54, 45, 83);
pub const GREEN: Color32 = Color32::from_rgb(135, 221, 178);
pub const AMBER: Color32 = Color32::from_rgb(249, 197, 140);
pub const RED: Color32 = Color32::from_rgb(255, 160, 168);
pub const INK: Color32 = Color32::from_rgb(29, 22, 48);

pub fn setup(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/ui-regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "Inter semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/ui-semibold.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".into());
    let bold = fonts.families[&FontFamily::Proportional]
        .iter()
        .filter(|name| name.as_str() != "Inter")
        .cloned();
    fonts.families.insert(
        weight(),
        std::iter::once("Inter semibold".into())
            .chain(bold)
            .collect(),
    );
    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = BG;
    style.visuals.override_text_color = None;
    style.visuals.widgets.noninteractive.fg_stroke.color = MUTED;
    style.visuals.widgets.inactive.bg_fill = TILE;
    style.visuals.widgets.inactive.weak_bg_fill = TILE;
    style.visuals.widgets.hovered.weak_bg_fill = ACCENT_FILL;
    style.visuals.widgets.active.weak_bg_fill = ACCENT_FILL;
    style.visuals.slider_trailing_fill = true;
    style.visuals.handle_shape = egui::style::HandleShape::Circle;
    style.visuals.widgets.inactive.fg_stroke.color = TEXT;
    style.visuals.widgets.hovered.bg_fill = ACCENT_FILL;
    style.visuals.widgets.hovered.fg_stroke.color = ACCENT;
    style.visuals.widgets.active.bg_fill = ACCENT_FILL;
    style.visuals.widgets.active.fg_stroke.color = ACCENT;
    style.visuals.selection = egui::style::Selection {
        bg_fill: ACCENT_FILL,
        stroke: Stroke::new(1.0_f32, ACCENT),
    };
    style.spacing.item_spacing = egui::vec2(10.0, 12.0);
    style.spacing.button_padding = egui::vec2(13.0, 8.0);
    style.spacing.interact_size = egui::vec2(42.0, 36.0);
    ctx.set_global_style(style);
}

fn weight() -> FontFamily {
    FontFamily::Name("inter-bold".into())
}

pub fn title(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size, weight()))
        .color(TEXT)
}

pub fn accent(text: impl Into<String>) -> RichText {
    title(text, 12.0).color(ACCENT)
}

pub fn secondary(text: impl Into<String>) -> RichText {
    RichText::new(text).size(13.5).color(MUTED)
}

pub fn card(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, EDGE))
        .corner_radius(16)
        .inner_margin(Margin::same(22))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            body(ui);
        });
}

pub fn button(ui: &mut Ui, text: impl Into<WidgetText>, primary: bool) -> egui::Response {
    let mut button = egui::Button::new(text).corner_radius(8);
    if primary {
        button = button.fill(ACCENT).stroke(Stroke::new(1.0_f32, ACCENT));
    }
    ui.add(button)
}

pub fn status(ui: &mut Ui, text: &str, color: Color32) {
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(9.0, 10.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.5, color);
        ui.label(title(text, 12.0).color(color));
    });
}

pub fn nav(ui: &mut Ui, name: &str, icon: usize, selected: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::click());
    if selected || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(8),
            if selected { ACCENT_FILL } else { TILE },
        );
    }
    let color = if selected { ACCENT } else { MUTED };
    let p = rect.left_center() + egui::vec2(18.0, 0.0);
    glyph(ui, p, icon, color);
    ui.painter().text(
        rect.left_center() + egui::vec2(39.0, 0.0),
        Align2::LEFT_CENTER,
        name,
        FontId::new(14.0, weight()),
        if selected { TEXT } else { color },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    response.clicked()
}

fn glyph(ui: &Ui, p: Pos2, index: usize, color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.5_f32, color);
    let at = |x, y| p + egui::vec2(x, y);
    match index {
        0 => {
            painter.rect_stroke(
                Rect::from_center_size(p, Vec2::new(13.0, 16.0)),
                3,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.circle_filled(at(0.0, -4.0), 1.1, color);
            painter.circle_stroke(at(0.0, 2.5), 3.0, stroke);
        }
        1 | 2 => {
            for (x, y) in [(-5.0, -1.5), (0.0, 3.0), (5.0, -3.0)] {
                painter.line_segment([at(x, -7.0), at(x, 7.0)], stroke);
                painter.circle_filled(at(x, y), 2.0, color);
            }
            if index == 2 {
                painter.line_segment([at(-8.0, 8.0), at(8.0, 8.0)], stroke);
            }
        }
        3 => {
            painter.circle_stroke(p, 6.0, stroke);
            painter.circle_stroke(p, 2.0, stroke);
            for (x, y) in [(-8.0, 0.0), (8.0, 0.0), (0.0, -8.0), (0.0, 8.0)] {
                painter.line_segment([at(x * 0.65, y * 0.65), at(x, y)], stroke);
            }
        }
        5 => {
            painter.circle_stroke(p, 7.0, stroke);
            painter.circle_filled(at(0.0, -3.2), 0.85, color);
            painter.line_segment([at(0.0, -0.4), at(0.0, 3.4)], stroke);
        }
        _ => {
            for (a, b) in [
                ((-8.0, 0.0), (-4.0, 0.0)),
                ((-4.0, 0.0), (-1.0, -6.0)),
                ((-1.0, -6.0), (2.0, 6.0)),
                ((2.0, 6.0), (5.0, 0.0)),
                ((5.0, 0.0), (8.0, 0.0)),
            ] {
                painter.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
            }
        }
    }
}
