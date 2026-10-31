use crate::theme::{self, BAND, ETCH, LEGEND, RADIUS, READOUT, VALUE, WELL};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, Widget};

pub const KEY_H: f32 = 30.0;
const KEY_SIZE: f32 = 12.0;
const KEY_TRACKING: f32 = 1.4;
const KEY_PAD: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Look {
    Solid(Color32),
    Outline(Color32),
}

pub struct Key {
    label: String,
    look: Look,
    enabled: bool,
    min_width: f32,
    more: bool,
    id: Option<egui::Id>,
}

impl Key {
    pub fn new(label: impl Into<String>) -> Key {
        Key {
            label: label.into(),
            look: Look::Outline(VALUE),
            enabled: true,
            min_width: 0.0,
            more: false,
            id: None,
        }
    }

    pub fn more() -> Key {
        Key { more: true, min_width: KEY_H, ..Key::new("") }
    }

    pub fn solid(mut self, tint: Color32) -> Key {
        self.look = Look::Solid(tint);
        self
    }

    pub fn outline(mut self, tint: Color32) -> Key {
        self.look = Look::Outline(tint);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Key {
        self.enabled = enabled;
        self
    }

    pub fn min_width(mut self, width: f32) -> Key {
        self.min_width = width;
        self
    }

    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Key {
        self.id = Some(egui::Id::unique(salt));
        self
    }

    pub fn width(&self, ui: &Ui) -> f32 {
        let text = if self.label.is_empty() {
            0.0
        } else {
            ui.fonts_mut(|f| f.layout_job(job(&self.label, VALUE))).size().x + KEY_PAD
        };
        text.max(self.min_width)
    }

    /// The label laid out to fit a key `width` wide, on as many lines as it takes.
    fn lettered(&self, ui: &Ui, ink: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
        crate::text::lay(ui, job(&self.label, ink), width - KEY_PAD)
    }

    pub fn paint(self, ui: &mut Ui, rect: Rect) -> Response {
        let sense = if self.enabled { Sense::click() } else { Sense::hover() };
        let id = self.id.unwrap_or_else(|| {
            let id = ui.next_auto_id();
            ui.skip_ahead_auto_ids(1);
            id
        });
        let resp = ui.interact(rect, id, sense);
        let hot = self.enabled && (resp.hovered() || resp.has_focus());
        let down = self.enabled && resp.is_pointer_button_down_on();
        let (fill, stroke, ink) = match (self.look, self.enabled) {
            (_, false) => (WELL, ETCH, LEGEND.gamma_multiply(0.55)),
            (Look::Solid(t), true) => {
                let fill = if down {
                    t.gamma_multiply(0.82)
                } else if hot {
                    t.lerp_to_gamma(Color32::WHITE, 0.25)
                } else {
                    t
                };
                (fill, fill, WELL)
            }
            (Look::Outline(t), true) => {
                (if hot { BAND } else { WELL }, if hot { t } else { ETCH }, t)
            }
        };
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            p.rect_filled(rect, RADIUS, fill);
            p.rect_stroke(rect, RADIUS, Stroke::new(1.0, stroke), StrokeKind::Inside);
            if self.more {
                let tint = if hot { VALUE } else { LEGEND };
                for dx in [-5.0, 0.0, 5.0] {
                    p.circle_filled(rect.center() + egui::vec2(dx, 0.0), 1.6, tint);
                }
            } else if !self.label.is_empty() {
                let g = self.lettered(ui, ink, rect.width());
                ui.painter().galley(rect.center() - g.size() / 2.0, g, ink);
            }
            focus_ring(ui, &resp, rect);
        }
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }
}

impl Widget for Key {
    fn ui(self, ui: &mut Ui) -> Response {
        let w = self.width(ui).min(crate::text::room(ui));
        let h = if self.label.is_empty() {
            KEY_H
        } else {
            (self.lettered(ui, VALUE, w).size().y + 12.0).max(KEY_H)
        };
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
        self.paint(ui, rect)
    }
}

pub fn busy(ui: &mut Ui, rect: Rect, done: Option<f32>, label: &str, tint: Color32) -> Response {
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    let resp = ui.interact(rect, id, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, RADIUS, WELL);
    match done {
        Some(f) => {
            let mut fill = rect;
            fill.set_width(rect.width() * f.clamp(0.0, 1.0));
            p.rect_filled(fill, RADIUS, tint.gamma_multiply(0.28));
            let mut edge = fill;
            edge.set_top(rect.bottom() - 2.0);
            p.rect_filled(edge, 0.0, tint);
        }
        None => {
            let t = (ui.input(|i| i.time) as f32 % 1.6) / 1.6;
            let w = rect.width() * 0.25;
            let x = rect.left() + (rect.width() + w) * t - w;
            let sweep =
                Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(x + w, rect.bottom()))
                    .intersect(rect);
            p.rect_filled(sweep, RADIUS, tint.gamma_multiply(0.28));
            ui.ctx().request_repaint();
        }
    }
    p.rect_stroke(rect, RADIUS, Stroke::new(1.0, tint.gamma_multiply(0.6)), StrokeKind::Inside);
    let g = crate::text::lay(ui, job(label, tint), rect.width() - KEY_PAD);
    p.galley(rect.center() - g.size() / 2.0, g, tint);
    resp
}

fn job(label: &str, tint: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        &label.to_uppercase(),
        0.0,
        TextFormat {
            font_id: theme::legend_font(KEY_SIZE),
            color: tint,
            extra_letter_spacing: KEY_TRACKING,
            ..Default::default()
        },
    );
    job
}

pub fn focus_ring(ui: &Ui, resp: &Response, rect: Rect) {
    if resp.has_focus() && !ui.input(|i| i.pointer.any_down()) {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            RADIUS,
            Stroke::new(1.5, READOUT),
            StrokeKind::Outside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunFrame;
    use egui::{Pos2, Rect};

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, enabled: bool) -> (bool, Rect) {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0));
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut clicked = false;
        let mut at = Rect::NOTHING;
        let _ = ctx.run_frame(input, |ui| {
            let r = ui.add(Key::new("launch").solid(VALUE).enabled(enabled));
            clicked = r.clicked();
            at = r.rect;
        });
        (clicked, at)
    }

    fn press(at: Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn a_key_is_as_tall_as_every_other_and_clicks() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let (_, rect) = frame(&ctx, vec![], true);
        assert_eq!(rect.height(), KEY_H);
        let _ = frame(&ctx, press(rect.center(), true), true);
        assert!(frame(&ctx, press(rect.center(), false), true).0);
    }

    #[test]
    fn a_busy_key_says_how_far_it_has_got() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0));
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let out = ctx.run_frame(input, |ui| {
            let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(300.0, KEY_H));
            busy(ui, rect, Some(0.5), "downloading 7 of 14 MB", READOUT);
        });
        let said = out.shapes.iter().any(|c| match &c.shape {
            egui::Shape::Text(t) => t.galley.text() == "DOWNLOADING 7 OF 14 MB",
            _ => false,
        });
        assert!(said);
    }

    #[test]
    fn a_disabled_key_does_not_click() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let (_, rect) = frame(&ctx, vec![], false);
        let _ = frame(&ctx, press(rect.center(), true), false);
        assert!(!frame(&ctx, press(rect.center(), false), false).0);
    }
}
