use std::collections::HashMap;
use std::path::{Path, PathBuf};

use git2::{Repository, Status, StatusOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitState {
    Clean,
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
            self.dir_states.get(path).copied().unwrap_or(GitState::Clean)
        } else {
            self.file_states.get(path).copied().unwrap_or(GitState::Clean)
        }
    }

    fn insert_file_state(&mut self, path: PathBuf, state: GitState, startup_root: &Path) {
        let existing = self.file_states.get(&path).copied().unwrap_or(GitState::Clean);
        self.file_states.insert(path.clone(), combine_state(existing, state));

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

fn map_status(status: Status) -> GitState {
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
        GitState::Untracked => 1,
        GitState::Added => 2,
        GitState::Modified => 3,
        GitState::Deleted => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::{combine_state, GitState};

    #[test]
    fn combine_prefers_stronger_state() {
        assert_eq!(combine_state(GitState::Untracked, GitState::Modified), GitState::Modified);
        assert_eq!(combine_state(GitState::Added, GitState::Deleted), GitState::Deleted);
        assert_eq!(combine_state(GitState::Deleted, GitState::Added), GitState::Deleted);
    }
}
