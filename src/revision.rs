use crate::form::{field, footer, modal_title};
use crate::panel::toggle;
use crate::text::hint;
use crate::theme::{LEGEND, READOUT, VALUE};
use egui::{RichText, Ui};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq)]
pub struct Revision {
    pub sha: String,
    pub short: String,
    pub date: String,
    pub author: String,
    pub refs: Vec<String>,
    pub subject: String,
}

impl Revision {
    pub fn label(&self) -> String {
        if self.refs.is_empty() {
            self.short.clone()
        } else {
            format!("{} ({})", self.short, self.refs.join(", "))
        }
    }

    fn matches(&self, needle: &str) -> bool {
        let needle = needle.trim().to_lowercase();
        needle.is_empty()
            || [&self.sha, &self.subject, &self.author, &self.date, &self.refs.join(" ")]
                .iter()
                .any(|s| s.to_lowercase().contains(&needle))
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn folder(path: &Path) -> PathBuf {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if path.is_dir() { path } else { path.parent().map(Path::to_path_buf).unwrap_or(path) }
}

fn toplevel(path: &Path) -> Result<PathBuf, String> {
    let top = git(&folder(path), &["rev-parse", "--show-toplevel"])?;
    let top = PathBuf::from(top.trim());
    Ok(std::fs::canonicalize(&top).unwrap_or(top))
}

pub fn list(path: &Path, limit: usize) -> Result<Vec<Revision>, String> {
    let n = format!("-n{limit}");
    let out = git(
        &folder(path),
        &[
            "log",
            &n,
            "--format=%H%x1f%h%x1f%cs%x1f%an%x1f%D%x1f%s%x1e",
            "--",
            ".",
        ],
    )?;
    Ok(out.split('\u{1e}').filter_map(parse).collect())
}

fn parse(record: &str) -> Option<Revision> {
    let mut f = record.trim_start_matches('\n').split('\u{1f}');
    let sha = f.next().filter(|s| !s.is_empty())?.to_string();
    let short = f.next()?.to_string();
    let date = f.next()?.to_string();
    let author = f.next()?.to_string();
    let refs = f
        .next()?
        .split(", ")
        .filter(|r| !r.is_empty())
        .map(|r| r.trim_start_matches("HEAD -> ").trim_start_matches("tag: ").to_string())
        .filter(|r| r != "HEAD")
        .collect();
    let subject = f.next()?.to_string();
    Some(Revision { sha, short, date, author, refs, subject })
}

pub fn snapshot_root(path: &Path, sha: &str) -> Result<PathBuf, String> {
    let top = toplevel(path)?;
    let full = git(&top, &["rev-parse", "--verify", &format!("{sha}^{{commit}}")])?;
    let full = full.trim();
    let tag = format!("{:016x}", fnv(top.to_string_lossy().as_bytes()));
    let dest = std::env::temp_dir().join("bench-revisions").join(format!("{tag}-{full}"));
    if dest.is_dir() {
        return Ok(dest);
    }
    let partial = dest.with_extension("partial");
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    let result = extract(&top, full, &partial);
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(e);
    }
    std::fs::rename(&partial, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

fn extract(top: &Path, sha: &str, into: &Path) -> Result<(), String> {
    let mut archive = Command::new("git")
        .arg("-C")
        .arg(top)
        .args(["archive", "--format=tar", sha])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    let tar = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(into)
        .stdin(archive.stdout.take().ok_or("git archive has no output")?)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("tar: {e}"))?;
    let git = archive.wait_with_output().map_err(|e| e.to_string())?;
    if !git.status.success() {
        return Err(String::from_utf8_lossy(&git.stderr).trim().to_string());
    }
    if !tar.status.success() {
        return Err(String::from_utf8_lossy(&tar.stderr).trim().to_string());
    }
    Ok(())
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

pub fn checkout(path: &Path, sha: &str) -> Result<PathBuf, String> {
    let top = toplevel(path)?;
    let root = snapshot_root(path, sha)?;
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let rel = path.strip_prefix(&top).map_err(|_| "outside the repository".to_string())?;
    let target = root.join(rel);
    if !target.exists() {
        return Err(format!("{} does not exist at {}", rel.display(), &sha[..sha.len().min(8)]));
    }
    Ok(target)
}

pub fn is_snapshot(path: &Path) -> bool {
    let base = std::fs::canonicalize(std::env::temp_dir().join("bench-revisions"));
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    base.is_ok_and(|b| path.starts_with(b))
}

pub enum Picked {
    Browsing,
    Cancelled,
    Revision(Revision),
}

pub struct Picker {
    revisions: Result<Vec<Revision>, String>,
    filter: String,
    title: String,
}

impl Picker {
    pub fn new(title: &str, path: &Path) -> Picker {
        Picker { revisions: list(path, 300), filter: String::new(), title: title.to_string() }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Picked {
        let mut picked = Picked::Browsing;
        let modal = egui::Modal::new(egui::Id::unique(("egui_bench_revisions", &self.title)))
            .show(ctx, |ui| {
                ui.set_width(640.0_f32.min((ctx.content_rect().width() - 48.0).max(120.0)));
                picked = self.body(ui);
            });
        match picked {
            Picked::Browsing if modal.should_close() => Picked::Cancelled,
            picked => picked,
        }
    }

    fn body(&mut self, ui: &mut Ui) -> Picked {
        modal_title(ui, &self.title);
        let mut picked = Picked::Browsing;
        match &self.revisions {
            Err(e) => hint(ui, &format!("no history here: {e}")),
            Ok(revisions) => {
                field(ui, &mut self.filter, "filter by message, tag, author or hash");
                ui.add_space(6.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(340.0).show(
                    ui,
                    |ui| {
                        for r in revisions.iter().filter(|r| r.matches(&self.filter)) {
                            let head = RichText::new(format!("{}  {}", r.label(), r.date))
                                .monospace()
                                .color(READOUT);
                            let text = RichText::new(&r.subject).color(VALUE);
                            let by = RichText::new(&r.author).small().color(LEGEND);
                            let mut job = egui::text::LayoutJob::default();
                            for (t, nl) in [(head, true), (text, true), (by, false)] {
                                t.append_to(
                                    &mut job,
                                    ui.style(),
                                    egui::FontSelection::Default,
                                    egui::Align::LEFT,
                                );
                                if nl {
                                    job.append("\n", 0.0, Default::default());
                                }
                            }
                            if ui.selectable_label(false, job).clicked() {
                                picked = Picked::Revision(r.clone());
                            }
                        }
                    },
                );
            }
        }
        footer(ui, |ui| {
            if toggle(ui, "cancel", false).clicked() {
                picked = Picked::Cancelled;
            }
        });
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init", "-q"]);
        std::fs::create_dir_all(dir.path().join("board")).unwrap();
        std::fs::write(dir.path().join("board/a.pcb.toml"), "v1").unwrap();
        run(dir.path(), &["add", "."]);
        run(dir.path(), &["commit", "-qm", "first order"]);
        run(dir.path(), &["tag", "order-1"]);
        std::fs::write(dir.path().join("board/a.pcb.toml"), "v2").unwrap();
        std::fs::write(dir.path().join("board/b.pcb.toml"), "new").unwrap();
        run(dir.path(), &["add", "."]);
        run(dir.path(), &["commit", "-qm", "rework"]);
        dir
    }

    #[test]
    fn lists_newest_first_with_tags() {
        let dir = repo();
        let revs = list(&dir.path().join("board"), 10).unwrap();
        assert_eq!(revs.len(), 2);
        assert_eq!(revs[0].subject, "rework");
        assert_eq!(revs[1].subject, "first order");
        assert!(revs[1].refs.contains(&"order-1".to_string()));
    }

    #[test]
    fn checkout_opens_the_old_file_without_touching_the_working_copy() {
        let dir = repo();
        let file = dir.path().join("board/a.pcb.toml");
        let revs = list(&dir.path().join("board"), 10).unwrap();
        let old = checkout(&file, &revs[1].sha).unwrap();
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "v1");
        assert!(!old.starts_with(dir.path()));
        assert!(is_snapshot(&old));
        assert!(!old.parent().unwrap().join("b.pcb.toml").exists());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2");
        assert!(!is_snapshot(&file));
    }

    #[test]
    fn a_file_missing_at_that_revision_is_reported() {
        let dir = repo();
        let revs = list(dir.path(), 10).unwrap();
        let err = checkout(&dir.path().join("board/b.pcb.toml"), &revs[1].sha).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }

    #[test]
    fn a_folder_with_no_repository_has_no_history() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list(dir.path(), 5).is_err());
    }

    #[test]
    fn filter_matches_tag_and_message() {
        let dir = repo();
        let revs = list(dir.path(), 10).unwrap();
        assert!(revs[1].matches("ORDER-1"));
        assert!(revs[0].matches("rework"));
        assert!(!revs[0].matches("order-1"));
    }
}
