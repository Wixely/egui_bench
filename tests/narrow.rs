//! Every widget has to cope with a narrow space: a phone, or one tile of four. Text wraps, rows of
//! buttons flow onto another line, and nothing is wider than the room it was given.

use egui::{Color32, Pos2, Rect, Vec2};
use egui_bench::prelude::*;

const WIDTH: f32 = 140.0;
const LONG: &str = "a very long sentence about nothing in particular that must wrap somewhere";
const WORD: &str = "https://example.com/an/extremely/long/address/with/no/spaces/in/it/at/all";

/// Draw `add` in a space `WIDTH` wide and report how far past it anything reached.
fn overflow(add: impl Fn(&mut egui::Ui)) -> f32 {
    let ctx = egui::Context::default();
    install(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(WIDTH, 2000.0));
    let mut right = 0.0f32;
    for _ in 0..3 {
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            ui.set_max_width(WIDTH);
            add(ui);
            right = ui.min_rect().right();
        });
        out.textures_delta.clear();
    }
    right - WIDTH
}

thread_local! {
    static TOO_WIDE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Note a widget that came out wider than its room; `done` fails the test with all of them.
fn fits(name: &str, add: impl Fn(&mut egui::Ui)) {
    let over = overflow(add);
    if over > 0.5 {
        TOO_WIDE.with(|t| t.borrow_mut().push(format!("{name}: {over:.1} px past {WIDTH}")));
    }
}

fn done() {
    let mut all = TOO_WIDE.with(|t| std::mem::take(&mut *t.borrow_mut()));
    all.sort();
    all.dedup();
    assert!(all.is_empty(), "wider than the room it was given:\n{}", all.join("\n"));
}

#[test]
fn text_wraps() {
    for text in [LONG, WORD] {
        fits("Line::wrapped", |ui| {
            Line::new().legend("legend").value(text).wrapped(ui);
        });
        fits("Line::show", |ui| {
            Line::new().legend(text).value(text).show(ui);
        });
        fits("Line::hanging", |ui| {
            Line::new().legend("downlink").hanging(ui, 98.0, Line::new().value(text));
        });
        fits("Line::elided", |ui| {
            Line::new().legend(text).note(text).elided(ui);
        });
        fits("note", |ui| note(ui, text, Color32::WHITE));
        fits("hint", |ui| hint(ui, text));
        fits("reading", |ui| reading(ui, "label", text));
        fits("legend label", |ui| {
            ui.label(legend(text));
        });
        fits("modal_title", |ui| modal_title(ui, text));
        fits("exit_note", |ui| exit_note(ui, text, Color32::WHITE));
    }
    done();
}

#[test]
fn panels_wrap() {
    for text in [LONG, WORD] {
        fits("card", |ui| {
            card(
                ui,
                None,
                |ui| {
                    Line::new().legend(text).show(ui);
                },
                |ui| {
                    Line::new().value(text).wrapped(ui);
                },
            );
        });
        fits("section", |ui| {
            section(ui, text, text, |ui| {
                hint(ui, text);
            });
        });
        fits("status", |ui| status(ui, true, text));
        fits("lamp", |ui| {
            lamp(ui, text, true, false);
        });
        fits("chip", |ui| {
            chip(ui, text, Color32::WHITE, true);
        });
        fits("toggle", |ui| {
            toggle(ui, text, true);
        });
        fits("heading", |ui| {
            heading(ui, text);
        });
        fits("tabs", |ui| {
            let mut at = 0;
            tabs(ui, &mut at, &[(0, text), (1, text), (2, "third")]);
        });
        fits("segments", |ui| {
            let mut at = 0;
            segments(ui, &mut at, &[(0, text, Some(3)), (1, text, None), (2, "third", Some(1))]);
        });
        fits("stage_rail", |ui| {
            stage_rail(ui, &[text, "two", text, "four"], Some(1), Color32::WHITE, 22.0, false);
        });
    }
    done();
}

#[test]
fn readouts_wrap() {
    for text in [LONG, WORD] {
        fits("readout", |ui| readout(ui, text, text, Color32::WHITE));
        fits("readouts", |ui| {
            readouts(
                ui,
                &[("a", text.to_string(), Color32::WHITE), (text, "b".into(), Color32::WHITE)],
            )
        });
        fits("hero", |ui| hero(ui, text, "-123456789.5", text, Color32::WHITE));
    }
    done();
}

#[test]
fn keys_and_forms_wrap() {
    for text in [LONG, WORD] {
        fits("Key", |ui| {
            ui.add(Key::new(text).solid(READOUT));
        });
        fits("two Keys", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(Key::new(text));
                ui.add(Key::new("short"));
            });
        });
        fits("row", |ui| {
            row(ui, text, |ui| {
                ui.label(text);
            })
        });
        fits("row_help", |ui| {
            row_help(ui, text, text, |ui| {
                ui.label(text);
            })
        });
        fits("switch", |ui| {
            let mut on = true;
            switch(ui, text, &mut on, text, text);
        });
        fits("choice", |ui| {
            let mut at = 0;
            choice(ui, "pick", &mut at, [(0, text.to_string()), (1, "b".to_string())]);
        });
        fits("field", |ui| {
            let mut s = text.to_string();
            field(ui, &mut s, text);
        });
        fits("prose", |ui| {
            let mut s = text.to_string();
            prose(ui, &mut s, text, 3);
        });
        fits("field_then", |ui| {
            let mut s = text.to_string();
            field_then(ui, &mut s, text, 60.0, |ui| {
                ui.add(Key::new("go"));
            });
        });
        fits("footer", |ui| {
            footer(ui, |ui| {
                ui.add(Key::new(text));
                ui.add(Key::new(text));
            })
        });
    }
    done();
}

#[test]
fn meters_wrap() {
    for text in [LONG, WORD] {
        fits("progress", |ui| progress(ui, text, 1.0, Some(2.0), text));
        fits("progress without a total", |ui| progress(ui, text, 1.0, None, text));
        fits("busy", |ui| {
            let (rect, _) = ui
                .allocate_exact_size(Vec2::new(ui.available_width(), KEY_H), egui::Sense::hover());
            busy(ui, rect, Some(0.5), text, READOUT);
        });
    }
    done();
}

#[cfg(feature = "markdown")]
#[test]
fn markdown_wraps() {
    use egui_bench::markdown;
    let cases = [
        format!("{LONG}\n\n{WORD}"),
        format!("# {LONG}\n\n## {WORD}"),
        format!("- {LONG}\n- {WORD}\n  - {LONG} {WORD}\n\n1. {LONG}\n2. {WORD}"),
        format!("> {LONG}\n>\n> {WORD}\n\n> > {LONG}"),
        format!("| a | b |\n|---|---|\n| {LONG} | {WORD} |\n\n{LONG}\n\n- {LONG}"),
        format!("```rust\n{LONG}\n```\n\n{LONG}"),
        format!("[{LONG}]({WORD}) **{LONG}** `{WORD}` ~~{LONG}~~ *{LONG}*"),
        format!("{WORD}"),
    ];
    for (n, text) in cases.iter().enumerate() {
        fits(&format!("markdown case {n}"), |ui| {
            let mut cache = markdown::Cache::default();
            markdown::show(ui, &mut cache, text, n);
        });
    }
    done();
}

#[test]
fn a_choice_has_an_accessible_name() {
    let mut picked = 1u8;
    let mut harness =
        egui_kittest::Harness::builder().with_size(Vec2::new(300.0, 120.0)).build_ui(move |ui| {
            install(ui.ctx());
            choice(
                ui,
                "scope_dev",
                &mut picked,
                vec![(0u8, "first".to_string()), (1, "second".to_string())],
            );
        });
    harness.run();
}

#[test]
fn a_secret_field_has_an_accessible_name() {
    let mut text = String::new();
    let mut harness =
        egui_kittest::Harness::builder().with_size(Vec2::new(300.0, 120.0)).build_ui(move |ui| {
            install(ui.ctx());
            secret(ui, &mut text);
        });
    harness.run();
}
