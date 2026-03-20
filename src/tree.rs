use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;

use crate::git_status::GitSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TreeMode {
    Normal,
    Changed,
}

impl TreeMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Changed => "changed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirEntryNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size_bytes: Option<u64>,
    pub modified_date: Option<String>,
}

#[derive(Debug)]
pub struct Tree {
    pub startup_root: PathBuf,
    pub current_dir: PathBuf,
    pub entries: Vec<DirEntryNode>,
    pub mode: TreeMode,
    selected: usize,
    changed_paths: HashSet<PathBuf>,
}

impl Tree {
    pub fn new(startup_root: PathBuf, mode: TreeMode, git: &GitSnapshot) -> anyhow::Result<Self> {
        let mut tree = Self {
            startup_root: startup_root.clone(),
            current_dir: startup_root,
            entries: Vec::new(),
            mode,
            selected: 0,
            changed_paths: collect_existing_changed_paths(git, mode),
        };
        tree.reload_entries(None)?;
        Ok(tree)
    }

    pub fn selected_path(&self) -> &Path {
        self.entries
            .get(self.selected)
            .map(|entry| entry.path.as_path())
            .unwrap_or(self.current_dir.as_path())
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    pub fn collapse_selected(&mut self) {
        if self.current_dir == self.startup_root {
            return;
        }

        let previous_dir = self.current_dir.clone();
        if let Some(parent) = self.current_dir.parent() {
            if parent.starts_with(&self.startup_root) {
                self.current_dir = parent.to_path_buf();
                if self.reload_entries(Some(&previous_dir)).is_err() {
                    self.entries.clear();
                    self.selected = 0;
                }
            }
        }
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let current_selected = self.selected_path().to_path_buf();
        self.reload_entries(Some(&current_selected))
    }

    pub fn set_mode(&mut self, mode: TreeMode, git: &GitSnapshot) -> anyhow::Result<()> {
        self.mode = mode;
        self.changed_paths = collect_existing_changed_paths(git, mode);
        let preferred = self.selected_path().to_path_buf();
        self.reload_entries(Some(&preferred))
    }

    pub fn update_changed_paths(&mut self, git: &GitSnapshot) -> anyhow::Result<()> {
        self.changed_paths = collect_existing_changed_paths(git, self.mode);
        let preferred = self.selected_path().to_path_buf();
        self.reload_entries(Some(&preferred))
    }

    pub fn expand_selected(&mut self) -> anyhow::Result<()> {
        let Some(selected) = self.entries.get(self.selected) else {
            return Ok(());
        };

        if !selected.is_dir {
            return Ok(());
        }

        self.current_dir = selected.path.clone();
        self.reload_entries(None)
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }

        self.selected = index;
        true
    }

    pub fn selected_is_dir(&self) -> bool {
        self.entries
            .get(self.selected)
            .map(|entry| entry.is_dir)
            .unwrap_or(false)
    }

    fn reload_entries(&mut self, prefer_selected_path: Option<&Path>) -> anyhow::Result<()> {
        loop {
            let read_dir = fs::read_dir(&self.current_dir)?;
            let mut entries = Vec::new();

            for entry_res in read_dir {
                let entry = match entry_res {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let path = entry.path();
                if !path.starts_with(&self.startup_root) {
                    continue;
                }

                let file_type = match entry.file_type() {
                    Ok(t) => t,
                    Err(_) => continue,
                };

                let is_dir = file_type.is_dir();
                if self.mode == TreeMode::Changed && !self.is_changed_visible(&path, is_dir) {
                    continue;
                }

                let name = entry.file_name().to_string_lossy().to_string();
                let (size_bytes, modified_date) = load_entry_metadata(&entry, is_dir);
                entries.push(DirEntryNode {
                    path,
                    name,
                    is_dir,
                    is_symlink: file_type.is_symlink(),
                    size_bytes,
                    modified_date,
                });
            }

            entries.sort_by(compare_entries);
            if self.mode == TreeMode::Changed
                && entries.is_empty()
                && self.current_dir != self.startup_root
            {
                if let Some(parent) = self.current_dir.parent() {
                    self.current_dir = parent.to_path_buf();
                    continue;
                }
            }

            self.entries = entries;
            self.selected = prefer_selected_path
                .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
                .unwrap_or(0);
            return Ok(());
        }
    }

    fn is_changed_visible(&self, path: &Path, is_dir: bool) -> bool {
        if is_dir {
            self.changed_paths
                .iter()
                .any(|changed| changed.starts_with(path))
        } else {
            self.changed_paths.contains(path)
        }
    }
}

fn collect_existing_changed_paths(git: &GitSnapshot, mode: TreeMode) -> HashSet<PathBuf> {
    if mode != TreeMode::Changed {
        return HashSet::new();
    }

    git.changed_file_paths()
        .into_iter()
        .filter(|path| path.exists())
        .collect()
}

fn compare_entries(a: &DirEntryNode, b: &DirEntryNode) -> Ordering {
    match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    }
}

fn load_entry_metadata(entry: &fs::DirEntry, is_dir: bool) -> (Option<u64>, Option<String>) {
    let metadata = match entry.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return (None, None),
    };

    let size_bytes = if is_dir { None } else { Some(metadata.len()) };
    let modified_date = metadata.modified().ok().and_then(format_system_time_date);
    (size_bytes, modified_date)
}

fn format_system_time_date(time: SystemTime) -> Option<String> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    let days = (duration.as_secs() / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    // 依存追加を避けるため、UNIX epoch から西暦日付へ変換する。
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = era * 400 + year_of_era + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use git2::{IndexAddOption, Repository, Signature};
    use tempfile::tempdir;

    use crate::git_status::GitSnapshot;

    use super::{Tree, TreeMode};

    #[test]
    fn tree_stays_within_startup_root() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dirs should work");
        fs::write(root.join("sub/file.txt"), "hello").expect("write file should work");

        let tree = Tree::new(root.clone(), TreeMode::Normal, &GitSnapshot::default())
            .expect("tree should build");

        for node in &tree.entries {
            assert!(node.path.starts_with(&root));
        }
    }

    #[test]
    fn cannot_collapse_above_startup_root() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dirs should work");

        let mut tree = Tree::new(root.clone(), TreeMode::Normal, &GitSnapshot::default())
            .expect("tree should build");
        tree.collapse_selected();

        assert_eq!(tree.current_dir, root);
    }

    #[test]
    fn collapse_restores_cursor_to_previous_directory() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        let a = root.join("a_dir");
        let b = root.join("b_dir");
        fs::create_dir_all(&a).expect("create a_dir should work");
        fs::create_dir_all(&b).expect("create b_dir should work");

        let mut tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");
        tree.move_down(); // b_dir を選択
        let selected_before = tree.selected_path().to_path_buf();
        tree.expand_selected().expect("expand should work");
        tree.collapse_selected();

        assert_eq!(tree.selected_path(), selected_before.as_path());
    }

    #[test]
    fn changed_mode_shows_changed_ancestors_only() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        let repo = Repository::init(root).expect("git init should succeed");
        fs::create_dir_all(root.join("src/nested")).expect("dirs should create");
        fs::write(root.join("src/nested/file.txt"), "v1").expect("file should write");
        fs::write(root.join("other.txt"), "clean").expect("file should write");
        commit_all(&repo, "initial");
        fs::write(root.join("src/nested/file.txt"), "v2").expect("file should update");

        let git = GitSnapshot::collect(root);
        let mut tree =
            Tree::new(root.to_path_buf(), TreeMode::Changed, &git).expect("tree should build");

        assert_eq!(tree.entries.len(), 1);
        assert_eq!(tree.entries[0].name, "src");
        tree.expand_selected().expect("expand should work");
        assert_eq!(tree.entries.len(), 1);
        assert_eq!(tree.entries[0].name, "nested");
        tree.expand_selected().expect("expand should work");
        assert_eq!(tree.entries.len(), 1);
        assert_eq!(tree.entries[0].name, "file.txt");
    }

    #[test]
    fn set_mode_keeps_current_directory_when_it_is_still_valid() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        let repo = Repository::init(root).expect("git init should succeed");
        fs::create_dir_all(root.join("src/nested")).expect("dirs should create");
        fs::write(root.join("src/nested/file.txt"), "v1").expect("file should write");
        commit_all(&repo, "initial");
        fs::write(root.join("src/nested/file.txt"), "v2").expect("file should update");

        let git = GitSnapshot::collect(root);
        let mut tree =
            Tree::new(root.to_path_buf(), TreeMode::Normal, &git).expect("tree should build");
        tree.current_dir = root.join("src");
        tree.refresh().expect("refresh should work");

        tree.set_mode(TreeMode::Changed, &git)
            .expect("mode switch should work");

        assert_eq!(tree.current_dir, root.join("src"));
        assert_eq!(tree.entries.len(), 1);
        assert_eq!(tree.entries[0].name, "nested");
    }

    #[test]
    fn changed_mode_excludes_deleted_entries_without_worktree_file() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        let repo = Repository::init(root).expect("git init should succeed");
        fs::write(root.join("gone.txt"), "v1").expect("file should write");
        commit_all(&repo, "initial");
        fs::remove_file(root.join("gone.txt")).expect("file should delete");

        let git = GitSnapshot::collect(root);
        let tree =
            Tree::new(root.to_path_buf(), TreeMode::Changed, &git).expect("tree should build");

        assert!(tree.entries.is_empty());
    }

    #[test]
    fn tree_collects_file_size_and_modified_date() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(&root).expect("create root should work");
        fs::write(root.join("note.txt"), "hello").expect("write file should work");

        let tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");
        let file = tree
            .entries
            .iter()
            .find(|entry| entry.name == "note.txt")
            .expect("note.txt should exist");

        assert_eq!(file.size_bytes, Some(5));
        assert_eq!(file.modified_date.as_deref().map(str::len), Some(10));
    }

    #[test]
    fn tree_uses_empty_size_for_directories() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dir should work");

        let tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");
        let dir = tree
            .entries
            .iter()
            .find(|entry| entry.name == "sub")
            .expect("sub should exist");

        assert_eq!(dir.size_bytes, None);
    }

    #[test]
    fn select_index_updates_selected_entry() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("a_dir")).expect("create dir should work");
        fs::create_dir_all(root.join("b_dir")).expect("create dir should work");

        let mut tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");

        assert!(tree.select_index(1));
        assert_eq!(tree.selected_index(), 1);
    }

    #[test]
    fn select_index_ignores_out_of_bounds() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dir should work");

        let mut tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");
        let before = tree.selected_index();

        assert!(!tree.select_index(99));
        assert_eq!(tree.selected_index(), before);
    }

    #[test]
    fn tree_keeps_broken_symlink_in_entries() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let tmp = tempdir().expect("tmpdir should exist");
            let root = tmp.path().join("root");
            fs::create_dir_all(&root).expect("create root should work");
            symlink("missing.txt", root.join("broken-link")).expect("symlink should work");

            let tree = Tree::new(root, TreeMode::Normal, &GitSnapshot::default())
                .expect("tree should build");
            let link = tree
                .entries
                .iter()
                .find(|entry| entry.name == "broken-link")
                .expect("broken link should exist");

            assert!(link.is_symlink);
        }
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
