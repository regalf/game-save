//! What the interface actually paints.

use egui::epaint::Shape;
use egui::{Color32, Context};

/// The colour a piece of text was painted with.
fn painted_colour(ctx: &Context) -> Vec<(String, Color32)> {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.strong("Intestazione");
        ui.label("Semplice");
    });
    output.textures_delta.clear();

    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(text) => Some((text.galley.text().to_string(), text.fallback_color)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_strong_text_is_not_the_accent_colour() {
    let ctx = Context::default();
    gamesave::theme::install_fonts(&ctx);
    gamesave::theme::apply(&ctx, true);

    let style = ctx.style_of(egui::Theme::Dark);
    let accent = style.visuals.hyperlink_color;
    let active = style.visuals.widgets.active.text_color();

    println!("accent: {accent:?}");
    println!("active: {active:?}   <- il colore del testo strong");
    println!("text:   {:?}", style.visuals.text_color());

    // In egui the strong text takes the colour of the active widget, and the
    // active widget is the one under the finger. That colour must never be
    // the accent: with the accent the whole table ends up blue.
    assert_ne!(
        active, accent,
        "the strong text must not use the accent colour"
    );

    // The plain text uses the colour of the theme.
    let painted = painted_colour(&ctx);
    for (text, colour) in &painted {
        println!("  {text:<14} {colour:?}");
    }
    let plain = painted
        .iter()
        .find(|(text, _)| text == "Semplice")
        .expect("the plain label");
    assert_eq!(plain.1, style.visuals.text_color());
}
