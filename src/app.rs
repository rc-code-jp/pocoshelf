use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use notify::{RecursiveMode, Watcher};
use ratatui::layout::Rect;

use crate::config::Config;
use crate::git_status::{collect_ignored_paths, GitSnapshot, GitState};
use crate::preview::{PreviewKind, PreviewRenderMode, PreviewState};
use crate::tree::{Tree, TreeMode};
use crate::ui;

const COPY_STATUS_DURATION: Duration = Duration::from_secs(3);
const FS_REFRESH_DEBOUNCE: Duration = Duration::from_millis(300);
const PREVIEW_WHEEL_SCROLL_AMOUNT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Tree,
    Preview,
}

pub struct App {
    pub config: Config,
    pub startup_root: PathBuf,
    pub tree: Tree,
    pub hovered_tree_index: Option<usize>,
    pub preview: PreviewState,
    pub focus: FocusPane,
    pub git: GitSnapshot,
    visible_ignored_paths: HashSet<PathBuf>,
    pub status_message: String,
    pub last_git_refresh: Instant,
    pub should_quit: bool,
    pub show_help: bool,
    clipboard: Option<Clipboard>,
    status_expires_at: Option<Instant>,
    git_refresh_tx: Sender<GitSnapshot>,
    git_refresh_rx: Receiver<GitSnapshot>,
    fs_refresh_rx: Receiver<()>,
    _fs_watcher: notify::RecommendedWatcher,
    git_refresh_in_flight: bool,
    pending_manual_refresh: bool,
    preferred_preview_mode: Option<PreviewRenderMode>,
    pending_fs_refresh: bool,
    last_fs_event_at: Option<Instant>,
    preview_viewport_height: usize,
    preview_viewport_width: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum Command {
    MoveUp,
    MoveDown,
    PreviewHalfPageUp,
    PreviewHalfPageDown,
    PreviewPageUp,
    PreviewPageDown,
    ExpandOrOpen,
    Collapse,
    RefreshGit,
    TogglePreviewMode,
    ToggleTreeMode,
    ToggleHelp,
    NextChange,
    PrevChange,
    CopyRelativePath,
    OpenInVi,
    OpenInFinder,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEffect {
    OpenInVi(PathBuf),
}

impl App {
    pub fn new(startup_root: PathBuf, initial_tree_mode: TreeMode) -> anyhow::Result<Self> {
        let config = Config::load();
        let git = GitSnapshot::collect(&startup_root);
        let tree = Tree::new(startup_root.clone(), initial_tree_mode, &git)?;
        let visible_ignored_paths = collect_ignored_paths(
            &startup_root,
            tree.entries.iter().map(|entry| entry.path.as_path()),
        );
        let preview = PreviewState::from_path(&startup_root, tree.selected_path(), None);
        let (git_refresh_tx, git_refresh_rx) = mpsc::channel();
        let (fs_refresh_tx, fs_refresh_rx) = mpsc::channel();

        let mut fs_watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    // Ignore Access/Metadata events as they don't change the tree structure
                    if !matches!(
                        event.kind,
                        notify::EventKind::Access(_)
                            | notify::EventKind::Modify(notify::event::ModifyKind::Metadata(_))
                    ) {
                        let _ = fs_refresh_tx.send(());
                    }
                }
            })?;
        fs_watcher.watch(&startup_root, RecursiveMode::Recursive)?;

        let mut app = Self {
            config,
            startup_root,
            tree,
            hovered_tree_index: None,
            preview,
            focus: FocusPane::Tree,
            git,
            visible_ignored_paths,
            status_message: String::from("ready"),
            last_git_refresh: Instant::now(),
            should_quit: false,
            show_help: false,
            clipboard: Clipboard::new().ok(),
            status_expires_at: None,
            git_refresh_tx,
            git_refresh_rx,
            fs_refresh_rx,
            _fs_watcher: fs_watcher,
            git_refresh_in_flight: false,
            pending_manual_refresh: false,
            preferred_preview_mode: None,
            pending_fs_refresh: false,
            last_fs_event_at: None,
            preview_viewport_height: 1,
            preview_viewport_width: 1,
        };
        app.update_changed_empty_status();
        Ok(app)
    }

    pub fn handle_command(&mut self, command: Command) -> Option<AppEffect> {
        self.poll_background_tasks();

        if self.show_help {
            match command {
                Command::ToggleHelp | Command::Collapse => {
                    self.show_help = false;
                }
                Command::Quit => self.should_quit = true,
                _ => {}
            }
            return None;
        }

        match command {
            Command::MoveUp => match self.focus {
                FocusPane::Tree => {
                    self.tree.move_up();
                    self.sync_preview();
                }
                FocusPane::Preview => self.scroll_preview_up(1),
            },
            Command::MoveDown => match self.focus {
                FocusPane::Tree => {
                    self.tree.move_down();
                    self.sync_preview();
                }
                FocusPane::Preview => self.scroll_preview_down(1),
            },
            Command::ExpandOrOpen => {
                if self.focus == FocusPane::Tree {
                    if self.tree.selected_is_parent_link() {
                        self.tree.collapse_selected();
                        self.sync_tree_state();
                    } else if self.tree.selected_is_dir() {
                        if let Err(err) = self.tree.expand_selected() {
                            self.status_message = format!("expand failed: {err}");
                        }
                        self.sync_tree_state();
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
                    self.sync_tree_state();
                }
            }
            Command::PreviewHalfPageUp => self.scroll_preview_up(self.preview_half_page_amount()),
            Command::PreviewHalfPageDown => {
                self.scroll_preview_down(self.preview_half_page_amount())
            }
            Command::PreviewPageUp => self.scroll_preview_up(self.preview_page_amount()),
            Command::PreviewPageDown => self.scroll_preview_down(self.preview_page_amount()),
            Command::RefreshGit => self.request_git_refresh(true),
            Command::TogglePreviewMode => self.toggle_preview_mode(),
            Command::ToggleTreeMode => self.toggle_tree_mode(),
            Command::ToggleHelp => self.show_help = true,
            Command::NextChange => self.jump_change(true),
            Command::PrevChange => self.jump_change(false),
            Command::CopyRelativePath => self.copy_relative_path(),
            Command::OpenInVi => return self.open_in_vi(),
            Command::OpenInFinder => self.open_in_finder(),
            Command::Quit => self.should_quit = true,
        }

        None
    }

    pub fn poll_background_tasks(&mut self) {
        if let Some(expires_at) = self.status_expires_at {
            if Instant::now() >= expires_at {
                self.status_message = String::from("ready");
                self.status_expires_at = None;
                self.update_changed_empty_status();
            }
        }

        let mut needs_tree_refresh = false;
        while let Ok(()) = self.fs_refresh_rx.try_recv() {
            needs_tree_refresh = true;
        }

        if needs_tree_refresh {
            self.pending_fs_refresh = true;
            self.last_fs_event_at = Some(Instant::now());
        }

        if self.should_flush_fs_refresh() {
            self.pending_fs_refresh = false;
            self.last_fs_event_at = None;

            if let Err(err) = self.tree.refresh() {
                self.status_message = format!("tree refresh failed: {err}");
            } else {
                self.sync_tree_state();
                self.request_git_refresh(false);
            }
        }

        loop {
            match self.git_refresh_rx.try_recv() {
                Ok(snapshot) => {
                    self.git = snapshot;
                    if let Err(err) = self.tree.update_changed_paths(&self.git) {
                        self.status_message = format!("tree refresh failed: {err}");
                    } else {
                        self.sync_tree_state();
                    }
                    self.last_git_refresh = Instant::now();
                    self.git_refresh_in_flight = false;

                    if self.pending_manual_refresh {
                        self.status_message = self.refresh_success_message();
                        self.status_expires_at = None;
                        self.pending_manual_refresh = false;
                    } else {
                        self.update_changed_empty_status();
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

    pub fn on_focus_gained(&mut self) {
        self.pending_fs_refresh = false;
        self.last_fs_event_at = None;

        if let Err(err) = self.tree.refresh() {
            self.status_message = format!("tree refresh failed: {err}");
            return;
        }

        self.sync_tree_state();
        self.request_git_refresh(false);
    }

    pub fn handle_tree_left_click(
        &mut self,
        terminal_area: Rect,
        column: u16,
        row: u16,
    ) -> Option<AppEffect> {
        if self.show_help {
            return None;
        }

        if self.is_preview_focused() && ui::tree_contains(terminal_area, self, column, row) {
            self.focus = FocusPane::Tree;
            return None;
        }

        let tree_area = ui::tree_area(terminal_area, self);
        let Some(index) = ui::tree_index_at(tree_area, self, column, row) else {
            return None;
        };

        if !self.tree.select_index(index) {
            return None;
        }

        self.sync_preview();
        self.handle_command(Command::ExpandOrOpen)
    }

    pub fn update_tree_hover(&mut self, terminal_area: Rect, column: u16, row: u16) {
        if self.show_help {
            self.hovered_tree_index = None;
            return;
        }

        let tree_area = ui::tree_area(terminal_area, self);
        self.hovered_tree_index = ui::tree_index_at(tree_area, self, column, row);
    }

    pub fn handle_preview_wheel(
        &mut self,
        terminal_area: Rect,
        column: u16,
        row: u16,
        scroll_up: bool,
    ) {
        if self.show_help || !ui::preview_contains(terminal_area, self, column, row) {
            return;
        }

        if scroll_up {
            self.scroll_preview_up(PREVIEW_WHEEL_SCROLL_AMOUNT);
        } else {
            self.scroll_preview_down(PREVIEW_WHEEL_SCROLL_AMOUNT);
        }
    }

    fn sync_preview(&mut self) {
        self.preview = PreviewState::from_path(
            &self.startup_root,
            self.tree.selected_path(),
            self.preferred_preview_mode,
        );
        self.clamp_preview_scroll();
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

    fn toggle_tree_mode(&mut self) {
        let next_mode = match self.tree.mode {
            TreeMode::Normal => TreeMode::Changed,
            TreeMode::Changed => TreeMode::Normal,
        };

        if let Err(err) = self.tree.set_mode(next_mode, &self.git) {
            self.set_temporary_status(format!("tree mode switch failed: {err}"));
            return;
        }

        self.sync_tree_state();
        self.set_temporary_status(self.tree_mode_status_message());
    }

    fn jump_change(&mut self, next: bool) {
        let moved = if next {
            self.preview.jump_to_next_change()
        } else {
            self.preview.jump_to_prev_change()
        };

        self.clamp_preview_scroll();

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

    fn open_in_finder(&mut self) {
        let selected = self.tree.selected_path();
        let target_dir = resolve_directory_to_open(selected);

        let mut command = finder_open_command(target_dir);
        match command.status() {
            Ok(status) if status.success() => {
                self.set_temporary_status(format!("opened: {}", target_dir.display()));
            }
            Ok(status) => {
                self.set_temporary_status(format!("open failed (status: {status})"));
            }
            Err(err) => {
                self.set_temporary_status(format!("open failed: {err}"));
            }
        }
    }

    fn open_in_vi(&mut self) -> Option<AppEffect> {
        if self.tree.selected_is_dir() {
            self.set_temporary_status("directory selected; vi skipped");
            return None;
        }

        Some(AppEffect::OpenInVi(self.tree.selected_path().to_path_buf()))
    }

    pub fn selected_git_state(&self, path: &Path, is_dir: bool) -> GitState {
        let state = self.git.state_for(path, is_dir);
        if state != GitState::Clean {
            return state;
        }

        if self.visible_ignored_paths.contains(path) {
            GitState::Ignored
        } else {
            GitState::Clean
        }
    }

    pub fn preview_title(&self) -> String {
        match self.preview.kind {
            PreviewKind::Text => format!("Preview ({})", self.preview.mode_label()),
            PreviewKind::Directory => String::from("Preview (directory)"),
            PreviewKind::Message => String::from("Preview (message)"),
        }
    }

    pub fn tree_title(&self) -> String {
        format!(
            "Dir: {} [{}]",
            self.tree.current_dir.display(),
            self.tree.mode.label()
        )
    }

    pub fn is_tree_focused(&self) -> bool {
        self.focus == FocusPane::Tree
    }

    pub fn is_preview_focused(&self) -> bool {
        self.focus == FocusPane::Preview
    }

    pub fn set_preview_viewport_size(&mut self, width: usize, height: usize) {
        self.preview_viewport_width = width.max(1);
        self.preview_viewport_height = height.max(1);
        self.clamp_preview_scroll();
    }

    fn set_temporary_status(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
        self.status_expires_at = Some(Instant::now() + COPY_STATUS_DURATION);
    }

    fn preview_half_page_amount(&self) -> usize {
        (self.preview_viewport_height / 2).max(1)
    }

    fn preview_page_amount(&self) -> usize {
        self.preview_viewport_height.max(1)
    }

    fn scroll_preview_up(&mut self, amount: usize) {
        self.preview.scroll_up(amount);
        self.clamp_preview_scroll();
    }

    fn scroll_preview_down(&mut self, amount: usize) {
        self.preview.scroll_down(amount);
        self.clamp_preview_scroll();
    }

    fn clamp_preview_scroll(&mut self) {
        let max_scroll = ui::preview_max_scroll(
            &self.preview,
            self.preview_viewport_height,
            self.preview_viewport_width,
        );
        self.preview.scroll = self.preview.scroll.min(max_scroll);
    }

    pub fn set_external_status(&mut self, msg: impl Into<String>) {
        self.set_temporary_status(msg);
    }

    fn should_flush_fs_refresh(&self) -> bool {
        self.pending_fs_refresh
            && self
                .last_fs_event_at
                .is_some_and(|at| at.elapsed() >= FS_REFRESH_DEBOUNCE)
    }

    fn refresh_visible_ignored_paths(&mut self) {
        self.visible_ignored_paths = collect_ignored_paths(
            &self.startup_root,
            self.tree.entries.iter().map(|entry| entry.path.as_path()),
        );
    }

    fn sync_tree_state(&mut self) {
        self.refresh_visible_ignored_paths();
        self.hovered_tree_index = self
            .hovered_tree_index
            .filter(|index| *index < self.tree.entries.len());
        self.sync_preview();
        self.update_changed_empty_status();
    }

    fn tree_mode_status_message(&self) -> String {
        if self.tree.mode == TreeMode::Changed && self.tree.entries.is_empty() {
            String::from("tree mode: changed (no files)")
        } else {
            format!("tree mode: {}", self.tree.mode.label())
        }
    }

    fn refresh_success_message(&self) -> String {
        if self.tree.mode == TreeMode::Changed && self.tree.entries.is_empty() {
            String::from("git refreshed | changed tree: no files")
        } else {
            String::from("git refreshed")
        }
    }

    fn update_changed_empty_status(&mut self) {
        if self.tree.mode == TreeMode::Changed && self.tree.entries.is_empty() {
            self.status_message = String::from("changed tree: no files");
            self.status_expires_at = None;
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

fn resolve_directory_to_open(selected: &Path) -> &Path {
    selected.parent().unwrap_or(selected)
}

fn finder_open_command(path: &Path) -> ProcessCommand {
    #[cfg(target_os = "macos")]
    {
        let mut command = ProcessCommand::new("open");
        command.arg(path);
        command
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mut command = ProcessCommand::new("xdg-open");
        command.arg(path);
        command
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use git2::{IndexAddOption, Repository, Signature};
    use tempfile::tempdir;

    use crate::git_status::GitState;
    use crate::preview::PreviewRenderMode;
    use crate::tree::TreeMode;
    use crate::ui;
    use ratatui::layout::Rect;

    use super::{
        format_relative_with_at, resolve_directory_to_open, App, AppEffect, Command,
        FS_REFRESH_DEBOUNCE,
    };

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

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "file.txt");
        assert_eq!(app.preview.render_mode, PreviewRenderMode::Diff);

        let _ = app.handle_command(Command::TogglePreviewMode);
        assert_eq!(app.preview.render_mode, PreviewRenderMode::Raw);

        let _ = app.handle_command(Command::TogglePreviewMode);
        assert_eq!(app.preview.render_mode, PreviewRenderMode::Diff);
    }

    #[test]
    fn preview_half_page_scroll_uses_viewport_height() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(
            tmp.path().join("note.txt"),
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
        )
        .expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        app.set_preview_viewport_size(20, 6);

        let _ = app.handle_command(Command::PreviewHalfPageDown);
        assert_eq!(app.preview.scroll, 3);

        let _ = app.handle_command(Command::PreviewHalfPageUp);
        assert_eq!(app.preview.scroll, 0);
    }

    #[test]
    fn preview_page_scroll_uses_viewport_height() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(
            tmp.path().join("note.txt"),
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
        )
        .expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        app.set_preview_viewport_size(20, 4);

        let _ = app.handle_command(Command::PreviewPageDown);
        assert_eq!(app.preview.scroll, 4);

        let _ = app.handle_command(Command::PreviewPageUp);
        assert_eq!(app.preview.scroll, 0);
    }

    #[test]
    fn preview_wheel_scrolls_down_by_three_lines() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(
            tmp.path().join("note.txt"),
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
        )
        .expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        app.set_preview_viewport_size(20, 4);
        let terminal_area = Rect::new(0, 0, 20, 10);
        let preview_area = ui::preview_area(terminal_area, &app);

        app.handle_preview_wheel(terminal_area, preview_area.x + 1, preview_area.y + 1, false);

        assert_eq!(app.preview.scroll, 3);
        assert!(app.is_preview_focused());
    }

    #[test]
    fn preview_wheel_scrolls_up_and_clamps_at_zero() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(
            tmp.path().join("note.txt"),
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
        )
        .expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        app.set_preview_viewport_size(20, 4);
        app.preview.scroll = 2;
        let terminal_area = Rect::new(0, 0, 20, 10);
        let preview_area = ui::preview_area(terminal_area, &app);

        app.handle_preview_wheel(terminal_area, preview_area.x + 1, preview_area.y + 1, true);

        assert_eq!(app.preview.scroll, 0);
        assert!(app.is_preview_focused());
    }

    #[test]
    fn preview_wheel_ignores_non_preview_region() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(
            tmp.path().join("note.txt"),
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
        )
        .expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        app.set_preview_viewport_size(20, 4);
        let before_focus = app.focus;
        let terminal_area = Rect::new(0, 0, 20, 10);

        app.handle_preview_wheel(terminal_area, 1, 1, false);

        assert_eq!(app.preview.scroll, 0);
        assert_eq!(app.focus, before_focus);
    }

    #[test]
    fn preview_wheel_ignores_help_mode() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(
            tmp.path().join("note.txt"),
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
        )
        .expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        app.set_preview_viewport_size(20, 4);
        app.show_help = true;
        let terminal_area = Rect::new(0, 0, 20, 10);
        let preview_area = ui::preview_area(terminal_area, &app);

        app.handle_preview_wheel(terminal_area, preview_area.x + 1, preview_area.y + 1, false);

        assert_eq!(app.preview.scroll, 0);
    }

    #[test]
    fn preview_page_down_stops_at_last_full_page() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(
            tmp.path().join("note.txt"),
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
        )
        .expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        app.set_preview_viewport_size(20, 4);

        for _ in 0..5 {
            let _ = app.handle_command(Command::PreviewPageDown);
        }

        assert_eq!(app.preview.scroll, 6);
    }

    #[test]
    fn preview_wheel_down_stops_at_last_full_page() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(
            tmp.path().join("note.txt"),
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
        )
        .expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        app.set_preview_viewport_size(20, 4);
        let terminal_area = Rect::new(0, 0, 20, 10);
        let preview_area = ui::preview_area(terminal_area, &app);

        for _ in 0..5 {
            app.handle_preview_wheel(terminal_area, preview_area.x + 1, preview_area.y + 1, false);
        }

        assert_eq!(app.preview.scroll, 6);
    }

    #[test]
    fn help_toggles_and_blocks_navigation_commands() {
        let tmp = tempdir().expect("tmpdir should exist");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let before = app.tree.selected_path().to_path_buf();

        let _ = app.handle_command(Command::ToggleHelp);
        assert!(app.show_help);

        let _ = app.handle_command(Command::MoveDown);
        assert_eq!(app.tree.selected_path(), before.as_path());

        let _ = app.handle_command(Command::ToggleHelp);
        assert!(!app.show_help);
    }

    #[test]
    fn open_in_vi_returns_effect_for_file() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("note.txt"), "hello").expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");

        let effect = app.handle_command(Command::OpenInVi);
        assert_eq!(
            effect,
            Some(AppEffect::OpenInVi(tmp.path().join("note.txt")))
        );
    }

    #[test]
    fn open_in_vi_skips_directory_selection() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::create_dir_all(tmp.path().join("sub")).expect("create dir should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "sub");

        let effect = app.handle_command(Command::OpenInVi);
        assert_eq!(effect, None);
        assert_eq!(app.status_message, "directory selected; vi skipped");
    }

    #[test]
    fn resolve_directory_returns_parent_for_file() {
        let file = Path::new("/repo/docs/sample.txt");
        let out = resolve_directory_to_open(file);
        assert_eq!(out, Path::new("/repo/docs"));
    }

    #[test]
    fn resolve_directory_returns_parent_for_directory() {
        let dir = Path::new("/repo/docs");
        let out = resolve_directory_to_open(dir);
        assert_eq!(out, Path::new("/repo"));
    }

    #[test]
    fn fs_refresh_flushes_after_debounce() {
        let tmp = tempdir().expect("tmpdir should exist");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.pending_fs_refresh = true;
        app.last_fs_event_at =
            Some(Instant::now() - FS_REFRESH_DEBOUNCE - Duration::from_millis(1));

        assert!(app.should_flush_fs_refresh());
    }

    #[test]
    fn fs_refresh_waits_within_debounce_window() {
        let tmp = tempdir().expect("tmpdir should exist");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.pending_fs_refresh = true;
        app.last_fs_event_at = Some(Instant::now());

        assert!(!app.should_flush_fs_refresh());
    }

    #[test]
    fn selected_git_state_marks_visible_ignored_entries() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        Repository::init(root).expect("git init should succeed");
        fs::write(root.join(".gitignore"), "ignored-dir/\nignored.txt\n")
            .expect("gitignore should write");
        fs::create_dir_all(root.join("ignored-dir")).expect("ignored dir should create");
        fs::write(root.join("ignored.txt"), "skip").expect("ignored file should write");

        let app = App::new(root.to_path_buf(), TreeMode::Normal).expect("app should build");

        let ignored_dir = root.join("ignored-dir");
        let ignored_file = root.join("ignored.txt");
        assert_eq!(
            app.selected_git_state(&ignored_dir, true),
            GitState::Ignored
        );
        assert_eq!(
            app.selected_git_state(&ignored_file, false),
            GitState::Ignored
        );
    }

    #[test]
    fn preview_title_for_directory_uses_directory_label() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::create_dir_all(tmp.path().join("sub")).expect("create dir should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "sub");

        assert_eq!(app.preview_title(), "Preview (directory)");
    }

    #[test]
    fn selected_git_state_refreshes_when_entering_directory() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        Repository::init(root).expect("git init should succeed");
        fs::write(root.join(".gitignore"), "nested/ignored.log\n").expect("gitignore should write");
        fs::create_dir_all(root.join("nested")).expect("nested dir should create");
        fs::write(root.join("nested/ignored.log"), "skip").expect("ignored file should write");

        let mut app = App::new(root.to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "nested");
        let _ = app.handle_command(Command::ExpandOrOpen);

        let ignored_file = root.join("nested/ignored.log");
        assert_eq!(
            app.selected_git_state(&ignored_file, false),
            GitState::Ignored
        );
    }

    #[test]
    fn changed_tree_mode_can_be_initial_mode() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        let repo = Repository::init(root).expect("git init should succeed");
        fs::write(root.join("changed.txt"), "v1").expect("file should write");
        commit_all(&repo, "initial");
        fs::write(root.join("changed.txt"), "v2").expect("file should update");

        let app = App::new(root.to_path_buf(), TreeMode::Changed).expect("app should build");

        assert_eq!(app.tree.mode, TreeMode::Changed);
        assert_eq!(app.tree.entries.len(), 1);
        assert_eq!(app.tree.entries[0].name, "changed.txt");
    }

    #[test]
    fn toggle_tree_mode_cycles_normal_and_changed() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        let repo = Repository::init(root).expect("git init should succeed");
        fs::write(root.join("changed.txt"), "v1").expect("file should write");
        fs::write(root.join("clean.txt"), "clean").expect("file should write");
        commit_all(&repo, "initial");
        fs::write(root.join("changed.txt"), "v2").expect("file should update");

        let mut app = App::new(root.to_path_buf(), TreeMode::Normal).expect("app should build");
        assert_eq!(app.tree.mode, TreeMode::Normal);
        assert!(app
            .tree
            .entries
            .iter()
            .any(|entry| entry.name == "changed.txt"));
        assert!(app
            .tree
            .entries
            .iter()
            .any(|entry| entry.name == "clean.txt"));

        let _ = app.handle_command(Command::ToggleTreeMode);
        assert_eq!(app.tree.mode, TreeMode::Changed);
        assert_eq!(app.tree.entries.len(), 1);
        assert_eq!(app.tree.entries[0].name, "changed.txt");

        let _ = app.handle_command(Command::ToggleTreeMode);
        assert_eq!(app.tree.mode, TreeMode::Normal);
        assert!(app
            .tree
            .entries
            .iter()
            .any(|entry| entry.name == "changed.txt"));
        assert!(app
            .tree
            .entries
            .iter()
            .any(|entry| entry.name == "clean.txt"));
    }

    #[test]
    fn toggle_tree_mode_keeps_current_directory_when_changed_entries_exist() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        let repo = Repository::init(root).expect("git init should succeed");
        fs::create_dir_all(root.join("src/nested")).expect("dirs should create");
        fs::write(root.join("src/nested/file.txt"), "v1").expect("file should write");
        commit_all(&repo, "initial");
        fs::write(root.join("src/nested/file.txt"), "v2").expect("file should update");

        let mut app = App::new(root.to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "src");
        let _ = app.handle_command(Command::ExpandOrOpen);

        assert_eq!(app.tree.current_dir, root.join("src"));

        let _ = app.handle_command(Command::ToggleTreeMode);

        assert_eq!(app.tree.mode, TreeMode::Changed);
        assert_eq!(app.tree.current_dir, root.join("src"));
        assert_eq!(app.tree.entries.len(), 2);
        assert_eq!(app.tree.entries[0].name, "..");
        assert_eq!(app.tree.entries[1].name, "nested");
    }

    #[test]
    fn tree_left_click_selects_file_and_moves_focus_to_preview() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        fs::write(tmp.path().join("b.txt"), "b").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let terminal_area = Rect::new(0, 0, 20, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        let effect = app.handle_tree_left_click(terminal_area, tree_area.x + 1, tree_area.y + 2);

        assert_eq!(effect, None);
        assert_eq!(app.tree.selected_path(), tmp.path().join("b.txt").as_path());
        assert!(app.is_preview_focused());
    }

    #[test]
    fn tree_left_click_expands_directory() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::create_dir_all(tmp.path().join("sub")).expect("create dir should succeed");
        fs::write(tmp.path().join("sub/note.txt"), "hello").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let terminal_area = Rect::new(0, 0, 20, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        let effect = app.handle_tree_left_click(terminal_area, tree_area.x + 1, tree_area.y + 1);

        assert_eq!(effect, None);
        assert_eq!(app.tree.current_dir, tmp.path().join("sub"));
        assert_eq!(app.tree.entries.len(), 2);
        assert_eq!(app.tree.entries[0].name, "..");
        assert_eq!(app.tree.entries[1].name, "note.txt");
        assert!(app.is_tree_focused());
    }

    #[test]
    fn expand_or_open_on_parent_link_moves_back_to_parent_directory() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dir should succeed");

        let mut app = App::new(root.clone(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "sub");
        let _ = app.handle_command(Command::ExpandOrOpen);
        assert_eq!(app.tree.current_dir, root.join("sub"));

        assert!(app.tree.select_index(0));
        let _ = app.handle_command(Command::ExpandOrOpen);

        assert_eq!(app.tree.current_dir, root);
    }

    #[test]
    fn tree_left_click_ignores_outside_tree_content() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let before = app.tree.selected_path().to_path_buf();

        let effect = app.handle_tree_left_click(Rect::new(0, 0, 20, 5), 0, 0);

        assert_eq!(effect, None);
        assert_eq!(app.tree.selected_path(), before.as_path());
        assert!(app.is_tree_focused());
    }

    #[test]
    fn tree_left_click_in_preview_empty_space_returns_focus_to_tree() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "a.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        let terminal_area = Rect::new(0, 0, 20, 10);
        let tree_area = ui::tree_area(terminal_area, &app);
        let before_path = app.tree.selected_path().to_path_buf();
        let before_scroll = app.preview.scroll;

        let effect = app.handle_tree_left_click(
            terminal_area,
            tree_area.right() - 2,
            tree_area.bottom() - 2,
        );

        assert_eq!(effect, None);
        assert!(app.is_tree_focused());
        assert_eq!(app.tree.selected_path(), before_path.as_path());
        assert_eq!(app.preview.scroll, before_scroll);
    }

    #[test]
    fn tree_left_click_on_border_in_preview_returns_focus_to_tree() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "a.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        let terminal_area = Rect::new(0, 0, 20, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        let effect = app.handle_tree_left_click(terminal_area, tree_area.x, tree_area.y);

        assert_eq!(effect, None);
        assert!(app.is_tree_focused());
    }

    #[test]
    fn tree_left_click_on_item_in_preview_only_returns_focus_to_tree() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        fs::write(tmp.path().join("b.txt"), "b").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "a.txt");
        let _ = app.handle_command(Command::ExpandOrOpen);
        let terminal_area = Rect::new(0, 0, 20, 10);
        let tree_area = ui::tree_area(terminal_area, &app);
        let before_path = app.tree.selected_path().to_path_buf();

        let effect = app.handle_tree_left_click(terminal_area, tree_area.x + 1, tree_area.y + 2);

        assert_eq!(effect, None);
        assert!(app.is_tree_focused());
        assert_eq!(app.tree.selected_path(), before_path.as_path());
    }

    #[test]
    fn tree_hover_tracks_non_selected_row() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        fs::write(tmp.path().join("b.txt"), "b").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let terminal_area = Rect::new(0, 0, 20, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        app.update_tree_hover(terminal_area, tree_area.x + 1, tree_area.y + 2);

        assert_eq!(app.hovered_tree_index, Some(1));
    }

    #[test]
    fn tree_hover_clears_outside_tree_content() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.hovered_tree_index = Some(0);

        app.update_tree_hover(Rect::new(0, 0, 20, 10), 0, 0);

        assert_eq!(app.hovered_tree_index, None);
    }

    #[test]
    fn changed_tree_mode_reports_empty_state() {
        let tmp = tempdir().expect("tmpdir should exist");
        let app = App::new(tmp.path().to_path_buf(), TreeMode::Changed).expect("app should build");

        assert!(app.tree.entries.is_empty());
        assert_eq!(app.status_message, "changed tree: no files");
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
            let _ = app.handle_command(Command::MoveDown);
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
