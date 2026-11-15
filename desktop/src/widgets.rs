//! The widgets of the interface, drawn the way the mockup wants them.

use egui::{Align2, Color32, FontId, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

use crate::theme::{palette, size};

/// A button with a line around it.
///
/// Under the pointer it turns white, with black letters.
pub fn button(ui: &mut Ui, text: &str) -> bool {
    let font = FontId::proportional(size::BODY);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), palette::dark::TEXT);
    let width = galley.size().x + 28.0;
    paint(ui, text, font, width, false)
}

/// A button of a given width.
pub fn sized_button(ui: &mut Ui, text: &str, width: f32) -> bool {
    let font = FontId::proportional(size::BODY);
    paint(ui, text, font, width, false)
}

/// A button that turns red when the pointer is on it.
///
/// Delete and Default are the two that destroy something, so they are painted
/// by hand: the theme cannot tell one button from the other.
pub fn danger_button(ui: &mut Ui, text: &str) -> bool {
    let font = FontId::proportional(size::BODY);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), palette::dark::TEXT);
    let width = galley.size().x + 28.0;
    paint(ui, text, font, width, true)
}

/// A button that turns red when the pointer is on it, of a given width.
pub fn sized_danger_button(ui: &mut Ui, text: &str, width: f32) -> bool {
    let font = FontId::proportional(size::BODY);
    paint(ui, text, font, width, true)
}

/// Paint a button and tell whether it was pressed.
fn paint(ui: &mut Ui, text: &str, font: FontId, width: f32, danger: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());

    if ui.is_rect_visible(rect) {
        let (fill, ink, line) = if response.hovered() {
            if danger {
                (palette::dark::DANGER, Color32::BLACK, palette::dark::DANGER)
            } else {
                (palette::dark::TEXT, Color32::BLACK, palette::dark::TEXT)
            }
        } else if danger {
            (
                palette::dark::SURFACE,
                palette::dark::DANGER,
                palette::dark::DANGER_BORDER,
            )
        } else {
            (
                palette::dark::SURFACE,
                palette::dark::TEXT,
                palette::dark::BORDER,
            )
        };

        ui.painter()
            .rect(rect, 0.0, fill, Stroke::new(1.0, line), StrokeKind::Middle);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text.to_string(),
            font,
            ink,
        );
    }

    response.clicked()
}

/// A box with a cross in it: how the settings show that something is on.
pub fn check_box(ui: &mut Ui, checked: &mut bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());

    if ui.is_rect_visible(rect) {
        let border = if response.hovered() {
            palette::dark::BORDER_STRONG
        } else {
            palette::dark::BORDER
        };
        ui.painter().rect(
            rect,
            0.0,
            palette::dark::SURFACE,
            Stroke::new(1.0, border),
            StrokeKind::Middle,
        );
        if *checked {
            let stroke = Stroke::new(1.6, palette::dark::TEXT);
            let inner = rect.shrink(4.0);
            ui.painter()
                .line_segment([inner.left_top(), inner.right_bottom()], stroke);
            ui.painter()
                .line_segment([inner.right_top(), inner.left_bottom()], stroke);
        }
    }

    if response.clicked() {
        *checked = !*checked;
    }
    response
}
