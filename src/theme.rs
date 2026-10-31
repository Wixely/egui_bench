use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};
use std::sync::Arc;

pub const CHASSIS: Color32 = Color32::from_rgb(0x17, 0x19, 0x1D);
pub const PANEL: Color32 = Color32::from_rgb(0x21, 0x24, 0x2A);
pub const WELL: Color32 = Color32::from_rgb(0x14, 0x16, 0x19);
pub const ETCH: Color32 = Color32::from_rgb(0x33, 0x38, 0x41);
pub const BAND: Color32 = Color32::from_rgb(0x2A, 0x2E, 0x36);
pub const LEGEND: Color32 = Color32::from_rgb(0x8B, 0x92, 0x9C);
pub const VALUE: Color32 = Color32::from_rgb(0xD5, 0xDB, 0xE3);
pub const READOUT: Color32 = Color32::from_rgb(0xF5, 0xA6, 0x3B);
pub const READOUT_DIM: Color32 = Color32::from_rgb(0x67, 0x46, 0x1A);
pub const TRACE: Color32 = Color32::from_rgb(0x5C, 0xD0, 0xE8);
pub const FAULT: Color32 = Color32::from_rgb(0xE2, 0x6D, 0x5A);
pub const OK: Color32 = Color32::from_rgb(0x5C, 0xB0, 0x7A);
pub const SAFE: Color32 = Color32::from_rgb(0x6F, 0xD1, 0x8A);
pub const WARN: Color32 = Color32::from_rgb(0xE8, 0xB0, 0x3E);

pub const LEGEND_FONT: &str = "legend";
pub const FIGURE_FONT: &str = "figure";

pub const LEGEND_SIZE: f32 = 11.5;
pub const VALUE_SIZE: f32 = 13.0;

pub const RADIUS: u8 = 2;
pub const RAIL_W: f32 = 3.0;

pub fn legend_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(LEGEND_FONT.into()))
}

pub fn figure(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FIGURE_FONT.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn fonts(ctx: &egui::Context) {
    let mut f = egui::FontDefinitions::default();
    for (name, bytes) in [
        ("plex-mono", &include_bytes!("../assets/IBMPlexMono-Regular.ttf")[..]),
        ("plex-mono-semibold", &include_bytes!("../assets/IBMPlexMono-SemiBold.ttf")[..]),
        ("plex-condensed", &include_bytes!("../assets/IBMPlexSansCondensed-Regular.ttf")[..]),
        (
            "plex-condensed-semibold",
            &include_bytes!("../assets/IBMPlexSansCondensed-SemiBold.ttf")[..],
        ),
    ] {
        f.font_data.insert(name.to_owned(), Arc::new(egui::FontData::from_static(bytes)));
    }
    f.families.entry(FontFamily::Proportional).or_default().insert(0, "plex-condensed".into());
    f.families.entry(FontFamily::Monospace).or_default().insert(0, "plex-mono".into());

    let fallback: Vec<String> =
        f.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, face) in
        [(LEGEND_FONT, "plex-condensed-semibold"), (FIGURE_FONT, "plex-mono-semibold")]
    {
        let mut stack = vec![face.to_owned()];
        stack.extend(fallback.iter().cloned());
        f.families.insert(FontFamily::Name(family.into()), stack);
    }
    ctx.set_fonts(f);
}

pub fn install(ctx: &egui::Context) {
    fonts(ctx);

    ctx.set_theme(egui::ThemePreference::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = Some(VALUE);
    v.panel_fill = CHASSIS;
    v.window_fill = CHASSIS;
    v.extreme_bg_color = WELL;
    v.faint_bg_color = Color32::from_rgb(0x1B, 0x1E, 0x23);
    v.window_stroke = Stroke::new(1.0, ETCH);

    let r = CornerRadius::same(RADIUS);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, ETCH);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, LEGEND);

    v.widgets.inactive.bg_fill = Color32::from_rgb(0x2A, 0x2E, 0x35);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, ETCH);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, VALUE);

    v.widgets.hovered.bg_fill = Color32::from_rgb(0x35, 0x3A, 0x43);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, LEGEND);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);

    v.widgets.active.bg_fill = Color32::from_rgb(0x3E, 0x34, 0x22);
    v.widgets.active.bg_stroke = Stroke::new(1.0, READOUT);
    v.widgets.active.fg_stroke = Stroke::new(1.0, READOUT);

    v.selection.bg_fill = Color32::from_rgb(0x2E, 0x3C, 0x46);
    v.selection.stroke = Stroke::new(1.0, TRACE);

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.slider_width = 120.0;

    style.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
    style.text_styles.insert(TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace));
    style.text_styles.insert(TextStyle::Heading, legend_font(16.0));

    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunFrame;

    fn luma(c: Color32) -> f32 {
        0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let l = |c: Color32| {
            let f = |v: u8| {
                let s = v as f32 / 255.0;
                if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let (x, y) = (l(a), l(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn body_text_is_readable_on_the_panel() {
        assert!(contrast(VALUE, PANEL) > 7.0, "{:.1}", contrast(VALUE, PANEL));
    }

    #[test]
    fn legends_stay_legible_without_shouting() {
        let c = contrast(LEGEND, PANEL);
        assert!(c > 4.5, "legend contrast only {c:.1}");
        assert!(c < contrast(VALUE, PANEL), "legend competes with its own value");
    }

    #[test]
    fn both_accents_carry_on_the_chassis() {
        assert!(contrast(READOUT, CHASSIS) > 6.0, "amber {:.1}", contrast(READOUT, CHASSIS));
        assert!(contrast(TRACE, CHASSIS) > 6.0, "cyan {:.1}", contrast(TRACE, CHASSIS));
    }

    #[test]
    fn the_accents_are_distinguishable_from_each_other() {
        let db = (READOUT.b() as i32 - TRACE.b() as i32).abs();
        assert!(db > 100, "accents differ by only {db} in blue");
    }

    #[test]
    fn surfaces_step_in_a_consistent_direction() {
        assert!(luma(WELL) < luma(CHASSIS));
        assert!(luma(CHASSIS) < luma(PANEL));
        assert!(luma(PANEL) < luma(ETCH));
    }

    #[test]
    fn dim_readout_reads_as_the_same_hue() {
        let hue = |c: Color32| (c.r() as f32 - c.b() as f32) / (c.r() as f32 + c.b() as f32);
        assert!((hue(READOUT) - hue(READOUT_DIM)).abs() < 0.12);
        assert!(luma(READOUT_DIM) < luma(READOUT) * 0.6);
    }

    #[test]
    fn fault_is_distinct_from_the_amber_readout() {
        let d = (FAULT.g() as i32 - READOUT.g() as i32).abs();
        assert!(d > 40, "fault and readout differ by only {d} in green");
    }

    #[test]
    fn every_named_family_is_bound() {
        let ctx = egui::Context::default();
        install(&ctx);
        let _ = ctx.run_frame(Default::default(), |ui| {
            for f in [legend_font(12.0), figure(12.0), mono(12.0)] {
                let _ = ui.painter().layout_no_wrap("0".into(), f, VALUE);
            }
        });
    }
}
