use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DirEntryNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
}

#[derive(Debug)]
pub struct Tree {
    pub startup_root: PathBuf,
    pub current_dir: PathBuf,
    pub entries: Vec<DirEntryNode>,
    selected: usize,
}

impl Tree {
    pub fn new(startup_root: PathBuf) -> anyhow::Result<Self> {
        let mut tree = Self {
            startup_root: startup_root.clone(),
            current_dir: startup_root,
            entries: Vec::new(),
            selected: 0,
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

    pub fn selected_is_dir(&self) -> bool {
        self.entries
            .get(self.selected)
            .map(|entry| entry.is_dir)
            .unwrap_or(false)
    }

    fn reload_entries(&mut self, prefer_selected_path: Option<&Path>) -> anyhow::Result<()> {
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

            let name = entry.file_name().to_string_lossy().to_string();
            entries.push(DirEntryNode {
                path,
                name,
                is_dir: file_type.is_dir(),
                is_symlink: file_type.is_symlink(),
            });
        }

        entries.sort_by(compare_entries);
        self.entries = entries;
        self.selected = prefer_selected_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .unwrap_or(0);
        Ok(())
    }
}

fn compare_entries(a: &DirEntryNode, b: &DirEntryNode) -> Ordering {
    match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::Tree;

    #[test]
    fn tree_stays_within_startup_root() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dirs should work");
        fs::write(root.join("sub/file.txt"), "hello").expect("write file should work");

        let tree = Tree::new(root.clone()).expect("tree should build");

        for node in &tree.entries {
            assert!(node.path.starts_with(&root));
        }
    }

    #[test]
    fn cannot_collapse_above_startup_root() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dirs should work");

        let mut tree = Tree::new(root.clone()).expect("tree should build");
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

        let mut tree = Tree::new(root).expect("tree should build");
        tree.move_down(); // b_dir を選択
        let selected_before = tree.selected_path().to_path_buf();
        tree.expand_selected().expect("expand should work");
        tree.collapse_selected();

        assert_eq!(tree.selected_path(), selected_before.as_path());
    }
}
