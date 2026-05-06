use std::hash::Hash;

use bevy_egui::egui::{
    self, Align2, Button, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Id, Margin,
    Order, RichText, Sense, Stroke, TextEdit, TextStyle, Vec2, vec2,
};

pub(super) const MENU_WIDTH: f32 = 360.0;

#[derive(Debug, Clone, Copy)]
pub(super) enum ButtonKind {
    Primary,
    Secondary,
    Danger,
}

pub(super) fn apply_game_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(28.0, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        );

        style.spacing.item_spacing = vec2(10.0, 8.0);
        style.spacing.button_padding = vec2(16.0, 9.0);
        style.spacing.window_margin = Margin::same(18);
        style.visuals.override_text_color = Some(text());
        style.visuals.window_fill = panel_fill();
        style.visuals.panel_fill = Color32::TRANSPARENT;
        style.visuals.extreme_bg_color = input_fill();
        style.visuals.text_edit_bg_color = Some(input_fill());
        style.visuals.window_corner_radius = CornerRadius::same(7);
        style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text());
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, panel_stroke());
        style.visuals.widgets.inactive.bg_fill = button_fill();
        style.visuals.widgets.inactive.weak_bg_fill = button_fill();
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, button_stroke());
        style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, text());
        style.visuals.widgets.hovered.bg_fill = button_hover_fill();
        style.visuals.widgets.hovered.weak_bg_fill = button_hover_fill();
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, accent());
        style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        style.visuals.widgets.active.bg_fill = accent_dark();
        style.visuals.widgets.active.weak_bg_fill = accent_dark();
        style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, accent());
        style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    });
}

pub(super) fn screen_scrim(ctx: &egui::Context, id: &'static str, alpha: u8) {
    let rect = ctx.content_rect();
    egui::Area::new(Id::new(id))
        .order(Order::Background)
        .interactable(false)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            let local_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, rect.size());
            ui.allocate_rect(local_rect, egui::Sense::hover());
            ui.painter().rect_filled(
                local_rect,
                0,
                Color32::from_rgba_unmultiplied(2, 4, 7, alpha),
            );
        });
}

pub(super) fn panel_frame() -> Frame {
    Frame::NONE
        .fill(panel_fill())
        .stroke(Stroke::new(1.0, panel_stroke()))
        .corner_radius(7)
        .inner_margin(Margin::symmetric(24, 22))
}

pub(super) fn inset_frame() -> Frame {
    Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(7, 10, 14, 206))
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(90, 108, 128, 92),
        ))
        .corner_radius(5)
        .inner_margin(Margin::symmetric(14, 12))
}

pub(super) fn anchored_panel(
    ctx: &egui::Context,
    id: &'static str,
    desired_width: f32,
    anchor: Align2,
    offset: [f32; 2],
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let screen_width = ctx.content_rect().width();
    let width = desired_width.min((screen_width - 56.0).max(300.0));
    egui::Area::new(Id::new(id))
        .order(Order::Foreground)
        .anchor(anchor, offset)
        .show(ctx, |ui| {
            ui.set_width(width);
            panel_frame().show(ui, |ui| {
                ui.set_width(width - 48.0);
                add_contents(ui);
            });
        });
}

pub(super) fn game_button(
    ui: &mut egui::Ui,
    label: &str,
    kind: ButtonKind,
    width: f32,
) -> egui::Response {
    let (fill, stroke, text_color) = match kind {
        ButtonKind::Primary => (accent_dark(), Stroke::new(1.0, accent()), Color32::WHITE),
        ButtonKind::Secondary => (button_fill(), Stroke::new(1.0, button_stroke()), text()),
        ButtonKind::Danger => (
            Color32::from_rgba_unmultiplied(92, 35, 38, 224),
            Stroke::new(1.0, Color32::from_rgb(165, 72, 76)),
            Color32::from_rgb(255, 224, 224),
        ),
    };

    ui.add(
        Button::new(RichText::new(label).size(14.0).strong().color(text_color))
            .min_size(Vec2::new(width, 46.0))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(4),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

pub(super) fn disabled_game_button(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::hover());

    ui.painter().rect(
        rect,
        4,
        Color32::from_rgba_unmultiplied(28, 32, 38, 210),
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(92, 102, 116, 72)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(14.0, FontFamily::Proportional),
        muted_text(),
    );

    response
}

pub(super) fn compact_button(
    ui: &mut egui::Ui,
    label: &str,
    kind: ButtonKind,
    width: f32,
) -> egui::Response {
    let (fill, stroke, text_color) = match kind {
        ButtonKind::Primary => (accent_dark(), Stroke::new(1.0, accent()), Color32::WHITE),
        ButtonKind::Secondary => (button_fill(), Stroke::new(1.0, button_stroke()), text()),
        ButtonKind::Danger => (
            Color32::from_rgba_unmultiplied(75, 31, 34, 218),
            Stroke::new(1.0, Color32::from_rgb(145, 60, 64)),
            Color32::from_rgb(255, 224, 224),
        ),
    };

    ui.add(
        Button::new(RichText::new(label).size(13.0).color(text_color))
            .min_size(Vec2::new(width, 34.0))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(4),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

pub(super) fn compact_button_in_rect(
    ui: &mut egui::Ui,
    id_source: impl Hash,
    rect: egui::Rect,
    label: &str,
    kind: ButtonKind,
) -> egui::Response {
    let response = ui
        .interact(rect, ui.id().with(id_source), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let (fill, stroke, text_color) = button_paint(
        kind,
        response.hovered(),
        response.is_pointer_button_down_on(),
    );

    ui.painter()
        .rect(rect, 4, fill, stroke, egui::StrokeKind::Inside);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::new(13.0, FontFamily::Proportional),
        text_color,
    );

    response
}

pub(super) fn text_input(value: &mut String) -> TextEdit<'_> {
    TextEdit::singleline(value)
        .vertical_align(egui::Align::Center)
        .margin(Margin::symmetric(10, 5))
}

pub(super) fn wow_tooltip(response: egui::Response, title: &str, body: &str) -> egui::Response {
    if let Some(pointer_position) = response.hover_pos().or_else(|| {
        response
            .contains_pointer()
            .then(|| response.ctx.pointer_hover_pos())
            .flatten()
    }) {
        let tooltip_position = pointer_position + vec2(16.0, 18.0);
        egui::Area::new(response.id.with("wow_tooltip"))
            .order(Order::Tooltip)
            .interactable(false)
            .fixed_pos(tooltip_position)
            .show(&response.ctx, |ui| {
                draw_wow_tooltip(ui, title, body);
            });
    }

    response
}

fn button_paint(kind: ButtonKind, hovered: bool, active: bool) -> (Color32, Stroke, Color32) {
    match kind {
        ButtonKind::Primary => {
            let fill = if active {
                Color32::from_rgb(24, 67, 118)
            } else if hovered {
                Color32::from_rgb(37, 101, 174)
            } else {
                accent_dark()
            };
            (fill, Stroke::new(1.0, accent()), Color32::WHITE)
        }
        ButtonKind::Secondary => {
            let fill = if active {
                Color32::from_rgba_unmultiplied(30, 36, 45, 246)
            } else if hovered {
                button_hover_fill()
            } else {
                button_fill()
            };
            (fill, Stroke::new(1.0, button_stroke()), text())
        }
        ButtonKind::Danger => {
            let fill = if active {
                Color32::from_rgba_unmultiplied(62, 22, 25, 236)
            } else if hovered {
                Color32::from_rgba_unmultiplied(94, 36, 40, 238)
            } else {
                Color32::from_rgba_unmultiplied(75, 31, 34, 218)
            };
            (
                fill,
                Stroke::new(1.0, Color32::from_rgb(145, 60, 64)),
                Color32::from_rgb(255, 224, 224),
            )
        }
    }
}

fn draw_wow_tooltip(ui: &mut egui::Ui, title: &str, body: &str) {
    Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(4, 6, 12, 244))
        .stroke(Stroke::new(1.0, Color32::from_rgb(78, 112, 174)))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_max_width(260.0);
            ui.label(
                RichText::new(title)
                    .size(14.0)
                    .strong()
                    .color(Color32::from_rgb(255, 214, 105)),
            );
            ui.add_space(4.0);
            ui.label(RichText::new(body).size(13.0).color(text()));
        });
}

pub(super) fn title(text_value: &str, size: f32) -> RichText {
    RichText::new(text_value)
        .size(size)
        .strong()
        .color(Color32::WHITE)
}

pub(super) fn section(text_value: &str) -> RichText {
    RichText::new(text_value)
        .size(21.0)
        .strong()
        .color(Color32::WHITE)
}

pub(super) fn muted(text_value: impl Into<String>) -> RichText {
    RichText::new(text_value.into()).color(muted_text())
}

pub(super) fn field_label(text_value: &str) -> RichText {
    RichText::new(text_value)
        .size(12.0)
        .strong()
        .color(Color32::from_rgb(172, 190, 208))
}

pub(super) fn status_text(text_value: &str) -> RichText {
    RichText::new(text_value)
        .size(13.0)
        .color(Color32::from_rgb(172, 207, 255))
}

pub(super) fn text() -> Color32 {
    Color32::from_rgb(224, 231, 238)
}

pub(super) fn muted_text() -> Color32 {
    Color32::from_rgb(146, 158, 171)
}

pub(super) fn accent() -> Color32 {
    Color32::from_rgb(92, 162, 255)
}

pub(super) fn accent_dark() -> Color32 {
    Color32::from_rgb(31, 82, 141)
}

pub(super) fn panel_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(12, 17, 23, 232)
}

pub(super) fn panel_stroke() -> Color32 {
    Color32::from_rgba_unmultiplied(112, 132, 154, 106)
}

pub(super) fn input_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(6, 9, 13, 232)
}

pub(super) fn button_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(38, 45, 54, 232)
}

pub(super) fn button_hover_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(53, 63, 75, 242)
}

pub(super) fn button_stroke() -> Color32 {
    Color32::from_rgba_unmultiplied(115, 132, 151, 112)
}
