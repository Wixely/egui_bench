use crate::text::{self, Line};
use crate::theme::{
    self, ETCH, FAULT, LEGEND, LEGEND_SIZE, OK, PANEL, RADIUS, RAIL_W, READOUT, VALUE, WELL,
};
use egui::{Color32, InnerResponse, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

const PAD_X: i8 = 10;

pub fn card<R>(
    ui: &mut Ui,
    rail: Option<Color32>,
    header: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let outer = egui::Frame::NONE.fill(PANEL).stroke(Stroke::new(1.0, ETCH)).corner_radius(RADIUS);
    let framed = outer.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let head = egui::Frame::NONE.fill(WELL).inner_margin(egui::Margin {
            left: PAD_X,
            right: PAD_X,
            top: 4,
            bottom: 4,
        });
        let h = head.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| header(ui));
        });
        let r = h.response.rect;
        ui.painter().line_segment(
            [Pos2::new(r.left(), r.bottom()), Pos2::new(r.right(), r.bottom())],
            Stroke::new(1.0, ETCH),
        );
        egui::Frame::NONE
            .inner_margin(egui::Margin { left: PAD_X, right: PAD_X, top: 6, bottom: 8 })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.set_width(ui.available_width());
                body(ui)
            })
            .inner
    });
    if let Some(c) = rail {
        let r = framed.response.rect;
        ui.painter().rect_filled(
            Rect::from_min_max(r.left_top(), Pos2::new(r.left() + RAIL_W, r.bottom())),
            0.0,
            c,
        );
    }
    framed
}

pub fn section<R>(
    ui: &mut Ui,
    label: &str,
    note: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    card(
        ui,
        None,
        |ui| {
            let label_w = Line::new().legend(label).measure(ui).x;
            // The note sits at the right of the label while there is room for some of it, and
            // goes under the label, wrapped, when there is not.
            if note.is_empty() || ui.available_width() - label_w >= 60.0 {
                Line::new().legend(label).show(ui);
                if !note.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Line::new().note(note).size(10.5).elided(ui);
                    });
                }
            } else {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    Line::new().legend(label).show(ui);
                    Line::new().note(note).size(10.5).wrapped(ui);
                });
            }
        },
        body,
    )
}

pub fn lamp(ui: &mut Ui, text: &str, on: bool, fault: bool) -> Response {
    let tint = if fault {
        FAULT
    } else if on {
        OK
    } else {
        LEGEND
    };
    let galley = text::lay(
        ui,
        text::caps(text, theme::legend_font(LEGEND_SIZE), tint, 0.0),
        text::room(ui) - 14.0,
    );
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, RADIUS as f32, if on { tint.gamma_multiply(0.18) } else { WELL });
        p.rect_stroke(
            rect,
            RADIUS as f32,
            Stroke::new(1.0, if on || fault { tint } else { ETCH }),
            StrokeKind::Inside,
        );
        p.galley(rect.center() - galley.size() / 2.0, galley, tint);
    }
    resp
}

pub fn toggle(ui: &mut Ui, text: &str, on: bool) -> Response {
    let tint = if on { READOUT } else { LEGEND };
    let galley = text::lay(
        ui,
        text::caps(text, theme::legend_font(LEGEND_SIZE), tint, 0.0),
        text::room(ui) - 26.0,
    );
    let size = galley.size() + Vec2::new(26.0, 7.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let lit = on || resp.hovered();
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let fill = if on {
            tint.gamma_multiply(0.18)
        } else if resp.hovered() {
            ETCH.gamma_multiply(0.6)
        } else {
            WELL
        };
        p.rect_filled(rect, RADIUS as f32, fill);
        p.rect_stroke(
            rect,
            RADIUS as f32,
            Stroke::new(1.0, if lit { tint } else { ETCH }),
            StrokeKind::Inside,
        );
        let tick = Rect::from_center_size(
            Pos2::new(rect.left() + 11.0, rect.center().y),
            Vec2::splat(8.0),
        );
        p.rect_stroke(
            tick,
            1.0,
            Stroke::new(1.0, if lit { tint } else { LEGEND }),
            StrokeKind::Inside,
        );
        if on {
            p.rect_filled(tick.shrink(2.0), 0.0, tint);
        }
        p.galley(Pos2::new(tick.right() + 6.0, rect.top() + 3.5), galley, tint);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn status(ui: &mut Ui, ok: bool, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 18.0), Sense::hover());
        let c = rect.center();
        let col = if ok { OK } else { FAULT };
        ui.painter().circle_filled(c, 3.0, col);
        ui.painter().circle_stroke(c, 4.5, Stroke::new(1.0, col.gamma_multiply(0.4)));
        Line::new().value(text).tint(col).size(11.0).wrapped(ui);
    });
}

pub fn stage_rail(
    ui: &mut Ui,
    stages: &[&str],
    at: Option<usize>,
    now: Color32,
    height: f32,
    muted: bool,
) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter();
    let n = stages.len().max(1);
    let gap = 5.0;
    let seg_w = ((rect.width() - gap * (n as f32 - 1.0)) / n as f32).max(8.0);
    for (i, name) in stages.iter().enumerate() {
        let x = rect.left() + i as f32 * (seg_w + gap);
        let seg = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(seg_w, rect.height()));
        let (fill, edge, ink) = match at {
            Some(a) if i == a => (if muted { WELL } else { now.gamma_multiply(0.20) }, now, now),
            Some(a) if i < a => (WELL, ETCH, OK.gamma_multiply(0.85)),
            _ => (WELL, ETCH, LEGEND.gamma_multiply(0.65)),
        };
        p.rect_filled(seg, RADIUS as f32, fill);
        p.rect_stroke(seg, RADIUS as f32, Stroke::new(1.0, edge), StrokeKind::Inside);
        let size = if height < 22.0 { LEGEND_SIZE - 1.5 } else { LEGEND_SIZE };
        let galley = p.layout_no_wrap(name.to_uppercase(), theme::legend_font(size), ink);
        p.galley(seg.center() - galley.size() / 2.0, galley, ink);
    }
    resp
}

pub fn exit_note(ui: &mut Ui, text: &str, tint: Color32) {
    if text.is_empty() {
        return;
    }
    ui.add_space(3.0);
    ui.add(
        egui::Label::new(egui::RichText::new(text).font(theme::legend_font(12.0)).color(tint))
            .wrap(),
    );
}

pub fn tabs<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, options: &[(T, &str)]) -> bool {
    const TAB_H: f32 = 20.0;
    const MARK_H: f32 = 2.0;
    const GAP: f32 = 20.0;

    let width = ui.available_width();
    let left = ui.cursor().left();
    let mut changed = false;
    // Laid out first so the tabs can be set in rows: one that does not fit goes on the next row.
    let mut placed = Vec::with_capacity(options.len());
    let (mut x, mut row) = (0.0f32, 0usize);
    for (value, label) in options {
        let galley = text::lay(ui, text::legend_job(label), width);
        let w = galley.size().x;
        if x > 0.0 && x + w > width {
            x = 0.0;
            row += 1;
        }
        placed.push((value, galley, x, row, w));
        x += w + GAP;
    }
    let rows = row + 1;
    let height = rows as f32 * (TAB_H + MARK_H);
    let (strip, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let mut marks = Vec::with_capacity(options.len());
    for (i, (value, galley, x, row, w)) in placed.into_iter().enumerate() {
        let top = strip.top() + row as f32 * (TAB_H + MARK_H);
        let h = galley.size().y.max(TAB_H);
        let hit = Rect::from_min_size(Pos2::new(left + x, top), Vec2::new(w, h));
        let r = ui.interact(hit, ui.scope_id().with(("tab", i)), Sense::click());
        let on = *current == *value;
        if r.clicked() && !on {
            *current = *value;
            changed = true;
        }
        let colour = if on || r.hovered() { VALUE } else { LEGEND };
        let at = Pos2::new(left + x, top + (TAB_H - galley.size().y) * 0.5);
        ui.painter().galley(at, galley, colour);
        marks.push((left + x, top, w, on));
    }
    for row in 0..rows {
        let bottom = strip.top() + (row + 1) as f32 * (TAB_H + MARK_H);
        let rule =
            Rect::from_min_size(Pos2::new(strip.left(), bottom - MARK_H), Vec2::new(width, 1.0));
        ui.painter().rect_filled(rule, 0.0, ETCH);
    }
    for (x, top, w, on) in marks {
        if on {
            let mark = Rect::from_min_size(Pos2::new(x, top + TAB_H), Vec2::new(w, MARK_H));
            ui.painter().rect_filled(mark, 0.0, READOUT);
        }
    }
    ui.add_space(10.0);
    changed
}

pub fn chip(ui: &mut Ui, text: &str, tint: Color32, lit: bool) -> Response {
    let galley =
        text::lay(ui, text::caps(text, theme::legend_font(10.5), tint, 1.4), text::room(ui) - 14.0);
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, RADIUS, if lit { tint.gamma_multiply(0.16) } else { WELL });
        p.rect_stroke(
            rect,
            RADIUS,
            Stroke::new(1.0, if lit { tint } else { ETCH }),
            StrokeKind::Inside,
        );
        p.galley(rect.center() - galley.size() / 2.0, galley, tint);
    }
    resp
}

pub fn heading(ui: &mut Ui, label: &str) -> Response {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &label.to_uppercase(),
        0.0,
        egui::TextFormat {
            font_id: theme::legend_font(12.0),
            color: LEGEND,
            extra_letter_spacing: 2.0,
            ..Default::default()
        },
    );
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), galley.size().y), Sense::hover());
    if ui.is_rect_visible(rect) {
        let y = rect.center().y;
        let from = rect.left() + galley.size().x + 12.0;
        let p = ui.painter();
        p.galley(rect.left_top(), galley, LEGEND);
        if from < rect.right() {
            p.line_segment(
                [Pos2::new(from, y), Pos2::new(rect.right(), y)],
                Stroke::new(1.0, ETCH),
            );
        }
    }
    resp
}

pub fn segments<T: PartialEq + Copy>(
    ui: &mut Ui,
    current: &mut T,
    options: &[(T, &str, Option<usize>)],
) -> bool {
    const PAD: f32 = 26.0;
    let mut changed = false;
    let left = ui.cursor().left();
    let width = ui.available_width();
    let top = ui.cursor().top();
    let mut x = 0.0f32;
    let mut row = 0usize;
    let mut right = 0.0f32;
    for (n, (value, label, count)) in options.iter().enumerate() {
        let on = *current == *value;
        let tint = if on { READOUT } else { LEGEND };
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &label.to_uppercase(),
            0.0,
            egui::TextFormat {
                font_id: theme::legend_font(LEGEND_SIZE),
                color: tint,
                extra_letter_spacing: 1.5,
                ..Default::default()
            },
        );
        if let Some(c) = count {
            let ink = if on { READOUT } else { LEGEND.gamma_multiply(0.7) };
            job.append(
                &c.to_string(),
                8.0,
                egui::TextFormat {
                    font_id: theme::figure(LEGEND_SIZE),
                    color: ink,
                    ..Default::default()
                },
            );
        }
        let galley = text::lay(ui, job, (width - PAD).max(1.0));
        let w = galley.size().x + PAD;
        let h = galley.size().y.max(crate::key::KEY_H - 12.0) + 12.0;
        // A segment that does not fit on this row starts the next, one row below.
        if x > 0.0 && x + w > width {
            x = 0.0;
            row += 1;
        }
        let rect = Rect::from_min_size(
            Pos2::new(left + x, top + row as f32 * (crate::key::KEY_H - 1.0)),
            Vec2::new(w, h.max(crate::key::KEY_H)),
        );
        let resp = ui.interact(rect, ui.scope_id().with(("segment", n)), Sense::click());
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            let fill = if on {
                READOUT.gamma_multiply(0.14)
            } else if resp.hovered() {
                theme::BAND
            } else {
                WELL
            };
            p.rect_filled(rect, 0.0, fill);
            p.rect_stroke(rect, 0.0, Stroke::new(1.0, ETCH), StrokeKind::Inside);
            if on {
                let mark = Rect::from_min_max(
                    Pos2::new(rect.left(), rect.bottom() - 2.0),
                    rect.right_bottom(),
                );
                p.rect_filled(mark, 0.0, READOUT);
            }
            p.galley(rect.center() - galley.size() / 2.0, galley, tint);
            crate::key::focus_ring(ui, &resp, rect);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() && !on {
            *current = *value;
            changed = true;
        }
        right = right.max(x + w);
        x += w - 1.0;
    }
    let rows = row + 1;
    ui.allocate_rect(
        Rect::from_min_max(
            Pos2::new(left, top),
            Pos2::new(left + right + 1.0, top + rows as f32 * (crate::key::KEY_H - 1.0) + 1.0),
        ),
        Sense::hover(),
    );
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunFrame;

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Pick {
        All,
        Some,
    }

    fn frame(ctx: &egui::Context, pick: &mut Pick, events: Vec<egui::Event>) -> Vec<Rect> {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 200.0));
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut rects = Vec::new();
        let out = ctx.run_frame(input, |ui| {
            segments(ui, pick, &[(Pick::All, "all", Some(6)), (Pick::Some, "some", Some(2))]);
        });
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                rects.push(t.galley.rect.translate(t.pos.to_vec2()));
            }
        }
        rects
    }

    #[test]
    fn a_segment_click_picks_it() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let mut pick = Pick::All;
        let texts = frame(&ctx, &mut pick, vec![]);
        let at = texts[1].center();
        let click = |pressed| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        frame(&ctx, &mut pick, click(true));
        frame(&ctx, &mut pick, click(false));
        assert_eq!(pick, Pick::Some);
    }
}
