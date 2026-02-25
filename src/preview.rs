use std::path::{Path, PathBuf};

use git2::{DiffFlags, DiffFormat, DiffOptions, Repository};

const MAX_PREVIEW_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy)]
pub enum PreviewKind {
    Text,
    Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRenderMode {
    Diff,
    Plain,
}

#[derive(Debug, Clone)]
pub struct PreviewState {
    pub kind: PreviewKind,
    pub render_mode: PreviewRenderMode,
    pub lines: Vec<String>,
    pub scroll: usize,
}

impl PreviewState {
    pub fn from_path(startup_root: &Path, path: &Path) -> Self {
        if path.is_dir() {
            return Self::message("directory");
        }

        if path.is_file() && !is_probably_text(path) {
            return Self::message("Binary or non-UTF-8 diff preview is not supported");
        }

        let repo = match Repository::discover(startup_root) {
            Ok(repo) => repo,
            Err(_) => return Self::message("not a git repository"),
        };

        let workdir = match repo.workdir() {
            Some(w) => w,
            None => return Self::message("unsupported repository type"),
        };

        let relative_path = match relative_to_workdir(workdir, path) {
            Some(rel) => rel,
            None => return Self::message("path is outside git workdir"),
        };

        let mut options = DiffOptions::new();
        options
            .pathspec(relative_path)
            .include_untracked(true)
            .recurse_untracked_dirs(true);

        let head_tree = match repo.head() {
            Ok(head) => head.peel_to_tree().ok(),
            Err(_) => None,
        };

        let diff = match repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut options)) {
            Ok(diff) => diff,
            Err(err) => return Self::message(format!("diff error: {err}")),
        };

        let mut has_binary = false;
        for delta in diff.deltas() {
            if delta.flags().contains(DiffFlags::BINARY) {
                has_binary = true;
                break;
            }
        }

        if has_binary {
            return Self::message("Binary or non-UTF-8 diff preview is not supported");
        }

        let mut non_utf8 = false;
        let mut lines = Vec::new();
        let print_result = diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
            let content = line.content();
            let text = match std::str::from_utf8(content) {
                Ok(t) => t.trim_end_matches('\n').trim_end_matches('\r').to_string(),
                Err(_) => {
                    non_utf8 = true;
                    return false;
                }
            };
            if let Some(renderable) = to_renderable_diff_line(line.origin(), &text) {
                lines.push(renderable);
            }
            true
        });

        if let Err(err) = print_result {
            return Self::message(format!("diff render error: {err}"));
        }

        if non_utf8 {
            return Self::message("Binary or non-UTF-8 diff preview is not supported");
        }

        if lines.is_empty() {
            return Self::plain_file(path);
        }

        Self {
            kind: PreviewKind::Text,
            render_mode: PreviewRenderMode::Diff,
            lines,
            scroll: 0,
        }
    }

    fn plain_file(path: &Path) -> Self {
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(err) => return Self::message(format!("preview read failed: {err}")),
        };

        if metadata.len() > MAX_PREVIEW_BYTES {
            return Self::message(format!("file too large (> {} bytes)", MAX_PREVIEW_BYTES));
        }

        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(err) => return Self::message(format!("preview read failed: {err}")),
        };

        if bytes.contains(&0) {
            return Self::message("Binary or non-UTF-8 text is not previewable");
        }

        let text = match std::str::from_utf8(&bytes) {
            Ok(t) => t,
            Err(_) => return Self::message("Binary or non-UTF-8 text is not previewable"),
        };

        let lines = text
            .lines()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();

        Self {
            kind: PreviewKind::Text,
            render_mode: PreviewRenderMode::Plain,
            lines,
            scroll: 0,
        }
    }

    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            kind: PreviewKind::Message,
            render_mode: PreviewRenderMode::Plain,
            lines: vec![msg.into()],
            scroll: 0,
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    pub fn is_diff_view(&self) -> bool {
        self.render_mode == PreviewRenderMode::Diff
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

fn is_probably_text(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return true;
    };

    if metadata.len() > MAX_PREVIEW_BYTES {
        return false;
    }

    let Ok(bytes) = std::fs::read(path) else {
        return true;
    };

    if bytes.contains(&0) {
        return false;
    }
    std::str::from_utf8(&bytes).is_ok()
}

fn to_renderable_diff_line(origin: char, line: &str) -> Option<String> {
    match origin {
        '+' => Some(format!("+{line}")),
        '-' => Some(format!("-{line}")),
        ' ' => Some(format!(" {line}")),
        '\\' => Some(format!("\\{line}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use git2::{IndexAddOption, Repository, Signature};
    use tempfile::tempdir;

    use super::{PreviewKind, PreviewState};

    #[test]
    fn preview_directory_returns_message() {
        let tmp = tempdir().expect("tmpdir should exist");
        let preview = PreviewState::from_path(tmp.path(), tmp.path());
        assert!(matches!(preview.kind, PreviewKind::Message));
    }

    #[test]
    fn preview_non_git_returns_message() {
        let tmp = tempdir().expect("tmpdir should exist");
        let path = tmp.path().join("file.txt");
        fs::write(&path, "text").expect("write should succeed");

        let preview = PreviewState::from_path(tmp.path(), &path);
        assert!(matches!(preview.kind, PreviewKind::Message));
    }

    #[test]
    fn preview_no_diff_falls_back_to_plain_file() {
        let tmp = tempdir().expect("tmpdir should exist");
        let repo = Repository::init(tmp.path()).expect("git init should succeed");
        let path = tmp.path().join("file.txt");
        fs::write(&path, "line1\n").expect("write should succeed");
        commit_all(&repo, "initial");

        let preview = PreviewState::from_path(tmp.path(), &path);
        assert!(matches!(preview.kind, PreviewKind::Text));
        assert_eq!(preview.lines, vec!["line1".to_string()]);
    }

    #[test]
    fn preview_modified_file_returns_patch() {
        let tmp = tempdir().expect("tmpdir should exist");
        let repo = Repository::init(tmp.path()).expect("git init should succeed");
        let path = tmp.path().join("file.txt");
        fs::write(&path, "line1\n").expect("write should succeed");
        commit_all(&repo, "initial");
        fs::write(&path, "line1\nline2\n").expect("write should succeed");

        let preview = PreviewState::from_path(tmp.path(), &path);
        assert!(matches!(preview.kind, PreviewKind::Text));
        assert!(preview.lines.iter().any(|line| line.starts_with("+line2")));
        assert!(!preview.lines.iter().any(|line| line.starts_with("@@")));
    }

    #[test]
    fn preview_binary_returns_message() {
        let tmp = tempdir().expect("tmpdir should exist");
        let repo = Repository::init(tmp.path()).expect("git init should succeed");
        let path = tmp.path().join("file.bin");
        fs::write(&path, b"text").expect("write should succeed");
        commit_all(&repo, "initial");
        fs::write(&path, vec![0xff, 0xfe, 0xfd]).expect("write should succeed");

        let preview = PreviewState::from_path(tmp.path(), &path);
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
