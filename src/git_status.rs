use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use git2::{Repository, Status, StatusOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitState {
    Clean,
    Ignored,
    Untracked,
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Default, Clone)]
pub struct GitSnapshot {
    file_states: HashMap<PathBuf, GitState>,
    dir_states: HashMap<PathBuf, GitState>,
}

impl GitSnapshot {
    pub fn collect(startup_root: &Path) -> Self {
        let repo = match Repository::discover(startup_root) {
            Ok(repo) => repo,
            Err(_) => return Self::default(),
        };

        let workdir = match repo.workdir() {
            Some(w) => w.to_path_buf(),
            None => return Self::default(),
        };

        let mut options = StatusOptions::new();
        options
            .include_untracked(true)
            .include_ignored(false)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true)
            .include_unmodified(false)
            .recurse_untracked_dirs(true);

        let statuses = match repo.statuses(Some(&mut options)) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };

        let mut snapshot = Self::default();

        for entry in statuses.iter() {
            let status = entry.status();
            let Some(path) = entry.path() else {
                continue;
            };
            let full_path = workdir.join(path);

            if !full_path.starts_with(startup_root) {
                continue;
            }

            let state = map_status(status);
            snapshot.insert_file_state(full_path, state, startup_root);
        }

        snapshot
    }

    pub fn state_for(&self, path: &Path, is_dir: bool) -> GitState {
        if is_dir {
            self.dir_states
                .get(path)
                .copied()
                .unwrap_or(GitState::Clean)
        } else {
            self.file_states
                .get(path)
                .copied()
                .unwrap_or(GitState::Clean)
        }
    }

    fn insert_file_state(&mut self, path: PathBuf, state: GitState, startup_root: &Path) {
        let existing = self
            .file_states
            .get(&path)
            .copied()
            .unwrap_or(GitState::Clean);
        self.file_states
            .insert(path.clone(), combine_state(existing, state));

        let mut cursor = path.parent();
        while let Some(parent) = cursor {
            if !parent.starts_with(startup_root) {
                break;
            }
            let existing_dir = self
                .dir_states
                .get(parent)
                .copied()
                .unwrap_or(GitState::Clean);
            self.dir_states
                .insert(parent.to_path_buf(), combine_state(existing_dir, state));

            if parent == startup_root {
                break;
            }
            cursor = parent.parent();
        }
    }
}

pub fn collect_ignored_paths<'a, I>(startup_root: &Path, paths: I) -> HashSet<PathBuf>
where
    I: IntoIterator<Item = &'a Path>,
{
    let repo = match Repository::discover(startup_root) {
        Ok(repo) => repo,
        Err(_) => return HashSet::new(),
    };

    let workdir = match repo.workdir() {
        Some(w) => w,
        None => return HashSet::new(),
    };
    let canonical_workdir = fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
    let canonical_startup_root =
        fs::canonicalize(startup_root).unwrap_or_else(|_| startup_root.to_path_buf());

    let mut ignored = HashSet::new();

    for path in paths {
        let canonical_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

        if !canonical_path.starts_with(&canonical_startup_root) {
            continue;
        }

        let Ok(relative) = canonical_path.strip_prefix(&canonical_workdir) else {
            continue;
        };

        if path_is_ignored(&repo, relative, canonical_path.is_dir()) {
            ignored.insert(path.to_path_buf());
        }
    }

    ignored
}

fn path_is_ignored(repo: &Repository, relative: &Path, is_dir: bool) -> bool {
    if repo.is_path_ignored(relative).unwrap_or(false) {
        return true;
    }

    if !is_dir {
        return false;
    }

    let mut with_slash = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if with_slash.is_empty() {
        return false;
    }
    with_slash.push('/');

    repo.is_path_ignored(Path::new(&with_slash)).unwrap_or(false)
}

fn map_status(status: Status) -> GitState {
    if status.contains(Status::IGNORED) {
        return GitState::Ignored;
    }

    if status.contains(Status::WT_DELETED) || status.contains(Status::INDEX_DELETED) {
        return GitState::Deleted;
    }

    if status.contains(Status::WT_MODIFIED)
        || status.contains(Status::INDEX_MODIFIED)
        || status.contains(Status::WT_RENAMED)
        || status.contains(Status::INDEX_RENAMED)
        || status.contains(Status::CONFLICTED)
        || status.contains(Status::WT_TYPECHANGE)
        || status.contains(Status::INDEX_TYPECHANGE)
    {
        return GitState::Modified;
    }

    if status.contains(Status::INDEX_NEW) {
        return GitState::Added;
    }

    if status.contains(Status::WT_NEW) {
        return GitState::Untracked;
    }

    GitState::Clean
}

fn combine_state(left: GitState, right: GitState) -> GitState {
    if rank(right) > rank(left) {
        right
    } else {
        left
    }
}

fn rank(state: GitState) -> u8 {
    match state {
        GitState::Clean => 0,
        GitState::Ignored => 1,
        GitState::Untracked => 2,
        GitState::Added => 3,
        GitState::Modified => 4,
        GitState::Deleted => 5,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use git2::Repository;
    use tempfile::tempdir;

    use super::{collect_ignored_paths, combine_state, GitState};

    #[test]
    fn combine_prefers_stronger_state() {
        assert_eq!(
            combine_state(GitState::Untracked, GitState::Modified),
            GitState::Modified
        );
        assert_eq!(
            combine_state(GitState::Ignored, GitState::Untracked),
            GitState::Untracked
        );
        assert_eq!(
            combine_state(GitState::Added, GitState::Deleted),
            GitState::Deleted
        );
        assert_eq!(
            combine_state(GitState::Deleted, GitState::Added),
            GitState::Deleted
        );
    }

    #[test]
    fn collect_ignored_paths_marks_ignored_file_and_directory() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        Repository::init(root).expect("git init should succeed");

        fs::write(root.join(".gitignore"), "ignored-dir/\nignored.txt\n")
            .expect("gitignore should write");
        fs::create_dir_all(root.join("ignored-dir")).expect("ignored dir should create");
        fs::write(root.join("ignored-dir/file.log"), "skip").expect("ignored file should write");
        fs::write(root.join("ignored.txt"), "skip").expect("ignored file should write");
        fs::write(root.join("tracked.txt"), "keep").expect("tracked file should write");

        let ignored = collect_ignored_paths(
            root,
            [
                root.join("ignored-dir"),
                root.join("ignored.txt"),
                root.join("tracked.txt"),
            ]
            .iter()
            .map(PathBuf::as_path),
        );

        assert!(ignored.contains(&root.join("ignored-dir")));
        assert!(ignored.contains(&root.join("ignored.txt")));
        assert!(!ignored.contains(&root.join("tracked.txt")));
    }
}
