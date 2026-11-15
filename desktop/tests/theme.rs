//! What the theme actually resolves to.

use egui::{Context, Theme};

#[test]
fn the_theme_is_applied_to_the_style() {
    let ctx = Context::default();

    let show = |label: &str| {
        let style = ctx.style_of(Theme::Dark);
        println!("{label}");
        println!("  text_color:   {:?}", style.visuals.text_color());
        println!("  override:     {:?}", style.visuals.override_text_color);
        println!("  hyperlink:    {:?}", style.visuals.hyperlink_color);
        println!(
            "  nonint. fg:   {:?}",
            style.visuals.widgets.noninteractive.fg_stroke
        );
        println!("  panel_fill:   {:?}", style.visuals.panel_fill);
        println!("  extreme_bg:   {:?}", style.visuals.extreme_bg_color);
        println!("  weak_alpha:   {:?}", style.visuals.weak_text_alpha);
        println!("  weak_color:   {:?}", style.visuals.weak_text_color);
    };

    println!("prima:");
    show("  (nessuna)");

    gamesave::theme::install_fonts(&ctx);
    gamesave::theme::apply(&ctx, true);

    println!("dopo:");
    show("  (tema scuro)");

    // Which fonts ended up in place.
    let definitions = gamesave::theme::font_definitions();
    println!(
        "font dei dati: {:?}",
        definitions.font_data.keys().collect::<Vec<_>>()
    );
    for (family, names) in &definitions.families {
        println!("  {family:?}: {names:?}");
    }
}
