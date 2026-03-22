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
    pub depth: usize,
    pub is_expanded: bool,
}

#[derive(Debug)]
pub struct Tree {
    pub startup_root: PathBuf,
    pub entries: Vec<DirEntryNode>,
    pub mode: TreeMode,
    selected: usize,
    changed_paths: HashSet<PathBuf>,
    expanded_dirs: HashSet<PathBuf>,
}

impl Tree {
    pub fn new(startup_root: PathBuf, mode: TreeMode, git: &GitSnapshot) -> anyhow::Result<Self> {
        let mut expanded_dirs = HashSet::new();
        expanded_dirs.insert(startup_root.clone());

        let mut tree = Self {
            startup_root,
            entries: Vec::new(),
            mode,
            selected: 0,
            changed_paths: collect_existing_changed_paths(git, mode),
            expanded_dirs,
        };
        tree.reload_entries(None)?;
        Ok(tree)
    }

    pub fn selected_path(&self) -> &Path {
        self.entries
            .get(self.selected)
            .map(|entry| entry.path.as_path())
            .unwrap_or(self.startup_root.as_path())
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    pub fn collapse_selected(&mut self) -> anyhow::Result<()> {
        let Some(selected) = self.entries.get(self.selected).cloned() else {
            return Ok(());
        };

        if selected.is_dir && selected.is_expanded {
            self.expanded_dirs.remove(&selected.path);
            return self.reload_entries(Some(&selected.path));
        }

        if let Some(parent_index) = self.parent_index_of(&selected.path) {
            self.selected = parent_index;
        }

        Ok(())
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

        let path = selected.path.clone();
        if selected.is_expanded {
            self.expanded_dirs.remove(&path);
        } else {
            self.expanded_dirs.insert(path.clone());
        }

        self.reload_entries(Some(&path))
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

    pub fn root_label(&self) -> String {
        self.startup_root.display().to_string()
    }

    fn reload_entries(&mut self, prefer_selected_path: Option<&Path>) -> anyhow::Result<()> {
        self.expanded_dirs
            .retain(|path| path == &self.startup_root || path.exists());

        let preferred = prefer_selected_path
            .filter(|path| path.starts_with(&self.startup_root) && path.exists())
            .map(Path::to_path_buf);

        if let Some(path) = preferred.as_deref() {
            self.ensure_path_visible(path);
        }

        let mut entries = Vec::new();
        self.collect_entries(&self.startup_root, 0, &mut entries)?;

        self.entries = entries;
        self.selected = preferred
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .unwrap_or_else(|| default_selected_index(&self.entries));
        Ok(())
    }

    fn collect_entries(
        &self,
        dir: &Path,
        depth: usize,
        out: &mut Vec<DirEntryNode>,
    ) -> anyhow::Result<()> {
        let mut children =
            read_directory_entries(dir, self.mode, &self.changed_paths, &self.startup_root)?;

        children.sort_by(compare_entries);

        for mut child in children {
            child.depth = depth;
            if child.is_dir {
                child.is_expanded = self.expanded_dirs.contains(&child.path);
            }

            let descend = child.is_dir && child.is_expanded;
            let child_path = child.path.clone();
            out.push(child);

            if descend {
                self.collect_entries(&child_path, depth + 1, out)?;
            }
        }

        Ok(())
    }

    fn ensure_path_visible(&mut self, path: &Path) {
        if !path.starts_with(&self.startup_root) {
            return;
        }

        let mut current = path.parent();
        while let Some(dir) = current {
            if !dir.starts_with(&self.startup_root) {
                break;
            }

            self.expanded_dirs.insert(dir.to_path_buf());
            if dir == self.startup_root {
                break;
            }
            current = dir.parent();
        }
    }

    fn parent_index_of(&self, path: &Path) -> Option<usize> {
        let parent = path.parent()?;
        if parent == self.startup_root {
            return None;
        }

        self.entries.iter().position(|entry| entry.path == parent)
    }
}

fn read_directory_entries(
    dir: &Path,
    mode: TreeMode,
    changed_paths: &HashSet<PathBuf>,
    startup_root: &Path,
) -> anyhow::Result<Vec<DirEntryNode>> {
    let read_dir = fs::read_dir(dir)?;
    let mut entries = Vec::new();

    for entry_res in read_dir {
        let entry = match entry_res {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.starts_with(startup_root) {
            continue;
        }

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        let is_dir = file_type.is_dir();
        if mode == TreeMode::Changed && !is_changed_visible(&path, is_dir, changed_paths) {
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
            depth: 0,
            is_expanded: false,
        });
    }

    Ok(entries)
}

fn default_selected_index(entries: &[DirEntryNode]) -> usize {
    if entries.is_empty() {
        0
    } else {
        entries.iter().position(|entry| !entry.is_dir).unwrap_or(0)
    }
}

fn compare_entries(a: &DirEntryNode, b: &DirEntryNode) -> Ordering {
    match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    }
}

fn is_changed_visible(path: &Path, is_dir: bool, changed_paths: &HashSet<PathBuf>) -> bool {
    if is_dir {
        changed_paths
            .iter()
            .any(|changed| changed.starts_with(path))
    } else {
        changed_paths.contains(path)
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

fn collect_existing_changed_paths(git: &GitSnapshot, mode: TreeMode) -> HashSet<PathBuf> {
    if mode != TreeMode::Changed {
        return HashSet::new();
    }

    git.changed_file_paths()
        .into_iter()
        .filter(|path| path.exists())
        .collect()
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
    fn collapse_at_root_level_keeps_selection() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dirs should work");

        let mut tree = Tree::new(root.clone(), TreeMode::Normal, &GitSnapshot::default())
            .expect("tree should build");
        tree.collapse_selected().expect("collapse should work");

        assert_eq!(tree.selected_path(), root.join("sub").as_path());
    }

    #[test]
    fn expanding_directory_inserts_children_inline() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dirs should work");
        fs::write(root.join("sub/file.txt"), "hello").expect("write file should work");

        let mut tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");
        tree.expand_selected().expect("expand should work");

        assert_eq!(tree.entries.len(), 2);
        assert_eq!(tree.entries[0].name, "sub");
        assert_eq!(tree.entries[0].depth, 0);
        assert!(tree.entries[0].is_expanded);
        assert_eq!(tree.entries[1].name, "file.txt");
        assert_eq!(tree.entries[1].depth, 1);
    }

    #[test]
    fn collapsing_expanded_directory_hides_children() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dirs should work");
        fs::write(root.join("sub/file.txt"), "hello").expect("write file should work");

        let mut tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");
        tree.expand_selected().expect("expand should work");
        tree.collapse_selected().expect("collapse should work");

        assert_eq!(tree.entries.len(), 1);
        assert_eq!(tree.entries[0].name, "sub");
        assert!(!tree.entries[0].is_expanded);
    }

    #[test]
    fn collapse_on_child_moves_selection_to_parent_directory() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dirs should work");
        fs::write(root.join("sub/file.txt"), "hello").expect("write file should work");

        let mut tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");
        tree.expand_selected().expect("expand should work");
        tree.move_down();

        tree.collapse_selected().expect("collapse should work");

        assert_eq!(
            tree.selected_path().file_name().and_then(|n| n.to_str()),
            Some("sub")
        );
    }

    #[test]
    fn refresh_keeps_selected_path_visible_by_expanding_ancestors() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("src/nested")).expect("create dirs should work");
        fs::write(root.join("src/nested/file.txt"), "hello").expect("write file should work");

        let mut tree = Tree::new(root.clone(), TreeMode::Normal, &GitSnapshot::default())
            .expect("tree should build");
        tree.expand_selected().expect("expand should work");
        tree.move_down();
        tree.expand_selected().expect("expand nested should work");
        tree.move_down();
        tree.move_down();

        assert_eq!(
            tree.selected_path(),
            root.join("src/nested/file.txt").as_path()
        );

        tree.refresh().expect("refresh should work");

        assert_eq!(
            tree.selected_path(),
            root.join("src/nested/file.txt").as_path()
        );
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
        assert_eq!(tree.entries[1].name, "nested");
        tree.move_down();
        tree.expand_selected().expect("expand nested should work");
        assert_eq!(tree.entries[2].name, "file.txt");
    }

    #[test]
    fn set_mode_keeps_selected_directory_visible_when_it_is_still_valid() {
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
        let src_index = tree
            .entries
            .iter()
            .position(|entry| entry.name == "src")
            .expect("src should exist");
        assert!(tree.select_index(src_index));
        tree.expand_selected().expect("expand src should work");
        tree.move_down();
        tree.expand_selected().expect("expand nested should work");

        tree.set_mode(TreeMode::Changed, &git)
            .expect("mode switch should work");

        assert_eq!(tree.selected_path(), root.join("src/nested").as_path());
        assert_eq!(tree.entries.len(), 3);
        assert_eq!(tree.entries[0].name, "src");
        assert_eq!(tree.entries[1].name, "nested");
        assert_eq!(tree.entries[2].name, "file.txt");
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
        assert!(dir.modified_date.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn tree_marks_symlinks() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(&root).expect("create root should work");
        fs::write(root.join("note.txt"), "hello").expect("write file should work");
        symlink(root.join("note.txt"), root.join("note-link")).expect("symlink should work");

        let tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");
        let link = tree
            .entries
            .iter()
            .find(|entry| entry.name == "note-link")
            .expect("note-link should exist");

        assert!(link.is_symlink);
    }

    #[cfg(unix)]
    #[test]
    fn broken_symlink_still_appears_in_tree() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(&root).expect("create root should work");
        symlink(root.join("missing.txt"), root.join("broken-link")).expect("symlink should work");

        let tree =
            Tree::new(root, TreeMode::Normal, &GitSnapshot::default()).expect("tree should build");

        assert!(tree.entries.iter().any(|entry| entry.name == "broken-link"));
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
            .and_then(|head| head.target())
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
