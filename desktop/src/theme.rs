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
        pub const BACKGROUND: Color32 = Color32::from_rgb(24, 25, 27);
        /// The colour of the panels that sit on the window.
        pub const SURFACE: Color32 = Color32::from_rgb(32, 33, 36);
        /// The colour of the lines between the elements.
        pub const BORDER: Color32 = Color32::from_rgb(52, 54, 58);
        /// The colour of the text.
        pub const TEXT: Color32 = Color32::from_rgb(232, 234, 238);
        /// The colour of the text that has to stand out: this is the colour
        /// egui gives to the strong text.
        pub const TEXT_STRONG: Color32 = Color32::from_rgb(255, 255, 255);
        /// The colour of the text that matters less.
        pub const TEXT_MUTED: Color32 = Color32::from_rgb(148, 152, 160);
        /// The colour of the accent.
        pub const ACCENT: Color32 = Color32::from_rgb(86, 156, 214);
        /// The colour of the accent when it is hovered.
        pub const ACCENT_HOVER: Color32 = Color32::from_rgb(112, 176, 226);
        /// The colour of a good news.
        pub const SUCCESS: Color32 = Color32::from_rgb(126, 199, 132);
        /// The colour of something that wants the attention.
        pub const WARNING: Color32 = Color32::from_rgb(224, 178, 96);
        /// The colour of a problem.
        pub const DANGER: Color32 = Color32::from_rgb(226, 118, 110);
    }

    /// The colours of the light theme.
    pub mod light {
        use super::Color32;

        /// The colour of the window.
        pub const BACKGROUND: Color32 = Color32::from_rgb(245, 246, 248);
        /// The colour of the panels that sit on the window.
        pub const SURFACE: Color32 = Color32::from_rgb(255, 255, 255);
        /// The colour of the lines between the elements.
        pub const BORDER: Color32 = Color32::from_rgb(216, 219, 224);
        /// The colour of the text.
        pub const TEXT: Color32 = Color32::from_rgb(32, 34, 38);
        /// The colour of the text that has to stand out: this is the colour
        /// egui gives to the strong text.
        pub const TEXT_STRONG: Color32 = Color32::from_rgb(0, 0, 0);
        /// The colour of the text that matters less.
        pub const TEXT_MUTED: Color32 = Color32::from_rgb(108, 112, 120);
        /// The colour of the accent.
        pub const ACCENT: Color32 = Color32::from_rgb(0, 95, 184);
        /// The colour of the accent when it is hovered.
        pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0, 78, 152);
        /// The colour of a good news.
        pub const SUCCESS: Color32 = Color32::from_rgb(45, 135, 60);
        /// The colour of something that wants the attention.
        pub const WARNING: Color32 = Color32::from_rgb(176, 122, 12);
        /// The colour of a problem.
        pub const DANGER: Color32 = Color32::from_rgb(190, 48, 42);
    }
}

/// The fonts of the interface, in order of preference.
///
/// The first one that is installed wins.
const INTERFACE_FONTS: &[&str] = &[
    "Segoe UI Variable Text",
    "Segoe UI",
    "Inter",
    "Noto Sans",
    "Ubuntu",
    "Cantarell",
    "Fira Sans",
    "DejaVu Sans",
    "Liberation Sans",
];

/// The fonts of the code, in order of preference.
const MONOSPACE_FONTS: &[&str] = &[
    "Cascadia Mono",
    "JetBrains Mono",
    "Consolas",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Ubuntu Mono",
    "Hack",
];

/// The name given to the regular interface font.
const UI_REGULAR: &str = "gamesave-ui";
/// The name given to the bold interface font.
const UI_BOLD: &str = "gamesave-ui-bold";
/// The name given to the monospaced font.
const MONOSPACE: &str = "gamesave-mono";

/// Put the fonts in place.
///
/// The interface font of the system is used when it can be found, because the
/// default font of egui is a bitmap one and it shows. The built in fonts are
/// kept as a fallback, so the icons of egui keep working.
pub fn install_fonts(ctx: &Context) {
    ctx.set_fonts(font_definitions());
}

/// Build the font set of the interface.
///
/// The interface font of the system is used when it can be found, because the
/// default font of egui is a bitmap one and it shows. The built in fonts are
/// kept as a fallback, so the icons of egui keep working.
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    let mut database = fontdb::Database::new();
    database.load_system_fonts();

    if let Some(data) = face_data(&database, INTERFACE_FONTS, fontdb::Weight::NORMAL) {
        fonts
            .font_data
            .insert(UI_REGULAR.to_owned(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, UI_REGULAR.to_owned());
    }

    if let Some(data) = face_data(&database, INTERFACE_FONTS, fontdb::Weight::BOLD) {
        fonts
            .font_data
            .insert(UI_BOLD.to_owned(), Arc::new(FontData::from_owned(data)));
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            if !family.contains(&UI_BOLD.to_owned()) {
                family.insert(1, UI_BOLD.to_owned());
            }
        }
    }

    if let Some(data) = face_data(&database, MONOSPACE_FONTS, fontdb::Weight::NORMAL) {
        fonts
            .font_data
            .insert(MONOSPACE.to_owned(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, MONOSPACE.to_owned());
    }

    fonts
}

/// Find the first font of the list with the given weight and return its data.
fn face_data(
    database: &fontdb::Database,
    names: &[&str],
    weight: fontdb::Weight,
) -> Option<Vec<u8>> {
    for name in names {
        // A name that is not installed has to move on to the next one.
        let Some(id) = database.query(&fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            weight,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        }) else {
            continue;
        };
        if let Some(data) = database.with_face_data(id, |data, _index| data.to_vec()) {
            return Some(data);
        }
    }
    None
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
        visuals.override_text_color = Some(palette::dark::TEXT);
        visuals.window_fill = palette::dark::SURFACE;
        visuals.panel_fill = palette::dark::SURFACE;
        visuals.extreme_bg_color = palette::dark::BACKGROUND;
        visuals.faint_bg_color = palette::dark::SURFACE;
        visuals.hyperlink_color = palette::dark::ACCENT;
        visuals.selection.bg_fill = palette::dark::ACCENT.gamma_multiply(0.35);
        visuals.widgets.noninteractive.bg_fill = palette::dark::SURFACE;
        visuals.widgets.noninteractive.weak_bg_fill = palette::dark::SURFACE;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, palette::dark::BORDER);
        visuals.widgets.inactive.bg_fill = palette::dark::SURFACE;
        visuals.widgets.inactive.weak_bg_fill = palette::dark::SURFACE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, palette::dark::TEXT);
        visuals.widgets.hovered.bg_fill = palette::dark::SURFACE;
        visuals.widgets.hovered.weak_bg_fill = palette::dark::SURFACE;
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, palette::dark::TEXT);
        // The active colour is the colour of the strong text in egui, so it
        // has to be a brighter text and not the accent.
        visuals.widgets.active.bg_fill = palette::dark::SURFACE;
        visuals.widgets.active.weak_bg_fill = palette::dark::SURFACE;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, palette::dark::TEXT_STRONG);
        visuals.widgets.open.bg_fill = palette::dark::SURFACE;
        visuals.widgets.open.weak_bg_fill = palette::dark::SURFACE;
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, palette::dark::TEXT);
    } else {
        visuals.override_text_color = Some(palette::light::TEXT);
        visuals.window_fill = palette::light::SURFACE;
        visuals.panel_fill = palette::light::SURFACE;
        visuals.extreme_bg_color = palette::light::BACKGROUND;
        visuals.faint_bg_color = palette::light::SURFACE;
        visuals.hyperlink_color = palette::light::ACCENT;
        visuals.selection.bg_fill = palette::light::ACCENT.gamma_multiply(0.25);
        visuals.widgets.noninteractive.bg_fill = palette::light::SURFACE;
        visuals.widgets.noninteractive.weak_bg_fill = palette::light::SURFACE;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, palette::light::BORDER);
        visuals.widgets.inactive.bg_fill = palette::light::SURFACE;
        visuals.widgets.inactive.weak_bg_fill = palette::light::SURFACE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, palette::light::TEXT);
        visuals.widgets.hovered.bg_fill = palette::light::SURFACE;
        visuals.widgets.hovered.weak_bg_fill = palette::light::SURFACE;
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, palette::light::TEXT);
        // The active colour is the colour of the strong text in egui, so it
        // has to be a stronger text and not the accent.
        visuals.widgets.active.bg_fill = palette::light::SURFACE;
        visuals.widgets.active.weak_bg_fill = palette::light::SURFACE;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, palette::light::TEXT_STRONG);
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
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(12);
        style.spacing.menu_margin = egui::Margin::same(8);
        style.spacing.interact_size.y = 26.0;

        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(20.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(13.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(11.5, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(12.5, egui::FontFamily::Monospace),
        );
    });
}
