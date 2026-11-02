use crate::key::Key;
use crate::text::{Line, lay};
use crate::theme::{FAULT, OK, PANEL, VALUE, WARN};
use egui::{Align, Align2, Color32, Context, Id, Layout, Margin, Order, Stroke, Ui, vec2};

pub const MIN_W: f32 = 280.0;
pub const MAX_W: f32 = 480.0;
const EDGE: f32 = 20.0;
const DOT_W: f32 = 10.0;
const MARGIN_X: i8 = 14;
const MARGIN_Y: i8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Ok,
    Warn,
    Fault,
}

impl Tone {
    fn dot(self) -> Color32 {
        match self {
            Tone::Ok => OK,
            Tone::Warn => WARN,
            Tone::Fault => FAULT,
        }
    }

    fn ink(self) -> Color32 {
        match self {
            Tone::Ok => VALUE,
            Tone::Warn => WARN,
            Tone::Fault => FAULT,
        }
    }
}

pub fn toast(
    ctx: &Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    text: &str,
    tone: Tone,
) -> bool {
    let mut dismissed = false;
    let screen = (ctx.content_rect().width() - EDGE * 2.0).max(120.0);
    let max = MAX_W.min(screen);
    let min = MIN_W.min(max);
    egui::Area::new(Id::unique(("egui_bench_toast", id)))
        .anchor(Align2::LEFT_BOTTOM, vec2(EDGE, -EDGE))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(PANEL)
                .stroke(Stroke::new(1.0, tone.dot()))
                .inner_margin(Margin::symmetric(MARGIN_X, MARGIN_Y))
                .show(ui, |ui| {
                    let pad = f32::from(MARGIN_X) * 2.0 + 2.0;
                    dismissed = body(ui, text, tone, min - pad, max - pad);
                });
        });
    dismissed
}

fn body(ui: &mut Ui, text: &str, tone: Tone, min: f32, max: f32) -> bool {
    let key = Key::new("dismiss").outline(VALUE);
    let key_w = key.width(ui);
    let gap = ui.spacing().item_spacing.x;
    let chrome = DOT_W + key_w + gap * 2.0;
    let message = || Line::new().note(text).tint(tone.ink());
    let natural = lay(ui, message().job(), f32::INFINITY).size().x.ceil() + 1.0;
    let width = (natural + chrome).clamp(min, max);
    let text_w = (width - chrome).max(40.0);
    let mut dismissed = false;
    ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_width(width);
        let (rect, _) = ui.allocate_exact_size(vec2(DOT_W, 16.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.0, tone.dot());
        ui.allocate_ui(vec2(text_w, 0.0), |ui| {
            ui.set_width(text_w);
            message().show(ui);
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            dismissed = ui.add(key).clicked();
        });
    });
    dismissed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunFrame;
    use egui::{Pos2, Rect, Vec2};

    struct Shown {
        dismiss_rows: usize,
        message_rows: usize,
        right: f32,
        width: f32,
    }

    fn shown(screen_w: f32, text: &str) -> Shown {
        let ctx = Context::default();
        crate::install(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(screen_w, 600.0));
        let mut last = None;
        for _ in 0..3 {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            last = Some(ctx.run_frame(input, |ui| {
                toast(ui.ctx(), "t", text, Tone::Fault);
            }));
        }
        let mut s = Shown { dismiss_rows: 0, message_rows: 0, right: 0.0, width: 0.0 };
        let mut left = f32::MAX;
        for clipped in last.unwrap().shapes {
            if let egui::Shape::Text(t) = &clipped.shape {
                let rows = t.galley.rows.len();
                if t.galley.text().eq_ignore_ascii_case("dismiss") {
                    s.dismiss_rows = rows;
                } else {
                    s.message_rows = rows;
                }
            }
            let bounds = clipped.shape.visual_bounding_rect();
            s.right = s.right.max(bounds.right());
            left = left.min(bounds.left());
        }
        s.width = s.right - left;
        s
    }

    #[test]
    fn dismiss_stays_on_one_line_beside_a_long_message() {
        let long = "could not reach github.com to ask for the newest releases, check the network and try again later";
        let s = shown(900.0, long);
        assert_eq!(s.dismiss_rows, 1);
        assert!(s.message_rows > 1, "the message should wrap, not the key");
    }

    #[test]
    fn dismiss_stays_on_one_line_beside_a_short_message() {
        let s = shown(900.0, "installed");
        assert_eq!(s.dismiss_rows, 1);
        assert_eq!(s.message_rows, 1);
    }

    #[test]
    fn toast_fits_a_narrow_window() {
        let s = shown(240.0, "could not reach github.com to ask for the newest releases");
        assert_eq!(s.dismiss_rows, 1);
        assert!(s.right <= 240.0 + 0.5, "{} past the window", s.right - 240.0);
    }

    #[test]
    fn a_short_message_still_gets_the_minimum_width() {
        let s = shown(900.0, "ok");
        assert!(s.width >= MIN_W - 1.0, "{} wide", s.width);
    }

    #[test]
    fn a_long_message_stops_at_the_maximum_width() {
        let s = shown(1400.0, &"word ".repeat(80));
        assert!(s.width <= MAX_W + 1.0, "{} wide", s.width);
        assert!(s.message_rows > 2);
    }
}
