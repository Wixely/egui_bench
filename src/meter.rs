use crate::text::Line;
use crate::theme::{CHASSIS, ETCH, FAULT, LEGEND, READOUT, SAFE, TRACE, VALUE, WARN, WELL};
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, Widget};

pub const VU_H: f32 = 6.0;
pub const FADER_H: f32 = 14.0;
const GRIP: f32 = 3.0;
const WARN_AT: f32 = 0.70;
const PEAK_AT: f32 = 0.90;

pub fn vu(p: &egui::Painter, r: Rect, peak: f32) {
    p.rect_filled(r, 1.0, WELL);
    p.rect_stroke(r, 1.0, Stroke::new(1.0, ETCH), StrokeKind::Inside);

    let at = |v: f32| r.left() + v.clamp(0.0, 1.0).sqrt() * r.width();
    for (v, c) in [(WARN_AT, WARN), (PEAK_AT, FAULT)] {
        let x = at(v);
        p.line_segment(
            [Pos2::new(x, r.top() + 1.0), Pos2::new(x, r.bottom() - 1.0)],
            Stroke::new(1.0, c.gamma_multiply(0.45)),
        );
    }

    let peak = peak.clamp(0.0, 1.0);
    if peak <= 0.001 {
        return;
    }
    let end = at(peak);
    let mut x = r.left();
    for (limit, colour) in [(WARN_AT, SAFE), (PEAK_AT, WARN), (1.0, FAULT)] {
        let stop = at(limit).min(end);
        if stop > x {
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x, r.top() + 1.0),
                    Pos2::new(stop.max(x + 1.0), r.bottom() - 1.0),
                ),
                0.0,
                colour,
            );
        }
        x = stop;
        if x >= end {
            break;
        }
    }
}

fn handle(p: &egui::Painter, rect: Rect, x: f32, hot: bool) {
    let h =
        Rect::from_center_size(Pos2::new(x, rect.center().y), Vec2::new(GRIP * 2.0, rect.height()));
    p.rect_filled(h, 1.0, CHASSIS);
    p.rect_filled(h.shrink(1.0), 1.0, if hot { VALUE } else { READOUT });
}

const WHEEL_NOTCH: f32 = 50.0;

fn wheel(ui: &Ui, resp: &Response) -> f32 {
    if !resp.hovered() {
        return 0.0;
    }
    let delta = ui.input(|i| i.smooth_scroll_delta);
    if delta == Vec2::ZERO {
        return 0.0;
    }
    ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
    (delta.y + delta.x) / WHEEL_NOTCH
}

pub struct Fader<'a> {
    value: &'a mut f32,
    peak: f32,
    width: f32,
}

impl<'a> Fader<'a> {
    pub fn new(value: &'a mut f32, peak: f32) -> Self {
        Self { value, peak, width: 130.0 }
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
}

impl Widget for Fader<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let w = self.width.min(ui.available_width()).max(40.0);
        let (rect, mut resp) =
            ui.allocate_exact_size(Vec2::new(w, FADER_H), Sense::click_and_drag());
        let (lo, hi) = (rect.left() + GRIP, rect.right() - GRIP);

        if let Some(p) =
            ui.ctx().pointer_interact_pos().filter(|_| resp.dragged() || resp.clicked())
        {
            let t = ((p.x - lo) / (hi - lo)).clamp(0.0, 1.0);
            if (t - *self.value).abs() > 1e-4 {
                *self.value = t;
                resp.mark_changed();
            }
        }
        let notches = wheel(ui, &resp);
        if notches != 0.0 {
            *self.value = (*self.value + notches * 0.02).clamp(0.0, 1.0);
            resp.mark_changed();
        }
        if !ui.is_rect_visible(rect) {
            return resp;
        }

        let p = ui.painter();
        vu(p, Rect::from_center_size(rect.center(), Vec2::new(rect.width(), VU_H)), self.peak);
        let x = lo + self.value.clamp(0.0, 1.0) * (hi - lo);
        handle(p, rect, x, resp.hovered() || resp.dragged());
        resp
    }
}

pub struct Threshold<'a> {
    value: &'a mut f32,
    range: (f32, f32),
    measured: f32,
    open: bool,
    width: f32,
}

impl<'a> Threshold<'a> {
    pub fn new(value: &'a mut f32, lo: f32, hi: f32, measured: f32, open: bool) -> Self {
        Self { value, range: (lo, hi), measured, open, width: 130.0 }
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
}

impl Widget for Threshold<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (lo, hi) = self.range;
        let w = self.width.min(ui.available_width()).max(40.0);
        let (rect, mut resp) =
            ui.allocate_exact_size(Vec2::new(w, FADER_H), Sense::click_and_drag());
        let (x0, x1) = (rect.left() + GRIP, rect.right() - GRIP);
        let at = |v: f32| x0 + ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * (x1 - x0);

        if let Some(p) =
            ui.ctx().pointer_interact_pos().filter(|_| resp.dragged() || resp.clicked())
        {
            let t = ((p.x - x0) / (x1 - x0)).clamp(0.0, 1.0);
            let v = lo + t * (hi - lo);
            if (v - *self.value).abs() > 1e-3 {
                *self.value = v;
                resp.mark_changed();
            }
        }
        let notches = wheel(ui, &resp);
        if notches != 0.0 {
            *self.value = (*self.value + notches * (hi - lo) / 50.0).clamp(lo, hi);
            resp.mark_changed();
        }
        if !ui.is_rect_visible(rect) {
            return resp;
        }

        let p = ui.painter();
        let well = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), VU_H));
        p.rect_filled(well, 1.0, WELL);
        p.rect_stroke(well, 1.0, Stroke::new(1.0, ETCH), StrokeKind::Inside);
        let end = at(self.measured);
        if end > well.left() + 1.0 {
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(well.left() + 1.0, well.top() + 1.0),
                    Pos2::new(end, well.bottom() - 1.0),
                ),
                0.0,
                if self.open { TRACE } else { LEGEND },
            );
        }
        handle(p, rect, at(*self.value), resp.hovered() || resp.dragged());
        resp
    }
}

pub fn progress(ui: &mut Ui, label: &str, done: f32, total: Option<f32>, said: &str) {
    let ctx = ui.ctx().clone();
    let h = 10.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 1.0, WELL);
    match total.filter(|t| *t > 0.0) {
        Some(total) => {
            let mut bar = rect;
            bar.set_width(rect.width() * (done / total).clamp(0.0, 1.0));
            p.rect_filled(bar, 1.0, TRACE);
        }
        None => {
            let t = ctx.input(|i| i.time) as f32 % 2.0 / 2.0;
            let w = rect.width() * 0.2;
            let x = rect.left() + (rect.width() + w) * t - w;
            let bar =
                Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, h)).intersect(rect);
            p.rect_filled(bar, 1.0, TRACE);
            ctx.request_repaint();
        }
    }
    if !said.is_empty() {
        Line::new().legend(label).value(said).size(11.0).show(ui);
    }
}

pub fn bar(p: &egui::Painter, r: Rect, fraction: f32, tint: Color32) {
    p.rect_filled(r, 1.0, WELL);
    p.rect_stroke(r, 1.0, Stroke::new(1.0, ETCH), StrokeKind::Inside);
    let w = (r.width() - 2.0) * fraction.clamp(0.0, 1.0);
    if w > 0.5 {
        p.rect_filled(
            Rect::from_min_size(r.min + Vec2::splat(1.0), Vec2::new(w, r.height() - 2.0)),
            0.0,
            tint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunFrame;

    fn wheel_over(value: f32, at: Pos2, notches: f32) -> f32 {
        let ctx = egui::Context::default();
        let mut v = value;
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 100.0));
        for frame in 0..60 {
            let mut events = vec![egui::Event::PointerMoved(at)];
            if frame == 1 {
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    source: egui::MouseWheelSource::Wheel,
                    delta: Vec2::new(0.0, notches),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let _ = ctx.run_frame(input, |ui| {
                ui.add(Fader::new(&mut v, 0.0).width(200.0));
            });
        }
        v
    }

    #[test]
    fn the_wheel_moves_a_fader_under_the_pointer_and_no_other() {
        let up = wheel_over(0.5, Pos2::new(100.0, 12.0), 1.0);
        let down = wheel_over(0.5, Pos2::new(100.0, 12.0), -1.0);
        let away = wheel_over(0.5, Pos2::new(100.0, 90.0), 1.0);
        assert!(up > 0.5 && up < 0.6, "one notch up moved it to {up}");
        assert!(down < 0.5 && down > 0.4, "one notch down moved it to {down}");
        assert_eq!(away, 0.5, "a wheel away from the fader moved it");
    }
}
