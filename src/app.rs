use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use arboard::Clipboard;

use crate::git_status::{GitSnapshot, GitState};
use crate::preview::{PreviewKind, PreviewState, MAX_PREVIEW_BYTES};
use crate::tree::Tree;

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
pub const TREE_RATIO_PERCENT: u16 = 20;

pub struct App {
    pub startup_root: PathBuf,
    pub tree: Tree,
    pub preview: PreviewState,
    pub git: GitSnapshot,
    pub status_message: String,
    pub last_git_refresh: Instant,
    pub should_quit: bool,
    clipboard: Option<Clipboard>,
}

#[derive(Debug, Clone, Copy)]
pub enum Command {
    MoveUp,
    MoveDown,
    ExpandOrOpen,
    Collapse,
    PreviewUp,
    PreviewDown,
    RefreshGit,
    CopyRelativePath,
    Quit,
}

impl App {
    pub fn new(startup_root: PathBuf) -> anyhow::Result<Self> {
        let mut tree = Tree::new(startup_root.clone())?;
        tree.expand_selected()?;

        let git = GitSnapshot::collect(&startup_root);
        let preview = PreviewState::from_path(tree.selected_path(), MAX_PREVIEW_BYTES);

        Ok(Self {
            startup_root,
            tree,
            preview,
            git,
            status_message: String::from("ready"),
            last_git_refresh: Instant::now(),
            should_quit: false,
            clipboard: Clipboard::new().ok(),
        })
    }

    pub fn handle_command(&mut self, command: Command) {
        match command {
            Command::MoveUp => {
                self.tree.move_up();
                self.sync_preview();
            }
            Command::MoveDown => {
                self.tree.move_down();
                self.sync_preview();
            }
            Command::ExpandOrOpen => {
                if let Err(err) = self.tree.expand_selected() {
                    self.status_message = format!("expand failed: {err}");
                }
                self.sync_preview();
            }
            Command::Collapse => {
                self.tree.collapse_selected();
                self.sync_preview();
            }
            Command::PreviewUp => self.preview.scroll_up(1),
            Command::PreviewDown => self.preview.scroll_down(1),
            Command::RefreshGit => self.refresh_git(true),
            Command::CopyRelativePath => self.copy_relative_path(),
            Command::Quit => self.should_quit = true,
        }
    }

    pub fn periodic_refresh(&mut self) {
        self.refresh_git(false);
    }

    fn refresh_git(&mut self, manual: bool) {
        self.git = GitSnapshot::collect(&self.startup_root);
        self.last_git_refresh = Instant::now();
        if manual {
            self.status_message = String::from("git refreshed");
        }
    }

    fn sync_preview(&mut self) {
        self.preview = PreviewState::from_path(self.tree.selected_path(), MAX_PREVIEW_BYTES);
    }

    fn copy_relative_path(&mut self) {
        let selected = self.tree.selected_path();
        match format_relative_with_at(&self.startup_root, selected) {
            Ok(text) => {
                if let Some(clipboard) = self.clipboard.as_mut() {
                    match clipboard.set_text(text.clone()) {
                        Ok(()) => self.status_message = format!("copied: {text}"),
                        Err(err) => self.status_message = format!("copy failed: {err}"),
                    }
                } else {
                    self.status_message = String::from("clipboard unavailable");
                }
            }
            Err(err) => self.status_message = format!("copy failed: {err}"),
        }
    }

    pub fn selected_git_state(&self, path: &Path, is_dir: bool) -> GitState {
        self.git.state_for(path, is_dir)
    }

    pub fn preview_title(&self) -> &'static str {
        match self.preview.kind {
            PreviewKind::Text => "Preview",
            PreviewKind::Message => "Preview (message)",
        }
    }
}

pub fn format_relative_with_at(startup_root: &Path, selected: &Path) -> anyhow::Result<String> {
    let relative = selected.strip_prefix(startup_root)?;

    if relative.as_os_str().is_empty() {
        return Ok(String::from("@."));
    }

    let mut out = String::from("@");
    out.push_str(&normalize_to_slashes(relative));
    Ok(out)
}

fn normalize_to_slashes(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::format_relative_with_at;

    #[test]
    fn format_relative_file() {
        let root = Path::new("/repo");
        let file = Path::new("/repo/docs/sample.txt");
        let out = format_relative_with_at(root, file).expect("relative path should format");
        assert_eq!(out, "@docs/sample.txt");
    }

    #[test]
    fn format_relative_root() {
        let root = Path::new("/repo");
        let out = format_relative_with_at(root, root).expect("root should format");
        assert_eq!(out, "@.");
    }

    #[test]
    fn format_relative_fails_outside_root() {
        let root = Path::new("/repo");
        let outside = Path::new("/other/file.txt");
        assert!(format_relative_with_at(root, outside).is_err());
    }
}
