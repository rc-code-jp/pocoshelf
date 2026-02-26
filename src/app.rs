use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;

use crate::git_status::{GitSnapshot, GitState};
use crate::preview::{PreviewKind, PreviewRenderMode, PreviewState};
use crate::tree::Tree;

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
pub const TREE_RATIO_PERCENT: u16 = 20;
const COPY_STATUS_DURATION: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Tree,
    Preview,
}

pub struct App {
    pub startup_root: PathBuf,
    pub tree: Tree,
    pub preview: PreviewState,
    pub focus: FocusPane,
    pub git: GitSnapshot,
    pub status_message: String,
    pub last_git_refresh: Instant,
    pub should_quit: bool,
    clipboard: Option<Clipboard>,
    status_expires_at: Option<Instant>,
    git_refresh_tx: Sender<GitSnapshot>,
    git_refresh_rx: Receiver<GitSnapshot>,
    git_refresh_in_flight: bool,
    pending_manual_refresh: bool,
    preferred_preview_mode: Option<PreviewRenderMode>,
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
    TogglePreviewMode,
    NextChange,
    PrevChange,
    CopyRelativePath,
    Quit,
}

impl App {
    pub fn new(startup_root: PathBuf) -> anyhow::Result<Self> {
        let tree = Tree::new(startup_root.clone())?;

        let git = GitSnapshot::collect(&startup_root);
        let preview = PreviewState::from_path(&startup_root, tree.selected_path(), None);
        let (git_refresh_tx, git_refresh_rx) = mpsc::channel();

        Ok(Self {
            startup_root,
            tree,
            preview,
            focus: FocusPane::Tree,
            git,
            status_message: String::from("ready"),
            last_git_refresh: Instant::now(),
            should_quit: false,
            clipboard: Clipboard::new().ok(),
            status_expires_at: None,
            git_refresh_tx,
            git_refresh_rx,
            git_refresh_in_flight: false,
            pending_manual_refresh: false,
            preferred_preview_mode: None,
        })
    }

    pub fn handle_command(&mut self, command: Command) {
        self.poll_background_tasks();
        match command {
            Command::MoveUp => match self.focus {
                FocusPane::Tree => {
                    self.tree.move_up();
                    self.sync_preview();
                }
                FocusPane::Preview => self.preview.scroll_up(1),
            },
            Command::MoveDown => match self.focus {
                FocusPane::Tree => {
                    self.tree.move_down();
                    self.sync_preview();
                }
                FocusPane::Preview => self.preview.scroll_down(1),
            },
            Command::ExpandOrOpen => {
                if self.focus == FocusPane::Tree {
                    if self.tree.selected_is_dir() {
                        if let Err(err) = self.tree.expand_selected() {
                            self.status_message = format!("expand failed: {err}");
                        }
                        self.sync_preview();
                    } else {
                        self.sync_preview();
                        self.focus = FocusPane::Preview;
                    }
                }
            }
            Command::Collapse => {
                if self.focus == FocusPane::Preview {
                    self.focus = FocusPane::Tree;
                } else {
                    self.tree.collapse_selected();
                    self.sync_preview();
                }
            }
            Command::PreviewUp => self.preview.scroll_up(1),
            Command::PreviewDown => self.preview.scroll_down(1),
            Command::RefreshGit => self.request_git_refresh(true),
            Command::TogglePreviewMode => self.toggle_preview_mode(),
            Command::NextChange => self.jump_change(true),
            Command::PrevChange => self.jump_change(false),
            Command::CopyRelativePath => self.copy_relative_path(),
            Command::Quit => self.should_quit = true,
        }
    }

    pub fn periodic_refresh(&mut self) {
        self.request_git_refresh(false);
        self.poll_background_tasks();
    }

    pub fn poll_background_tasks(&mut self) {
        if let Some(expires_at) = self.status_expires_at {
            if Instant::now() >= expires_at {
                self.status_message = String::from("ready");
                self.status_expires_at = None;
            }
        }

        loop {
            match self.git_refresh_rx.try_recv() {
                Ok(snapshot) => {
                    self.git = snapshot;
                    self.last_git_refresh = Instant::now();
                    self.git_refresh_in_flight = false;

                    if self.pending_manual_refresh {
                        self.status_message = String::from("git refreshed");
                        self.status_expires_at = None;
                        self.pending_manual_refresh = false;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.git_refresh_in_flight = false;
                    break;
                }
            }
        }
    }

    fn request_git_refresh(&mut self, manual: bool) {
        if manual {
            self.pending_manual_refresh = true;
            self.status_message = String::from("git refreshing...");
            self.status_expires_at = None;
        }

        if self.git_refresh_in_flight {
            return;
        }

        self.git_refresh_in_flight = true;
        let tx = self.git_refresh_tx.clone();
        let root = self.startup_root.clone();

        thread::spawn(move || {
            let snapshot = GitSnapshot::collect(&root);
            let _ = tx.send(snapshot);
        });
    }

    fn sync_preview(&mut self) {
        self.preview = PreviewState::from_path(
            &self.startup_root,
            self.tree.selected_path(),
            self.preferred_preview_mode,
        );
    }

    fn toggle_preview_mode(&mut self) {
        let Some(next_mode) = self.preview.next_render_mode() else {
            self.set_temporary_status("preview mode unchanged");
            return;
        };

        self.preferred_preview_mode = Some(next_mode);
        self.sync_preview();
        self.set_temporary_status(format!("preview mode: {}", self.preview.mode_label()));
    }

    fn jump_change(&mut self, next: bool) {
        let moved = if next {
            self.preview.jump_to_next_change()
        } else {
            self.preview.jump_to_prev_change()
        };

        if !moved {
            self.set_temporary_status("no change marker in current view");
        }
    }

    fn copy_relative_path(&mut self) {
        let selected = self.tree.selected_path();
        match format_relative_with_at(&self.startup_root, selected) {
            Ok(text) => {
                if let Some(clipboard) = self.clipboard.as_mut() {
                    match clipboard.set_text(text.clone()) {
                        Ok(()) => self.set_temporary_status(format!("copied: {text}")),
                        Err(err) => self.set_temporary_status(format!("copy failed: {err}")),
                    }
                } else {
                    self.set_temporary_status("clipboard unavailable");
                }
            }
            Err(err) => self.set_temporary_status(format!("copy failed: {err}")),
        }
    }

    pub fn selected_git_state(&self, path: &Path, is_dir: bool) -> GitState {
        self.git.state_for(path, is_dir)
    }

    pub fn preview_title(&self) -> String {
        match self.preview.kind {
            PreviewKind::Text => format!("Preview ({})", self.preview.mode_label()),
            PreviewKind::Message => String::from("Preview (message)"),
        }
    }

    pub fn is_tree_focused(&self) -> bool {
        self.focus == FocusPane::Tree
    }

    pub fn is_preview_focused(&self) -> bool {
        self.focus == FocusPane::Preview
    }

    fn set_temporary_status(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
        self.status_expires_at = Some(Instant::now() + COPY_STATUS_DURATION);
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
    use std::fs;
    use std::path::Path;

    use git2::{IndexAddOption, Repository, Signature};
    use tempfile::tempdir;

    use crate::preview::PreviewRenderMode;

    use super::format_relative_with_at;
    use super::{App, Command};

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

    #[test]
    fn toggle_preview_mode_cycles_raw_diff() {
        let tmp = tempdir().expect("tmpdir should exist");
        let repo = Repository::init(tmp.path()).expect("git init should succeed");
        let file = tmp.path().join("file.txt");
        fs::write(&file, "line1\n").expect("write should succeed");
        commit_all(&repo, "initial");
        fs::write(&file, "line1\nline2\n").expect("write should succeed");

        let mut app = App::new(tmp.path().to_path_buf()).expect("app should build");
        select_by_file_name(&mut app, "file.txt");
        assert_eq!(app.preview.render_mode, PreviewRenderMode::Diff);

        app.handle_command(Command::TogglePreviewMode);
        assert_eq!(app.preview.render_mode, PreviewRenderMode::Raw);

        app.handle_command(Command::TogglePreviewMode);
        assert_eq!(app.preview.render_mode, PreviewRenderMode::Diff);
    }

    fn select_by_file_name(app: &mut App, file_name: &str) {
        for _ in 0..app.tree.entries.len() {
            if app
                .tree
                .selected_path()
                .file_name()
                .and_then(|name| name.to_str())
                == Some(file_name)
            {
                return;
            }
            app.handle_command(Command::MoveDown);
        }

        panic!("file should exist in tree: {file_name}");
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
