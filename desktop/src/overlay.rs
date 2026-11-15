//! The panels that appear in the middle of the screen.
//!
//! They are not windows that can be dragged around: they stay where they
//! are, in the middle of the screen, and they come in with a quick fade.

use std::time::Instant;

use egui::{Color32, Context, Frame, Id, Modal, Sense, Stroke, Ui, Vec2};

use crate::theme;

/// How long the fade in lasts, in seconds.
const FADE_IN: f32 = 0.13;

/// How dark the background goes behind the panel.
const BACKDROP: f32 = 0.62;

/// A panel that appears in the middle of the screen.
pub struct Overlay {
    opened: Option<Instant>,
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

impl Overlay {
    /// Create an overlay that is closed.
    pub fn new() -> Self {
        Self { opened: None }
    }

    /// Whether the panel is open, or on its way in.
    pub fn is_open(&self) -> bool {
        self.opened.is_some()
    }

    /// Show the panel, or take it away.
    ///
    /// The `visible` flag is set to false when the panel is closed, either
    /// with the close button or with a click on the background.
    pub fn show(
        &mut self,
        ctx: &Context,
        id: &str,
        visible: &mut bool,
        contents: impl FnOnce(&mut Ui),
    ) {
        if !*visible {
            self.opened = None;
            return;
        }

        let opened = *self.opened.get_or_insert_with(Instant::now);
        let progress = (opened.elapsed().as_secs_f32() / FADE_IN).clamp(0.0, 1.0);
        // It starts fast and settles: a smooth step.
        let alpha = progress * progress * (3.0 - 2.0 * progress);

        let frame = Frame::new()
            .fill(theme::with_alpha(theme::palette::dark::SURFACE, alpha))
            .stroke(Stroke::new(
                1.0,
                theme::with_alpha(theme::palette::dark::BORDER, alpha),
            ))
            .corner_radius(8)
            .inner_margin(18.0);

        let response = Modal::new(Id::unique(id))
            .backdrop_color(Color32::from_rgba_premultiplied(
                0,
                0,
                0,
                (BACKDROP * alpha * 255.0) as u8,
            ))
            .frame(frame)
            .show(ctx, |ui| {
                let text = theme::with_alpha(theme::palette::dark::TEXT, alpha);
                let border = theme::with_alpha(theme::palette::dark::BORDER, alpha);
                ui.visuals_mut().override_text_color = Some(text);
                ui.visuals_mut().widgets.noninteractive.fg_stroke = Stroke::new(1.0, border);
                ui.visuals_mut().widgets.inactive.fg_stroke = Stroke::new(1.0, text);
                ui.visuals_mut().widgets.hovered.fg_stroke = Stroke::new(1.0, text);
                contents(ui);
            });

        // A click on the background closes the panel.
        if response.backdrop_response.clicked() {
            *visible = false;
        }
    }
}

/// The header of a panel: the title on the left, the close button on the right.
pub fn header(ui: &mut Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .color(theme::palette::dark::TEXT)
                .size(theme::size::PANEL_TITLE),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = close_button(ui);
        });
    });
    ui.separator();
    close
}

/// A small button with a cross drawn on it.
///
/// The cross is drawn and not written, so it looks the same whatever the font
/// of the system has in it.
pub fn close_button(ui: &mut Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let colour = if response.hovered() {
            theme::palette::dark::TEXT
        } else {
            theme::palette::dark::TEXT_MUTED
        };
        let stroke = Stroke::new(1.2, colour);
        let inner = rect.shrink(5.0);
        let painter = ui.painter();
        painter.line_segment([inner.left_top(), inner.right_bottom()], stroke);
        painter.line_segment([inner.right_top(), inner.left_bottom()], stroke);
    }
    response.clicked()
}
