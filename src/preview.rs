use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use git2::{DiffFlags, DiffFormat, DiffOptions, Repository};

const MAX_PREVIEW_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy)]
pub enum PreviewKind {
    Text,
    Directory,
    Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRenderMode {
    Raw,
    Diff,
}

impl PreviewRenderMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Diff => "diff",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreviewState {
    pub kind: PreviewKind,
    pub render_mode: PreviewRenderMode,
    pub lines: Vec<String>,
    pub directory_entries: Vec<PreviewDirectoryEntry>,
    pub scroll: usize,
    available_modes: Vec<PreviewRenderMode>,
    changed_lines: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewDirectoryEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
}

impl PreviewState {
    pub fn from_path(
        startup_root: &Path,
        path: &Path,
        preferred_mode: Option<PreviewRenderMode>,
    ) -> Self {
        if path.is_dir() {
            return match load_directory_listing(startup_root, path) {
                Ok(lines) => Self::directory(lines),
                Err(msg) => Self::message(msg),
            };
        }

        let raw_lines = match load_raw_file(path) {
            Ok(lines) => lines,
            Err(msg) => return Self::message(msg),
        };

        let diff_view = collect_diff_view(startup_root, path, raw_lines.len());

        let mut available_modes = vec![PreviewRenderMode::Raw];
        if diff_view.is_some() {
            available_modes.push(PreviewRenderMode::Diff);
        }

        let default_mode = if diff_view.is_some() {
            PreviewRenderMode::Diff
        } else {
            PreviewRenderMode::Raw
        };

        let render_mode = preferred_mode
            .filter(|mode| available_modes.contains(mode))
            .unwrap_or(default_mode);

        let changed_lines = diff_view
            .as_ref()
            .map(|view| view.changed_lines.clone())
            .unwrap_or_default();

        let lines = match render_mode {
            // Diff mode shows the full file and highlights changed lines.
            PreviewRenderMode::Diff => raw_lines.clone(),
            PreviewRenderMode::Raw => raw_lines,
        };

        Self {
            kind: PreviewKind::Text,
            render_mode,
            lines,
            directory_entries: Vec::new(),
            scroll: 0,
            available_modes,
            changed_lines,
        }
    }

    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            kind: PreviewKind::Message,
            render_mode: PreviewRenderMode::Raw,
            lines: vec![msg.into()],
            directory_entries: Vec::new(),
            scroll: 0,
            available_modes: vec![PreviewRenderMode::Raw],
            changed_lines: Vec::new(),
        }
    }

    pub fn directory(entries: Vec<PreviewDirectoryEntry>) -> Self {
        Self {
            kind: PreviewKind::Directory,
            render_mode: PreviewRenderMode::Raw,
            lines: entries
                .iter()
                .map(|entry| {
                    if entry.is_dir {
                        format!("{}/", entry.name)
                    } else if entry.is_symlink {
                        format!("{}@", entry.name)
                    } else {
                        entry.name.clone()
                    }
                })
                .collect(),
            directory_entries: entries,
            scroll: 0,
            available_modes: vec![PreviewRenderMode::Raw],
            changed_lines: Vec::new(),
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    pub fn jump_to_next_change(&mut self) -> bool {
        if self.render_mode != PreviewRenderMode::Diff || self.changed_lines.is_empty() {
            return false;
        }

        let next = self
            .changed_lines
            .iter()
            .copied()
            .find(|line| *line > self.scroll)
            .unwrap_or(self.changed_lines[0]);
        self.scroll = next;
        true
    }

    pub fn jump_to_prev_change(&mut self) -> bool {
        if self.render_mode != PreviewRenderMode::Diff || self.changed_lines.is_empty() {
            return false;
        }

        let prev = self
            .changed_lines
            .iter()
            .copied()
            .rev()
            .find(|line| *line < self.scroll)
            .unwrap_or(*self.changed_lines.last().expect("non-empty"));
        self.scroll = prev;
        true
    }

    pub fn is_changed_line(&self, line_index: usize) -> bool {
        self.changed_lines.binary_search(&line_index).is_ok()
    }

    pub fn next_render_mode(&self) -> Option<PreviewRenderMode> {
        if self.available_modes.len() <= 1 {
            return None;
        }

        let index = self
            .available_modes
            .iter()
            .position(|mode| *mode == self.render_mode)?;
        let next_index = (index + 1) % self.available_modes.len();
        Some(self.available_modes[next_index])
    }

    pub fn mode_label(&self) -> &'static str {
        self.render_mode.label()
    }
}

#[derive(Debug, Clone)]
struct DiffView {
    changed_lines: Vec<usize>,
}

#[derive(Debug)]
struct DirectoryEntry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    is_symlink: bool,
}

fn load_directory_listing(
    startup_root: &Path,
    path: &Path,
) -> Result<Vec<PreviewDirectoryEntry>, String> {
    let read_dir = fs::read_dir(path).map_err(|err| format!("preview read failed: {err}"))?;
    let mut entries = Vec::new();

    for entry_res in read_dir {
        let entry = match entry_res {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let entry_path = entry.path();
        if !entry_path.starts_with(startup_root) {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        entries.push(DirectoryEntry {
            path: entry_path,
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir: file_type.is_dir(),
            is_symlink: file_type.is_symlink(),
        });
    }

    entries.sort_by(compare_directory_entries);

    Ok(entries
        .into_iter()
        .map(|entry| PreviewDirectoryEntry {
            path: entry.path,
            name: entry.name,
            is_dir: entry.is_dir,
            is_symlink: entry.is_symlink,
        })
        .collect())
}

fn compare_directory_entries(a: &DirectoryEntry, b: &DirectoryEntry) -> std::cmp::Ordering {
    match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a
            .name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.path.cmp(&b.path)),
    }
}

fn load_raw_file(path: &Path) -> Result<Vec<String>, String> {
    let metadata = std::fs::metadata(path).map_err(|err| format!("preview read failed: {err}"))?;

    if metadata.len() > MAX_PREVIEW_BYTES {
        return Err(format!("file too large (> {} bytes)", MAX_PREVIEW_BYTES));
    }

    let bytes = std::fs::read(path).map_err(|err| format!("preview read failed: {err}"))?;

    if bytes.contains(&0) {
        return Err(String::from("Binary or non-UTF-8 text is not previewable"));
    }

    let text = std::str::from_utf8(&bytes)
        .map_err(|_| String::from("Binary or non-UTF-8 text is not previewable"))?;

    Ok(text.lines().map(std::string::ToString::to_string).collect())
}

fn collect_diff_view(startup_root: &Path, path: &Path, line_count: usize) -> Option<DiffView> {
    let repo = Repository::discover(startup_root).ok()?;
    let workdir = repo.workdir()?;
    let relative_path = relative_to_workdir(workdir, path)?;

    let mut options = DiffOptions::new();
    options
        .pathspec(relative_path)
        .include_untracked(true)
        .recurse_untracked_dirs(true);

    let head_tree = repo.head().ok().and_then(|head| head.peel_to_tree().ok());

    let diff = repo
        .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut options))
        .ok()?;

    let mut has_delta = false;
    for delta in diff.deltas() {
        has_delta = true;
        if delta.flags().contains(DiffFlags::BINARY) {
            return None;
        }
    }

    if !has_delta {
        return None;
    }

    let mut non_utf8 = false;
    let print_ok = diff
        .print(DiffFormat::Patch, |_delta, _hunk, line| {
            if std::str::from_utf8(line.content()).is_err() {
                non_utf8 = true;
                return false;
            }
            true
        })
        .is_ok();

    if !print_ok || non_utf8 {
        return None;
    }

    let mut changed = BTreeSet::new();
    let mut file_cb = |_delta: git2::DiffDelta<'_>, _progress: f32| true;
    let mut line_cb = |_delta: git2::DiffDelta<'_>,
                       _hunk: Option<git2::DiffHunk<'_>>,
                       line: git2::DiffLine<'_>| {
        mark_changed_line(&mut changed, line, line_count);
        true
    };

    if diff
        .foreach(&mut file_cb, None, None, Some(&mut line_cb))
        .is_err()
    {
        return None;
    }

    Some(DiffView {
        changed_lines: changed.into_iter().collect(),
    })
}

fn mark_changed_line(changed: &mut BTreeSet<usize>, line: git2::DiffLine<'_>, line_count: usize) {
    if line_count == 0 {
        return;
    }

    match line.origin() {
        '+' | '>' => {
            if let Some(new_lineno) = line.new_lineno() {
                let index = usize::min(new_lineno.saturating_sub(1) as usize, line_count - 1);
                changed.insert(index);
            }
        }
        '-' | '<' => {
            if let Some(old_lineno) = line.old_lineno() {
                let index = usize::min(old_lineno.saturating_sub(1) as usize, line_count - 1);
                changed.insert(index);
            }
        }
        _ => {}
    }
}

fn relative_to_workdir(workdir: &Path, path: &Path) -> Option<PathBuf> {
    if let Ok(rel) = path.strip_prefix(workdir) {
        return Some(rel.to_path_buf());
    }

    let canonical_workdir = workdir.canonicalize().ok()?;

    if let Ok(canonical_path) = path.canonicalize() {
        if let Ok(rel) = canonical_path.strip_prefix(&canonical_workdir) {
            return Some(rel.to_path_buf());
        }
    }

    // Deleted/renamed targets may not exist anymore; resolve parent and rebuild.
    let canonical_parent = path.parent()?.canonicalize().ok()?;
    let file_name = path.file_name()?;
    let rel_parent = canonical_parent.strip_prefix(&canonical_workdir).ok()?;
    Some(rel_parent.join(file_name))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use git2::{IndexAddOption, Repository, Signature};
    use tempfile::tempdir;

    use super::{PreviewKind, PreviewRenderMode, PreviewState};

    #[test]
    fn preview_directory_returns_listing() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::create_dir(tmp.path().join("b_dir")).expect("dir should be created");
        fs::create_dir(tmp.path().join("a_dir")).expect("dir should be created");
        fs::write(tmp.path().join("z_file.txt"), "z").expect("file should be created");
        fs::write(tmp.path().join("m_file.txt"), "m").expect("file should be created");

        let preview = PreviewState::from_path(tmp.path(), tmp.path(), None);
        assert!(matches!(preview.kind, PreviewKind::Directory));
        assert_eq!(
            preview.lines,
            vec![
                "a_dir/".to_string(),
                "b_dir/".to_string(),
                "m_file.txt".to_string(),
                "z_file.txt".to_string()
            ]
        );
        assert_eq!(preview.directory_entries.len(), 4);
        assert!(preview.directory_entries[0].is_dir);
        assert_eq!(preview.directory_entries[0].name, "a_dir");
        assert_eq!(preview.directory_entries[2].name, "m_file.txt");
    }

    #[test]
    fn preview_empty_directory_returns_empty_listing() {
        let tmp = tempdir().expect("tmpdir should exist");

        let preview = PreviewState::from_path(tmp.path(), tmp.path(), None);
        assert!(matches!(preview.kind, PreviewKind::Directory));
        assert!(preview.lines.is_empty());
        assert!(preview.directory_entries.is_empty());
    }

    #[test]
    fn preview_non_git_text_uses_raw_mode() {
        let tmp = tempdir().expect("tmpdir should exist");
        let path = tmp.path().join("file.txt");
        fs::write(&path, "text").expect("write should succeed");

        let preview = PreviewState::from_path(tmp.path(), &path, None);
        assert!(matches!(preview.kind, PreviewKind::Text));
        assert_eq!(preview.render_mode, PreviewRenderMode::Raw);
        assert_eq!(preview.lines, vec!["text".to_string()]);
    }

    #[test]
    fn preview_no_diff_falls_back_to_raw_file() {
        let tmp = tempdir().expect("tmpdir should exist");
        let repo = Repository::init(tmp.path()).expect("git init should succeed");
        let path = tmp.path().join("file.txt");
        fs::write(&path, "line1\n").expect("write should succeed");
        commit_all(&repo, "initial");

        let preview = PreviewState::from_path(tmp.path(), &path, None);
        assert!(matches!(preview.kind, PreviewKind::Text));
        assert_eq!(preview.render_mode, PreviewRenderMode::Raw);
        assert_eq!(preview.lines, vec!["line1".to_string()]);
    }

    #[test]
    fn preview_modified_file_uses_full_file_in_diff_mode() {
        let tmp = tempdir().expect("tmpdir should exist");
        let repo = Repository::init(tmp.path()).expect("git init should succeed");
        let path = tmp.path().join("file.txt");
        fs::write(&path, "line1\n").expect("write should succeed");
        commit_all(&repo, "initial");
        fs::write(&path, "line1\nline2\n").expect("write should succeed");

        let preview = PreviewState::from_path(tmp.path(), &path, Some(PreviewRenderMode::Diff));
        assert!(matches!(preview.kind, PreviewKind::Text));
        assert_eq!(preview.render_mode, PreviewRenderMode::Diff);
        assert_eq!(
            preview.lines,
            vec!["line1".to_string(), "line2".to_string()]
        );
        assert!(preview.is_changed_line(1));
        assert!(!preview.is_changed_line(0));
    }

    #[test]
    fn raw_diff_cycle_when_diff_available() {
        let tmp = tempdir().expect("tmpdir should exist");
        let repo = Repository::init(tmp.path()).expect("git init should succeed");
        let path = tmp.path().join("file.txt");
        fs::write(&path, "line1\n").expect("write should succeed");
        commit_all(&repo, "initial");
        fs::write(&path, "line1\nline2\n").expect("write should succeed");

        let diff_preview = PreviewState::from_path(tmp.path(), &path, None);
        assert_eq!(diff_preview.render_mode, PreviewRenderMode::Diff);
        assert_eq!(
            diff_preview.next_render_mode(),
            Some(PreviewRenderMode::Raw)
        );

        let raw_preview = PreviewState::from_path(
            tmp.path(),
            &path,
            Some(
                diff_preview
                    .next_render_mode()
                    .expect("raw mode should exist"),
            ),
        );
        assert_eq!(raw_preview.render_mode, PreviewRenderMode::Raw);
        assert_eq!(
            raw_preview.next_render_mode(),
            Some(PreviewRenderMode::Diff)
        );
    }

    #[test]
    fn jump_change_moves_scroll_in_diff_mode() {
        let tmp = tempdir().expect("tmpdir should exist");
        let repo = Repository::init(tmp.path()).expect("git init should succeed");
        let path = tmp.path().join("file.txt");
        fs::write(&path, "line1\nline2\nline3\n").expect("write should succeed");
        commit_all(&repo, "initial");
        fs::write(&path, "line1\nline2 changed\nline3\n").expect("write should succeed");

        let mut preview = PreviewState::from_path(tmp.path(), &path, Some(PreviewRenderMode::Diff));
        preview.scroll = 0;
        assert!(preview.jump_to_next_change());
        assert_eq!(preview.scroll, 1);
        assert!(preview.jump_to_prev_change());
        assert_eq!(preview.scroll, 1);
    }

    #[test]
    fn preview_binary_returns_message() {
        let tmp = tempdir().expect("tmpdir should exist");
        let path = tmp.path().join("file.bin");
        fs::write(&path, vec![0xff, 0xfe, 0xfd]).expect("write should succeed");

        let preview = PreviewState::from_path(tmp.path(), &path, None);
        assert!(matches!(preview.kind, PreviewKind::Message));
    }

    fn commit_all(repo: &Repository, message: &str) {
        let mut index = repo.index().expect("index should open");
        index
            .add_all([Path::new(".")], IndexAddOption::DEFAULT, None)
            .expect("add_all should succeed");
        index.write().expect("index write should succeed");

        let tree_id = index.write_tree().expect("write_tree should succeed");
        let tree = repo.find_tree(tree_id).expect("tree should exist");

        let sig = Signature::now("test", "test@example.com").expect("signature should build");
        let parent_commit = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|oid| repo.find_commit(oid).ok());

        if let Some(parent) = parent_commit.as_ref() {
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[parent])
                .expect("commit should succeed");
        } else {
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
                .expect("commit should succeed");
        }
    }
}
