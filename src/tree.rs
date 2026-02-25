use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

pub type NodeId = usize;

#[derive(Debug, Clone)]
pub struct FsNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub loaded_children: bool,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

#[derive(Debug)]
pub struct Tree {
    pub startup_root: PathBuf,
    pub nodes: Vec<FsNode>,
    pub root: NodeId,
    pub selected: NodeId,
    visible: Vec<NodeId>,
}

impl Tree {
    pub fn new(startup_root: PathBuf) -> anyhow::Result<Self> {
        let name = startup_root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| startup_root.display().to_string());

        let root_node = FsNode {
            path: startup_root.clone(),
            name,
            is_dir: true,
            expanded: true,
            loaded_children: false,
            parent: None,
            children: Vec::new(),
        };

        let mut tree = Self {
            startup_root,
            nodes: vec![root_node],
            root: 0,
            selected: 0,
            visible: vec![0],
        };

        tree.load_children(0)?;
        tree.rebuild_visible();
        Ok(tree)
    }

    pub fn selected_path(&self) -> &Path {
        &self.nodes[self.selected].path
    }

    pub fn move_up(&mut self) {
        let index = self
            .visible
            .iter()
            .position(|id| *id == self.selected)
            .unwrap_or(0);

        if index > 0 {
            self.selected = self.visible[index - 1];
        }
    }

    pub fn move_down(&mut self) {
        let index = self
            .visible
            .iter()
            .position(|id| *id == self.selected)
            .unwrap_or(0);

        if index + 1 < self.visible.len() {
            self.selected = self.visible[index + 1];
        }
    }

    pub fn collapse_selected(&mut self) {
        if self.nodes[self.selected].is_dir && self.nodes[self.selected].expanded {
            self.nodes[self.selected].expanded = false;
            self.rebuild_visible();
            return;
        }

        if let Some(parent_id) = self.nodes[self.selected].parent {
            self.selected = parent_id;
        }
    }

    pub fn expand_selected(&mut self) -> anyhow::Result<()> {
        if !self.nodes[self.selected].is_dir {
            return Ok(());
        }

        if !self.nodes[self.selected].loaded_children {
            self.load_children(self.selected)?;
        }

        self.nodes[self.selected].expanded = true;
        self.rebuild_visible();
        Ok(())
    }

    pub fn visible_items(&self) -> Vec<VisibleNode<'_>> {
        self.visible
            .iter()
            .map(|id| {
                let depth = self.depth_of(*id);
                VisibleNode {
                    node: &self.nodes[*id],
                    depth,
                    is_selected: *id == self.selected,
                }
            })
            .collect()
    }

    fn depth_of(&self, mut id: NodeId) -> usize {
        let mut depth = 0;
        while let Some(parent) = self.nodes[id].parent {
            depth += 1;
            id = parent;
        }
        depth
    }

    fn load_children(&mut self, id: NodeId) -> anyhow::Result<()> {
        let parent_path = self.nodes[id].path.clone();
        let read_dir = fs::read_dir(&parent_path)?;

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
            entries.push((name, path, file_type.is_dir()));
        }

        entries.sort_by(compare_entries);

        let mut children = Vec::new();
        for (name, path, is_dir) in entries {
            let child_id = self.nodes.len();
            self.nodes.push(FsNode {
                path,
                name,
                is_dir,
                expanded: false,
                loaded_children: false,
                parent: Some(id),
                children: Vec::new(),
            });
            children.push(child_id);
        }

        self.nodes[id].children = children;
        self.nodes[id].loaded_children = true;
        Ok(())
    }

    fn rebuild_visible(&mut self) {
        self.visible.clear();
        self.push_visible(self.root);

        if !self.visible.contains(&self.selected) {
            self.selected = self.root;
        }
    }

    fn push_visible(&mut self, id: NodeId) {
        self.visible.push(id);

        if self.nodes[id].is_dir && self.nodes[id].expanded {
            let children = self.nodes[id].children.clone();
            for child in children {
                self.push_visible(child);
            }
        }
    }
}

fn compare_entries(a: &(String, PathBuf, bool), b: &(String, PathBuf, bool)) -> Ordering {
    match (a.2, b.2) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
    }
}

pub struct VisibleNode<'a> {
    pub node: &'a FsNode,
    pub depth: usize,
    pub is_selected: bool,
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

        for node in &tree.nodes {
            assert!(node.path.starts_with(&root));
        }
    }
}
