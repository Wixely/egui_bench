use crate::form::{field, modal_title};
use crate::panel::toggle;
use crate::text::{Line, hint};
use crate::theme::{LEGEND, VALUE};
use egui::{RichText, Ui};
use std::path::{Path, PathBuf};

pub enum Picked {
    Browsing,
    Cancelled,
    File(PathBuf),
    Folder(PathBuf),
}

pub struct Browser {
    pub dir: PathBuf,
    typed: String,
    title: String,
    suffixes: Vec<String>,
    folders: bool,
}

impl Browser {
    pub fn new(title: &str, dir: impl Into<PathBuf>) -> Browser {
        let dir = dir.into();
        Browser {
            dir: std::fs::canonicalize(&dir).unwrap_or(dir),
            typed: String::new(),
            title: title.to_string(),
            suffixes: Vec::new(),
            folders: false,
        }
    }

    pub fn suffixes(mut self, suffixes: &[&str]) -> Browser {
        self.suffixes = suffixes.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn folders(mut self, pick: bool) -> Browser {
        self.folders = pick;
        self
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Picked {
        let mut picked = Picked::Browsing;
        let modal =
            egui::Modal::new(egui::Id::unique(("egui_bench_browser", &self.title))).show(ctx, |ui| {
                ui.set_width(560.0_f32.min((ctx.content_rect().width() - 48.0).max(120.0)));
                picked = self.body(ui);
            });
        match picked {
            Picked::Browsing if modal.should_close() => Picked::Cancelled,
            picked => picked,
        }
    }

    fn body(&mut self, ui: &mut Ui) -> Picked {
        modal_title(ui, &self.title);
        Line::new().legend("folder").value(self.dir.display().to_string()).show(ui);
        let mut go = None;
        let mut picked = Picked::Browsing;
        let typed = field(ui, &mut self.typed, "path to a file or folder");
        if typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let target = self.dir.join(self.typed.trim());
            if target.is_dir() {
                go = Some(target);
            } else if target.is_file() {
                picked = Picked::File(target);
            }
        }
        ui.add_space(6.0);
        let entries = entries(&self.dir, &self.suffixes);
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(320.0).show(ui, |ui| {
            if let Some(parent) = self.dir.parent()
                && ui.selectable_label(false, RichText::new("..").monospace()).clicked()
            {
                go = Some(parent.to_path_buf());
            }
            for (is_dir, path) in &entries {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                let (text, colour) =
                    if *is_dir { (format!("{name}/"), LEGEND) } else { (name.to_string(), VALUE) };
                if ui
                    .selectable_label(false, RichText::new(text).monospace().color(colour))
                    .clicked()
                {
                    if *is_dir {
                        go = Some(path.clone());
                    } else {
                        picked = Picked::File(path.clone());
                    }
                }
            }
            if entries.is_empty() {
                hint(ui, "nothing to open here");
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if self.folders && toggle(ui, "open this folder", false).clicked() {
                picked = Picked::Folder(self.dir.clone());
            }
            if toggle(ui, "cancel", false).clicked() {
                picked = Picked::Cancelled;
            }
        });
        if let Some(dir) = go {
            self.dir = std::fs::canonicalize(&dir).unwrap_or(dir);
            self.typed.clear();
        }
        picked
    }
}

pub fn entries(dir: &Path, suffixes: &[String]) -> Vec<(bool, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(bool, PathBuf)> = read
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?;
            if name.starts_with('.') {
                return None;
            }
            if p.is_dir() {
                return Some((true, p));
            }
            let shown = suffixes.is_empty() || suffixes.iter().any(|s| name.ends_with(s.as_str()));
            shown.then_some((false, p))
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunFrame;
    use egui::{Pos2, Rect, Vec2};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("egui_bench_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.pcb.toml"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        dir
    }

    fn frame(
        ctx: &egui::Context,
        b: &mut Browser,
        events: Vec<egui::Event>,
    ) -> (Picked, Vec<(String, Rect)>) {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0));
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut picked = Picked::Browsing;
        let out = ctx.run_frame(input, |ui| picked = b.show(ui.ctx()));
        let texts = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => {
                    Some((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2())))
                }
                _ => None,
            })
            .collect();
        (picked, texts)
    }

    fn click(at: Pos2, pressed: bool) -> Vec<egui::Event> {
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

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        ctx
    }

    fn pick(b: &mut Browser, label: &str) -> Picked {
        let ctx = context();
        let (_, texts) = frame(&ctx, b, vec![]);
        let (_, texts) = frame(&ctx, b, vec![]);
        let at = texts
            .iter()
            .find(|(t, _)| t == label)
            .unwrap_or_else(|| panic!("{label} not drawn: {texts:?}"))
            .1
            .center();
        let _ = frame(&ctx, b, click(at, true));
        frame(&ctx, b, click(at, false)).0
    }

    #[test]
    fn clicking_a_file_picks_it_and_a_folder_walks_into_it() {
        let dir = scratch("pick");
        let mut b = Browser::new("open", &dir).suffixes(&[".pcb.toml"]).folders(true);
        assert!(matches!(pick(&mut b, "a.pcb.toml"), Picked::File(p) if p.ends_with("a.pcb.toml")));
        assert!(matches!(pick(&mut b, "sub/"), Picked::Browsing));
        assert!(b.dir.ends_with("sub"));
        assert!(
            matches!(pick(&mut b, "OPEN THIS FOLDER"), Picked::Folder(p) if p.ends_with("sub"))
        );
        assert!(matches!(pick(&mut b, "CANCEL"), Picked::Cancelled));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn escape_cancels() {
        let dir = scratch("escape");
        let mut b = Browser::new("open", &dir);
        let ctx = context();
        let _ = frame(&ctx, &mut b, vec![]);
        let key = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert!(matches!(frame(&ctx, &mut b, vec![key]).0, Picked::Cancelled));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn folders_first_then_matching_files() {
        let dir = std::env::temp_dir().join(format!("egui_bench_browser_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        for f in ["b.sym.toml", "a.pcb.toml", "notes.txt", ".x.sym.toml"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let names: Vec<String> = entries(&dir, &[".sym.toml".into(), ".pcb.toml".into()])
            .iter()
            .map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["sub", "a.pcb.toml", "b.sym.toml"]);
        assert_eq!(entries(&dir, &[]).len(), 4);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
