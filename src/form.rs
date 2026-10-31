use crate::readout::LABEL_W;
use crate::text::{help, legend};
use crate::theme::{self, ETCH, LEGEND, RADIUS, VALUE, WELL};
use egui::{Layout, Pos2, Response, Stroke, Ui};

pub fn modal_title(ui: &mut Ui, text: &str) {
    ui.label(legend(text));
    ui.add_space(10.0);
}

/// Room the control wants beside its label. With less than this the label goes above it.
const CONTROL_W: f32 = 150.0;

fn stacked(ui: &Ui) -> bool {
    ui.available_width() < LABEL_W + ui.spacing().item_spacing.x + CONTROL_W
}

/// The label in a column `LABEL_W` wide, wrapping inside it.
fn label_cell(ui: &mut Ui, label: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(LABEL_W, 18.0),
        Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(LABEL_W);
            ui.add(egui::Label::new(legend(label)).wrap());
        },
    );
}

/// `add` in a row that wraps, so a control that does not fit goes to the next line instead of
/// past the edge.
fn controls(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        add(ui)
    });
}

pub fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    if stacked(ui) {
        ui.add(egui::Label::new(legend(label)).wrap());
        controls(ui, add);
        return;
    }
    ui.horizontal(|ui| {
        label_cell(ui, label);
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        add(ui);
    });
}

pub fn row_help(ui: &mut Ui, label: &str, text: &str, add: impl FnOnce(&mut Ui)) {
    if stacked(ui) {
        ui.add(egui::Label::new(legend(label)).wrap());
        controls(ui, |ui| {
            add(ui);
            help(ui, text);
        });
        return;
    }
    ui.horizontal(|ui| {
        label_cell(ui, label);
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            help(ui, text);
            ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                add(ui)
            });
        });
    });
}

pub fn switch(ui: &mut Ui, label: &str, on: &mut bool, text: &str, explain: &str) -> bool {
    let mut changed = false;
    row_help(ui, label, explain, |ui| {
        changed = ui.checkbox(on, text).changed();
    });
    changed
}

pub fn choice<T: PartialEq + Clone>(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    picked: &mut T,
    options: impl IntoIterator<Item = (T, String)>,
) -> bool {
    let options: Vec<(T, String)> = options.into_iter().collect();
    let shown = options.iter().find(|(v, _)| v == picked).map(|(_, l)| l.clone());
    let mut changed = false;
    let name = format!("{id:?}").trim_matches('"').replace(['_', '-'], " ");
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(shown.unwrap_or_default())
        .width(ui.available_width())
        .truncate()
        .show_ui(ui, |ui| {
            for (v, label) in options {
                let on = *picked == v;
                if ui.selectable_label(on, label).clicked() && !on {
                    *picked = v;
                    changed = true;
                }
            }
        });
    ui.ctx().accesskit_node_builder(combo.response.id, |node| node.set_label(name.clone()));
    changed
}

pub fn field(ui: &mut Ui, text: &mut String, hint: &str) -> Response {
    field_of(ui, text, hint, false, 1, f32::INFINITY)
}

pub fn field_then(
    ui: &mut Ui,
    text: &mut String,
    hint: &str,
    reserve: f32,
    after: impl FnOnce(&mut Ui),
) -> Response {
    let w = (ui.available_width() - reserve).max(60.0);
    let r = field_of(ui, text, hint, false, 1, w);
    after(ui);
    r
}

pub fn secret(ui: &mut Ui, text: &mut String) -> Response {
    field_of(ui, text, "", true, 1, f32::INFINITY)
}

pub fn prose(ui: &mut Ui, text: &mut String, hint: &str, rows: usize) -> Response {
    field_of(ui, text, hint, false, rows, f32::INFINITY)
}

fn field_of(
    ui: &mut Ui,
    text: &mut String,
    hint: &str,
    secret: bool,
    rows: usize,
    width: f32,
) -> Response {
    let font = theme::figure(12.5);
    let hint_text = hint.to_string();
    let hint = egui::RichText::new(hint).font(font.clone()).color(LEGEND.gamma_multiply(0.6));
    egui::Frame::NONE
        .fill(WELL)
        .stroke(Stroke::new(1.0, ETCH))
        .corner_radius(RADIUS)
        .inner_margin(egui::Margin { left: 6, right: 6, top: 3, bottom: 3 })
        .show(ui, |ui| {
            let mut edit = if rows > 1 {
                egui::TextEdit::multiline(text).desired_rows(rows)
            } else {
                egui::TextEdit::singleline(text)
            };
            edit = edit
                .frame(egui::Frame::NONE)
                .font(font)
                .text_color(VALUE)
                .hint_text(hint)
                .password(secret)
                .desired_width(if width.is_finite() { width - 12.0 } else { width });
            let response = ui.add(edit);
            let name = if hint_text.is_empty() && secret { "secret" } else { hint_text.as_str() };
            if !name.is_empty() {
                ui.ctx().accesskit_node_builder(response.id, |node| node.set_label(name));
            }
            clipboard_menu(response)
        })
        .inner
}

pub fn clipboard_menu(response: Response) -> Response {
    response.context_menu(|ui| {
        for (label, command) in [
            ("CUT", egui::ViewportCommand::RequestCut),
            ("COPY", egui::ViewportCommand::RequestCopy),
            ("PASTE", egui::ViewportCommand::RequestPaste),
        ] {
            if ui.button(legend(label)).clicked() {
                response.request_focus();
                ui.ctx().send_viewport_cmd(command);
                ui.close();
            }
        }
        if ui.button(legend("SELECT ALL")).clicked() {
            response.request_focus();
            select_all(ui.ctx(), response.id);
            ui.close();
        }
    });
    response
}

fn select_all(ctx: &egui::Context, id: egui::Id) {
    use egui::text::{CCursor, CCursorRange};
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(usize::MAX))));
    state.store(ctx, id);
}

pub fn footer(ui: &mut Ui, buttons: impl FnOnce(&mut Ui)) {
    ui.add_space(10.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [Pos2::new(r.left(), r.top()), Pos2::new(r.right(), r.top())],
        Stroke::new(1.0, ETCH),
    );
    ui.add_space(8.0);
    ui.with_layout(Layout::right_to_left(egui::Align::Center).with_main_wrap(true), buttons);
}

#[cfg(test)]
mod tests {
    use crate::RunFrame;

    #[test]
    fn select_all_covers_every_character_across_lines() {
        let ctx = egui::Context::default();
        let mut text = String::from("héllo\nworld");
        let mut selected = None;
        for pass in 0..2 {
            let _ = ctx.run_frame(Default::default(), |ui| {
                let out = egui::TextEdit::multiline(&mut text).show(ui);
                if pass == 0 {
                    out.response.request_focus();
                    super::select_all(ui.ctx(), out.response.id);
                } else {
                    selected = out.cursor_range.map(|r| {
                        let s = r.as_sorted_char_range();
                        s.start.0..s.end.0
                    });
                }
            });
        }
        assert_eq!(selected, Some(0..11));
    }

    #[test]
    fn a_paste_after_the_menu_refocuses_lands_in_the_field() {
        let ctx = egui::Context::default();
        let mut text = String::from("433");
        for pass in 0..2 {
            let mut input = egui::RawInput::default();
            if pass == 1 {
                input.events.push(egui::Event::Paste(".92".into()));
            }
            let _ = ctx.run_frame(input, |ui| {
                let response = ui.text_edit_singleline(&mut text);
                if pass == 0 {
                    response.request_focus();
                }
            });
        }
        assert_eq!(text, "433.92");
    }
}
