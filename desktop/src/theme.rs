//! The look of the interface: the fonts, the colours and the spacing.
//!
//! Everything visual lives here, in one place, so that a mockup can be
//! mapped onto the application without hunting through the widgets.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily, Theme, Visuals};

/// The colours of the interface.
pub mod palette {
    use egui::Color32;

    /// The colours of the dark theme.
    pub mod dark {
        use super::Color32;

        /// The colour of the window.
        pub const BACKGROUND: Color32 = Color32::from_rgb(0x0D, 0x0D, 0x14);
        /// The colour of the panels that sit on the window. The cards of the
        /// library have no fill of their own: only a line around them.
        pub const SURFACE: Color32 = Color32::from_rgb(0x0D, 0x0D, 0x14);
        /// The colour of the lines between the elements.
        pub const BORDER: Color32 = Color32::from_rgb(0x3C, 0x3C, 0x57);
        /// The colour of a line that stands out, or of a panel.
        pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x72, 0x72, 0xA2);
        /// The colour of the text.
        pub const TEXT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
        /// The colour of the text that matters less.
        pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8E, 0x8E, 0x91);
        /// The colour of the text that is only a hint.
        pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x3C, 0x3C, 0x57);
        /// The colour of the accent.
        pub const ACCENT: Color32 = Color32::from_rgb(0x4F, 0x80, 0xFF);
        /// The colour of a problem.
        pub const DANGER: Color32 = Color32::from_rgb(0xFB, 0x4D, 0x4D);
        /// The colour of the line around a button that destroys something.
        pub const DANGER_BORDER: Color32 = Color32::from_rgb(0x56, 0x15, 0x15);
    }

    /// The colours of the light theme.
    pub mod light {
        use super::Color32;

        /// The colour of the window.
        pub const BACKGROUND: Color32 = Color32::from_rgb(0xF7, 0xF7, 0xFA);
        /// The colour of the panels that sit on the window.
        pub const SURFACE: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
        /// The colour of the lines between the elements.
        pub const BORDER: Color32 = Color32::from_rgb(0xD2, 0xD3, 0xDE);
        /// The colour of a line that stands out, or of a panel.
        pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x9A, 0x9C, 0xB8);
        /// The colour of the text.
        pub const TEXT: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x24);
        /// The colour of the text that matters less.
        pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x6A, 0x6C, 0x7A);
        /// The colour of the text that is only a hint.
        pub const TEXT_FAINT: Color32 = Color32::from_rgb(0xA6, 0xA8, 0xB4);
        /// The colour of the accent.
        pub const ACCENT: Color32 = Color32::from_rgb(0x0F, 0x62, 0xD6);
        /// The colour of a problem.
        pub const DANGER: Color32 = Color32::from_rgb(0xBE, 0x30, 0x2A);
        /// The colour of the line around a button that destroys something.
        pub const DANGER_BORDER: Color32 = Color32::from_rgb(0xE8, 0xB0, 0xB0);
    }
}

/// The sizes of the text.
pub mod size {
    /// The name of the application, in the bar on top.
    pub const TITLE: f32 = 36.0;
    /// The title of a panel.
    pub const PANEL_TITLE: f32 = 23.0;
    /// The label of a field.
    pub const LABEL: f32 = 17.0;
    /// The ordinary text.
    pub const BODY: f32 = 15.0;
    /// The text that matters less.
    pub const SMALL: f32 = 12.0;
    /// The smallest text.
    pub const TINY: f32 = 11.0;
}

/// A colour with its alpha changed.
///
/// The components of a colour are premultiplied, so they have to be
/// unmultiplied, the alpha changed, and multiplied again.
pub fn with_alpha(color: egui::Color32, alpha: f32) -> egui::Color32 {
    let a = color.a();
    if a == 0 {
        return color;
    }
    let new_alpha = ((a as f32) * alpha).round().clamp(0.0, 255.0) as u8;
    // The components are premultiplied: they follow the alpha.
    let scale = new_alpha as f32 / a as f32;
    egui::Color32::from_rgba_premultiplied(
        (color.r() as f32 * scale) as u8,
        (color.g() as f32 * scale) as u8,
        (color.b() as f32 * scale) as u8,
        new_alpha,
    )
}

/// The interface font: Fira Code, under the Open Font License.
const FIRA_MONO: &[u8] = include_bytes!("../assets/fonts/FiraCodeNerdFontMono-Medium.ttf");

/// The same font, heavier: the text that has to stand out.
const FIRA_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/FiraCodeNerdFontMono-SemiBold.ttf");

/// The name given to the regular interface font.
const UI_REGULAR: &str = "fira-mono";
/// The name given to the bold interface font.
const UI_BOLD: &str = "fira-mono-bold";

/// Put the fonts in place.
///
/// The font travels with the program, so the interface looks the same on every
/// machine, whatever the system has installed. The fonts of egui are kept as a
/// fallback, so its own icons keep working.
pub fn install_fonts(ctx: &Context) {
    ctx.set_fonts(font_definitions());
}

/// Build the font set of the interface.
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        UI_REGULAR.to_owned(),
        Arc::new(FontData::from_static(FIRA_MONO)),
    );
    fonts.font_data.insert(
        UI_BOLD.to_owned(),
        Arc::new(FontData::from_static(FIRA_MONO_BOLD)),
    );

    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, UI_REGULAR.to_owned());
    proportional.insert(1, UI_BOLD.to_owned());

    // The monospaced family is the same font: the interface is written in one.
    let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
    monospace.insert(0, UI_REGULAR.to_owned());

    fonts
}

/// Apply the colours and the spacing of the given theme.
pub fn apply(ctx: &Context, dark: bool) {
    let theme = if dark { Theme::Dark } else { Theme::Light };

    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    if dark {
        visuals.window_fill = palette::dark::BACKGROUND;
        visuals.panel_fill = palette::dark::BACKGROUND;
        visuals.extreme_bg_color = palette::dark::BACKGROUND;
        visuals.faint_bg_color = palette::dark::BACKGROUND;
        visuals.hyperlink_color = palette::dark::ACCENT;
        visuals.selection.bg_fill = palette::dark::ACCENT.gamma_multiply(0.35);
        visuals.widgets.noninteractive.bg_fill = palette::dark::SURFACE;
        visuals.widgets.noninteractive.weak_bg_fill = palette::dark::SURFACE;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, palette::dark::TEXT);
        // A button at rest: the same colour as the window, with a line around.
        visuals.widgets.inactive.bg_fill = palette::dark::SURFACE;
        visuals.widgets.inactive.weak_bg_fill = palette::dark::SURFACE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, palette::dark::TEXT);
        // Under the pointer the button turns white, with black letters.
        visuals.widgets.hovered.bg_fill = palette::dark::TEXT;
        visuals.widgets.hovered.weak_bg_fill = palette::dark::TEXT;
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
        visuals.widgets.active.bg_fill = palette::dark::TEXT;
        visuals.widgets.active.weak_bg_fill = palette::dark::TEXT;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
        visuals.widgets.open.bg_fill = palette::dark::SURFACE;
        visuals.widgets.open.weak_bg_fill = palette::dark::SURFACE;
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, palette::dark::TEXT);
    } else {
        visuals.window_fill = palette::light::BACKGROUND;
        visuals.panel_fill = palette::light::BACKGROUND;
        visuals.extreme_bg_color = palette::light::BACKGROUND;
        visuals.faint_bg_color = palette::light::BACKGROUND;
        visuals.hyperlink_color = palette::light::ACCENT;
        visuals.selection.bg_fill = palette::light::ACCENT.gamma_multiply(0.25);
        visuals.widgets.noninteractive.bg_fill = palette::light::SURFACE;
        visuals.widgets.noninteractive.weak_bg_fill = palette::light::SURFACE;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, palette::light::TEXT);
        visuals.widgets.inactive.bg_fill = palette::light::SURFACE;
        visuals.widgets.inactive.weak_bg_fill = palette::light::SURFACE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, palette::light::TEXT);
        visuals.widgets.hovered.bg_fill = palette::light::TEXT;
        visuals.widgets.hovered.weak_bg_fill = palette::light::TEXT;
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
        visuals.widgets.active.bg_fill = palette::light::TEXT;
        visuals.widgets.active.weak_bg_fill = palette::light::TEXT;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
        visuals.widgets.open.bg_fill = palette::light::SURFACE;
        visuals.widgets.open.weak_bg_fill = palette::light::SURFACE;
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, palette::light::TEXT);
    }

    visuals.window_corner_radius = 8u8.into();
    visuals.menu_corner_radius = 8u8.into();
    visuals.widgets.noninteractive.corner_radius = 4u8.into();
    visuals.widgets.inactive.corner_radius = 4u8.into();
    visuals.widgets.hovered.corner_radius = 4u8.into();
    visuals.widgets.active.corner_radius = 4u8.into();
    visuals.widgets.open.corner_radius = 4u8.into();

    ctx.set_visuals_of(theme, visuals);

    ctx.style_mut_of(theme, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(12);
        style.spacing.menu_margin = egui::Margin::same(8);
        style.spacing.interact_size.y = 28.0;

        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(size::TITLE, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(size::BODY, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(size::BODY, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(size::SMALL, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(size::BODY, egui::FontFamily::Monospace),
        );
    });
}
