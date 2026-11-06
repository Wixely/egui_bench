use crate::form::{field, footer, modal_title};
use crate::panel::toggle;
use crate::text::hint;
use crate::theme::{LEGEND, READOUT, VALUE};
use egui::{RichText, Ui};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

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

fn folder(path: &Path) -> PathBuf {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if path.is_dir() { path } else { path.parent().map(Path::to_path_buf).unwrap_or(path) }
}

fn open_repo(path: &Path) -> Result<gix::ThreadSafeRepository, String> {
    gix::ThreadSafeRepository::discover(folder(path)).map_err(|e| e.to_string())
}

fn top_of(repo: &gix::Repository) -> Result<PathBuf, String> {
    let top = repo.workdir().ok_or("a bare repository has no files to open")?;
    Ok(std::fs::canonicalize(top).unwrap_or_else(|_| top.to_path_buf()))
}

fn relative_to(top: &Path, path: &Path) -> Result<PathBuf, String> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.strip_prefix(top).map(Path::to_path_buf).map_err(|_| "outside the repository".to_string())
}

fn names_by_commit(repo: &gix::Repository) -> HashMap<gix::ObjectId, Vec<String>> {
    let mut out: HashMap<gix::ObjectId, Vec<String>> = HashMap::new();
    let Ok(platform) = repo.references() else { return out };
    let Ok(all) = platform.all() else { return out };
    for reference in all.flatten() {
        let mut reference = reference;
        let name = reference.name().shorten().to_string();
        if let Ok(id) = reference.peel_to_id() {
            out.entry(id.detach()).or_default().push(name);
        }
    }
    out
}

fn entry_at(tree: &gix::Tree<'_>, rel: &Path) -> Option<gix::ObjectId> {
    if rel.as_os_str().is_empty() {
        return Some(tree.id);
    }
    tree.clone().lookup_entry_by_path(rel).ok().flatten().map(|e| e.object_id())
}

pub fn list(path: &Path, limit: usize) -> Result<Vec<Revision>, String> {
    let repo = open_repo(path)?.to_thread_local();
    let top = top_of(&repo)?;
    let rel = relative_to(&top, &folder(path))?;
    let names = names_by_commit(&repo);
    let head = repo.head_id().map_err(|e| e.to_string())?;
    let walk = repo.rev_walk([head.detach()]).all().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for info in walk {
        let info = info.map_err(|e| e.to_string())?;
        let commit = info.object().map_err(|e| e.to_string())?;
        let tree = commit.tree().map_err(|e| e.to_string())?;
        let here = entry_at(&tree, &rel);
        let before = match commit.parent_ids().next() {
            Some(parent) => {
                let parent = repo.find_commit(parent).map_err(|e| e.to_string())?;
                entry_at(&parent.tree().map_err(|e| e.to_string())?, &rel)
            }
            None => None,
        };
        if here.is_none() || here == before {
            continue;
        }
        let id = commit.id;
        let sha = id.to_string();
        let message = commit.message().map_err(|e| e.to_string())?;
        let author = commit.author().map(|a| a.name.to_string()).unwrap_or_default();
        let date = commit
            .time()
            .map(|t| t.format(gix::date::time::format::SHORT).unwrap_or_default())
            .unwrap_or_default();
        let mut refs = names.get(&id).cloned().unwrap_or_default();
        refs.sort();
        out.push(Revision {
            short: sha[..7.min(sha.len())].to_string(),
            sha,
            date,
            author,
            refs,
            subject: message.title.to_string().trim().to_string(),
        });
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

pub struct Snapshot {
    repo: gix::ThreadSafeRepository,
    top: PathBuf,
    sha: String,
    files: BTreeMap<PathBuf, gix::ObjectId>,
    dirs: BTreeSet<PathBuf>,
    cache: Mutex<HashMap<PathBuf, Arc<[u8]>>>,
}

impl Snapshot {
    pub fn open(path: &Path, sha: &str) -> Result<Snapshot, String> {
        let safe = open_repo(path)?;
        let repo = safe.to_thread_local();
        let top = top_of(&repo)?;
        let id = repo
            .rev_parse_single(sha)
            .map_err(|e| e.to_string())?
            .object()
            .map_err(|e| e.to_string())?
            .peel_to_commit()
            .map_err(|e| e.to_string())?;
        let tree = id.tree().map_err(|e| e.to_string())?;
        let mut recorder = gix::traverse::tree::Recorder::default();
        tree.traverse().breadthfirst(&mut recorder).map_err(|e| e.to_string())?;
        let mut files = BTreeMap::new();
        let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
        dirs.insert(PathBuf::new());
        for entry in recorder.records {
            let rel = PathBuf::from(entry.filepath.to_string());
            if entry.mode.is_tree() {
                dirs.insert(rel);
            } else if entry.mode.is_blob() || entry.mode.is_link() {
                files.insert(rel, entry.oid);
            }
        }
        let snapshot = Snapshot {
            repo: safe,
            top,
            sha: id.id.to_string(),
            files,
            dirs,
            cache: Mutex::default(),
        };
        if !snapshot.is_file(path) && !snapshot.is_dir(path) {
            return Err(format!("{} does not exist at {}", path.display(), snapshot.short()));
        }
        Ok(snapshot)
    }

    pub fn short(&self) -> &str {
        &self.sha[..self.sha.len().min(8)]
    }

    pub fn top(&self) -> &Path {
        &self.top
    }

    fn relative(&self, path: &Path) -> Option<PathBuf> {
        let tidy = tidy(path);
        if let Ok(r) = tidy.strip_prefix(&self.top) {
            return Some(r.to_path_buf());
        }
        let real = std::fs::canonicalize(&tidy).ok().or_else(|| {
            let parent = std::fs::canonicalize(tidy.parent()?).ok()?;
            Some(parent.join(tidy.file_name()?))
        })?;
        real.strip_prefix(&self.top).ok().map(Path::to_path_buf)
    }

    pub fn covers(&self, path: &Path) -> bool {
        self.relative(path).is_some()
    }

    pub fn is_file(&self, path: &Path) -> bool {
        self.relative(path).is_some_and(|r| self.files.contains_key(&r))
    }

    pub fn is_dir(&self, path: &Path) -> bool {
        self.relative(path).is_some_and(|r| self.dirs.contains(&r))
    }

    pub fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        match self.relative(path) {
            Some(r) if self.files.contains_key(&r) || self.dirs.contains(&r) => {
                Ok(self.top.join(r))
            }
            _ => Err(io::Error::new(io::ErrorKind::NotFound, "no such file at this revision")),
        }
    }

    pub fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let missing = || io::Error::new(io::ErrorKind::NotFound, "no such file at this revision");
        let rel = self.relative(path).filter(|r| self.files.contains_key(r)).ok_or_else(missing)?;
        if let Some(bytes) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&rel) {
            return Ok(bytes.to_vec());
        }
        let id = self.files[&rel];
        let repo = self.repo.to_thread_local();
        let blob = repo.find_object(id).map_err(|_| missing())?;
        let bytes: Arc<[u8]> = blob.data.clone().into();
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(rel, bytes.clone());
        Ok(bytes.to_vec())
    }

    pub fn read_dir(&self, path: &Path) -> io::Result<Vec<(PathBuf, bool)>> {
        let rel = self
            .relative(path)
            .filter(|r| self.dirs.contains(r))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such folder"))?;
        let child = |p: &PathBuf| p.parent() == Some(rel.as_path()) && !p.as_os_str().is_empty();
        let mut out: Vec<(PathBuf, bool)> = self
            .dirs
            .iter()
            .filter(|p| child(p))
            .map(|p| (self.top.join(p), true))
            .chain(self.files.keys().filter(|p| child(p)).map(|p| (self.top.join(p), false)))
            .collect();
        out.sort();
        Ok(out)
    }
}

fn tidy(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

static ACTIVE: RwLock<Option<Arc<Snapshot>>> = RwLock::new(None);

pub fn set_active(snapshot: Option<Arc<Snapshot>>) {
    *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = snapshot;
}

pub fn active() -> Option<Arc<Snapshot>> {
    ACTIVE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn covering(path: &Path) -> Option<Arc<Snapshot>> {
    active().filter(|s| s.covers(path))
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
        let modal = egui::Modal::new(egui::Id::unique(("egui_bench_revisions", &self.title))).show(
            ctx,
            |ui| {
                ui.set_width(640.0_f32.min((ctx.content_rect().width() - 48.0).max(120.0)));
                picked = self.body(ui);
            },
        );
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
    use std::process::Command;

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
    fn snapshot_reads_the_old_file_from_memory_and_leaves_disk_alone() {
        let dir = repo();
        let file = dir.path().join("board/a.pcb.toml");
        let revs = list(&dir.path().join("board"), 10).unwrap();
        let snap = Snapshot::open(&file, &revs[1].sha).unwrap();
        assert_eq!(snap.read(&file).unwrap(), b"v1");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2");
        assert!(!snap.is_file(&dir.path().join("board/b.pcb.toml")));
        assert!(snap.read(&dir.path().join("board/b.pcb.toml")).is_err());
        assert!(snap.is_dir(&dir.path().join("board")));
        assert!(snap.covers(&dir.path().join("board/anything")));
        assert!(!snap.covers(Path::new("/elsewhere")));
    }

    #[test]
    fn snapshot_lists_folders_as_they_were() {
        let dir = repo();
        let revs = list(dir.path(), 10).unwrap();
        let old = Snapshot::open(dir.path(), &revs[1].sha).unwrap();
        let new = Snapshot::open(dir.path(), &revs[0].sha).unwrap();
        let board = dir.path().canonicalize().unwrap().join("board");
        assert_eq!(old.read_dir(&board).unwrap().len(), 1);
        assert_eq!(new.read_dir(&board).unwrap().len(), 2);
        assert_eq!(old.read_dir(dir.path()).unwrap(), vec![(board, true)]);
    }

    #[test]
    fn a_file_missing_at_that_revision_is_reported() {
        let dir = repo();
        let revs = list(dir.path(), 10).unwrap();
        let err = Snapshot::open(&dir.path().join("board/b.pcb.toml"), &revs[1].sha).err().unwrap();
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
