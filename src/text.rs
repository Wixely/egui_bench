use crate::theme::{
    self, FIGURE_FONT, LEGEND, LEGEND_FONT, LEGEND_SIZE, READOUT, TRACE, VALUE, VALUE_SIZE,
};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, RichText, Sense, Stroke, Vec2};

const TRACKING: f32 = 1.5;
const SPAN_GAP: f32 = 8.0;
const LINE_H: f32 = 18.0;
const LINE_BASELINE: f32 = 13.0;

/// The width a widget may use on the line it is on. A wrapping row offers its whole width, because
/// a widget that does not fit moves down to the next line; any other row offers what is left of it.
/// Never under `MIN_ROOM`, so a nearly full row does not squeeze text into a column of letters.
pub fn room(ui: &egui::Ui) -> f32 {
    let width = if ui.layout().main_wrap() { ui.max_rect().width() } else { ui.available_width() };
    width.max(MIN_ROOM)
}

const MIN_ROOM: f32 = 48.0;

/// Lay `job` out on lines no wider than `width`. A word wider than that is broken, so nothing
/// reaches past it.
pub fn lay(ui: &egui::Ui, mut job: LayoutJob, width: f32) -> std::sync::Arc<egui::Galley> {
    job.wrap.max_width = width.max(1.0);
    ui.fonts_mut(|f| f.layout_job(job))
}

/// `text` in capitals as one run of `font`, tracked by `spacing`, ready for [`lay`].
pub fn caps(text: &str, font: FontId, tint: Color32, spacing: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        TextFormat {
            font_id: font,
            color: tint,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    job
}

pub fn legend(text: impl Into<String>) -> RichText {
    RichText::new(text.into().to_uppercase())
        .font(theme::legend_font(LEGEND_SIZE))
        .extra_letter_spacing(TRACKING)
        .color(LEGEND)
}

pub fn value(text: impl Into<String>) -> RichText {
    RichText::new(text).font(theme::figure(VALUE_SIZE)).color(VALUE)
}

pub fn action(text: impl Into<String>) -> RichText {
    RichText::new(text).font(theme::legend_font(VALUE_SIZE + 0.5)).color(VALUE)
}

pub fn legend_job(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut f = Line::face(LEGEND_FONT, LEGEND_SIZE, LEGEND);
    f.extra_letter_spacing = TRACKING;
    job.append(&text.to_uppercase(), 0.0, f);
    job
}

#[derive(Default)]
pub struct Line {
    job: LayoutJob,
    gap: Option<f32>,
}

impl Line {
    pub fn new() -> Self {
        Self::default()
    }

    fn face(name: &'static str, size: f32, colour: Color32) -> TextFormat {
        TextFormat {
            font_id: FontId::new(size, FontFamily::Name(name.into())),
            color: colour,
            ..Default::default()
        }
    }

    fn add(mut self, text: impl Into<String>, format: TextFormat) -> Self {
        let lead = match self.job.sections.is_empty() {
            true => 0.0,
            false => self.gap.take().unwrap_or(SPAN_GAP),
        };
        self.job.append(&text.into(), lead, format);
        self
    }

    pub fn legend(self, text: &str) -> Self {
        let mut f = Self::face(LEGEND_FONT, LEGEND_SIZE, LEGEND);
        f.extra_letter_spacing = TRACKING;
        self.add(text.to_uppercase(), f)
    }

    pub fn value(self, text: impl Into<String>) -> Self {
        self.add(text, Self::face(FIGURE_FONT, VALUE_SIZE, VALUE))
    }

    pub fn set(self, text: impl Into<String>) -> Self {
        self.add(text, Self::face(FIGURE_FONT, VALUE_SIZE, READOUT))
    }

    pub fn measured(self, text: impl Into<String>) -> Self {
        self.add(text, Self::face(FIGURE_FONT, VALUE_SIZE, TRACE))
    }

    pub fn note(self, text: impl Into<String>) -> Self {
        self.add(
            text,
            TextFormat { font_id: FontId::proportional(12.0), color: LEGEND, ..Default::default() },
        )
    }

    pub fn gap(mut self, px: f32) -> Self {
        self.gap = Some(px);
        self
    }

    pub fn column(mut self, ui: &egui::Ui, x: f32) -> Self {
        let so_far = ui.ctx().fonts_mut(|f| f.layout_job(self.job.clone()).size().x);
        self.gap = Some((x - so_far).max(SPAN_GAP));
        self
    }

    pub fn tint(mut self, colour: Color32) -> Self {
        if let Some(s) = self.job.sections.last_mut() {
            s.format.color = colour;
        }
        self
    }

    pub fn size(mut self, px: f32) -> Self {
        if let Some(s) = self.job.sections.last_mut() {
            s.format.font_id.size = px;
        }
        self
    }

    /// How big the line is when it has all the room it wants.
    pub fn measure(&self, ui: &egui::Ui) -> Vec2 {
        let mut job = self.job.clone();
        job.wrap.max_width = f32::INFINITY;
        ui.fonts_mut(|f| f.layout_job(job)).size()
    }

    pub fn job(self) -> LayoutJob {
        self.job
    }

    pub fn show(mut self, ui: &mut egui::Ui) -> egui::Response {
        if self.job.wrap.max_width == f32::INFINITY {
            self.job.wrap.max_width = room(ui);
        }
        let galley = ui.fonts_mut(|f| f.layout_job(self.job));
        let baseline = baseline_of(&galley);
        let size = galley.size();
        let lift = (LINE_BASELINE - baseline).max(0.0);
        let h = LINE_H.max(size.y + lift);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(size.x, h), Sense::hover());
        if ui.is_rect_visible(rect) {
            ui.painter().galley(egui::pos2(rect.left(), rect.top() + lift), galley, VALUE);
        }
        resp
    }

    pub fn elided(mut self, ui: &mut egui::Ui) -> egui::Response {
        self.job.wrap.max_width = ui.available_width().max(1.0);
        self.job.wrap.max_rows = 1;
        self.job.wrap.overflow_character = Some('…');
        self.show(ui)
    }

    pub fn wrapped(mut self, ui: &mut egui::Ui) -> egui::Response {
        self.job.wrap.max_width = ui.available_width();
        ui.add(egui::Label::new(self.job).wrap())
    }

    pub fn hanging(self, ui: &mut egui::Ui, column: f32, value: Line) -> egui::Response {
        let head = ui.fonts_mut(|f| f.layout_job(self.job));
        let x = column.max(head.size().x + SPAN_GAP);
        let width = ui.available_width();
        let mut job = value.job;
        job.wrap.max_width = (width - x).max(SPAN_GAP);
        let tail = ui.fonts_mut(|f| f.layout_job(job));
        let (head_base, tail_base) = (baseline_of(&head), baseline_of(&tail));
        let base = LINE_BASELINE.max(head_base).max(tail_base);
        let h = LINE_H.max(base - head_base + head.size().y).max(base - tail_base + tail.size().y);
        let w = (x + tail.size().x).min(width.max(x));
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            p.galley(egui::pos2(rect.left(), rect.top() + base - head_base), head, VALUE);
            p.galley(egui::pos2(rect.left() + x, rect.top() + base - tail_base), tail, VALUE);
        }
        resp
    }
}

fn baseline_of(galley: &egui::Galley) -> f32 {
    galley
        .rows
        .first()
        .map(|r| r.pos.y + r.glyphs.iter().map(|g| g.pos.y).fold(0.0f32, f32::max))
        .unwrap_or(LINE_BASELINE)
}

pub fn note(ui: &mut egui::Ui, text: impl Into<String>, colour: Color32) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.add(egui::Label::new(RichText::new(text).size(11.5).color(colour)).wrap());
        },
    );
}

pub fn hint(ui: &mut egui::Ui, text: &str) {
    Line::new().note(text).size(10.5).wrapped(ui);
}

pub fn help(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    let col = if r.hovered() { READOUT } else { LEGEND };
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.circle_stroke(rect.center(), 6.0, Stroke::new(1.0, col));
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "?", theme::legend_font(10.0), col);
    }
    r.on_hover_text(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunFrame;

    #[test]
    fn spans_of_different_sizes_share_a_baseline() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let _ = ctx.run_frame(Default::default(), |_| {});
        let job = Line::new().legend("squelch").value("-42 dBFS").size(11.0).job;
        let galley = ctx.fonts_mut(|f| f.layout_job(job));
        assert_eq!(galley.rows.len(), 1, "a short line wrapped");
        let ys: Vec<f32> = galley.rows[0].glyphs.iter().map(|g| g.pos.y).collect();
        let lo = ys.iter().cloned().fold(f32::MAX, f32::min);
        let hi = ys.iter().cloned().fold(f32::MIN, f32::max);
        assert!(hi - lo <= 1.0, "baselines differ by {:.2} px", hi - lo);
    }

    #[test]
    fn a_hanging_value_wraps_inside_its_column_and_keeps_to_the_width() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let _ = ctx.run_frame(Default::default(), |ui| {
            ui.set_max_width(300.0);
            let long = "435.3350 MHz GFSK (Mode U - GFSK9k6 - AX.25 Beacon and Ham Payload)";
            let short = Line::new().legend("level").hanging(ui, 98.0, Line::new().value("-7 dB"));
            let r = Line::new().legend("downlink").hanging(ui, 98.0, Line::new().value(long));
            assert!(r.rect.width() <= 300.0, "a row {:.1} wide in 300", r.rect.width());
            assert!(r.rect.height() >= 2.0 * short.rect.height(), "did not wrap: {:?}", r.rect);
            assert_eq!(short.rect.height(), LINE_H, "a short row is one line high");
        });
    }

    #[test]
    fn a_column_lands_the_value_where_it_was_asked_for() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let _ = ctx.run_frame(Default::default(), |ui| {
            let job = Line::new().legend("mode").column(ui, 90.0).value("FM").job;
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let x = galley.rows[0].glyphs.last().unwrap().pos.x;
            assert!(x >= 90.0, "value started at {x:.1}");
        });
    }
}
