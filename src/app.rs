use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use notify::{RecursiveMode, Watcher};
use ratatui::layout::Rect;

use crate::config::{Config, HelpLanguage};
use crate::git_status::{collect_ignored_paths, GitSnapshot, GitState};
use crate::tree::{Tree, TreeMode};
use crate::ui;

const COPY_STATUS_DURATION: Duration = Duration::from_secs(3);
const FS_REFRESH_DEBOUNCE: Duration = Duration::from_millis(300);
const HELP_WHEEL_SCROLL_AMOUNT: usize = 3;
const TREE_WHEEL_SCROLL_AMOUNT: usize = 3;

impl HelpLanguage {
    pub fn toggle(&mut self) {
        *self = match self {
            Self::Ja => Self::En,
            Self::En => Self::Ja,
        };
    }
}

pub struct HelpState {
    pub visible: bool,
    pub language: HelpLanguage,
    pub scroll: usize,
    viewport_height: usize,
    viewport_width: usize,
}

impl HelpState {
    fn new(language: HelpLanguage) -> Self {
        Self {
            visible: false,
            language,
            scroll: 0,
            viewport_height: 1,
            viewport_width: 1,
        }
    }

    fn open(&mut self) {
        self.visible = true;
        self.scroll = 0;
    }

    fn close(&mut self) {
        self.visible = false;
    }

    fn set_viewport_size(&mut self, width: usize, height: usize) {
        self.viewport_width = width.max(1);
        self.viewport_height = height.max(1);
    }

    fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    fn scroll_down(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_add(amount);
    }

    fn clamp_scroll(&mut self) {
        let max_scroll =
            ui::help_max_scroll(self.language, self.viewport_height, self.viewport_width);
        self.scroll = self.scroll.min(max_scroll);
    }
}

pub struct ContextMenu {
    pub position: (u16, u16),
    pub selected: usize,
    pub hovered: Option<usize>,
}

impl ContextMenu {
    pub const ITEM_COUNT: usize = 5;

    fn new(column: u16, row: u16) -> Self {
        Self {
            position: (column, row),
            selected: 0,
            hovered: None,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < Self::ITEM_COUNT {
            self.selected += 1;
        }
    }
}

pub struct App {
    pub startup_root: PathBuf,
    pub tree: Tree,
    pub hovered_tree_index: Option<usize>,
    pub git: GitSnapshot,
    visible_ignored_paths: HashSet<PathBuf>,
    pub status_message: String,
    pub last_git_refresh: Instant,
    pub should_quit: bool,
    pub help: HelpState,
    pub context_menu: Option<ContextMenu>,
    clipboard: Option<Clipboard>,
    status_expires_at: Option<Instant>,
    git_refresh_tx: Sender<GitSnapshot>,
    git_refresh_rx: Receiver<GitSnapshot>,
    fs_refresh_rx: Receiver<()>,
    _fs_watcher: notify::RecommendedWatcher,
    git_refresh_in_flight: bool,
    pending_manual_refresh: bool,
    pending_fs_refresh: bool,
    last_fs_event_at: Option<Instant>,
    tree_scroll: usize,
    tree_viewport_height: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum Command {
    MoveUp,
    MoveDown,
    ExpandOrOpen,
    Collapse,
    RefreshGit,
    ToggleTreeMode,
    ToggleHelp,
    ToggleHelpLanguage,
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
        let help_language = config.help.language;
        let git = GitSnapshot::collect(&startup_root);
        let tree = Tree::new(startup_root.clone(), initial_tree_mode, &git)?;
        let visible_ignored_paths = collect_ignored_paths(
            &startup_root,
            tree.entries.iter().map(|entry| entry.path.as_path()),
        );
        let (git_refresh_tx, git_refresh_rx) = mpsc::channel();
        let (fs_refresh_tx, fs_refresh_rx) = mpsc::channel();

        let mut fs_watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
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
            startup_root,
            tree,
            hovered_tree_index: None,
            git,
            visible_ignored_paths,
            status_message: String::from("ready"),
            last_git_refresh: Instant::now(),
            should_quit: false,
            help: HelpState::new(help_language),
            context_menu: None,
            clipboard: Clipboard::new().ok(),
            status_expires_at: None,
            git_refresh_tx,
            git_refresh_rx,
            fs_refresh_rx,
            _fs_watcher: fs_watcher,
            git_refresh_in_flight: false,
            pending_manual_refresh: false,
            pending_fs_refresh: false,
            last_fs_event_at: None,
            tree_scroll: 0,
            tree_viewport_height: 1,
        };
        app.update_changed_empty_status();
        Ok(app)
    }

    pub fn handle_command(&mut self, command: Command) -> Option<AppEffect> {
        self.poll_background_tasks();

        if self.context_menu.is_some() {
            match command {
                Command::MoveUp => {
                    if let Some(menu) = self.context_menu.as_mut() {
                        menu.move_up();
                    }
                }
                Command::MoveDown => {
                    if let Some(menu) = self.context_menu.as_mut() {
                        menu.move_down();
                    }
                }
                Command::ExpandOrOpen => return self.execute_context_menu_selection(),
                Command::Quit | Command::Collapse => self.context_menu = None,
                _ => {}
            }
            return None;
        }

        if self.help.visible {
            match command {
                Command::ToggleHelp | Command::Collapse => self.help.close(),
                Command::MoveUp => self.help.scroll_up(1),
                Command::MoveDown => self.help.scroll_down(1),
                Command::ToggleHelpLanguage => self.help.language.toggle(),
                Command::Quit => self.should_quit = true,
                _ => {}
            }
            self.help.clamp_scroll();
            return None;
        }

        match command {
            Command::MoveUp => {
                self.tree.move_up();
                self.ensure_tree_selection_visible();
            }
            Command::MoveDown => {
                self.tree.move_down();
                self.ensure_tree_selection_visible();
            }
            Command::ExpandOrOpen => {
                if self.tree.selected_is_dir() {
                    if let Err(err) = self.tree.expand_selected() {
                        self.status_message = format!("expand failed: {err}");
                    }
                    self.sync_tree_state();
                }
            }
            Command::Collapse => {
                let _ = self.tree.collapse_selected();
                self.sync_tree_state();
            }
            Command::RefreshGit => self.request_git_refresh(true),
            Command::ToggleTreeMode => self.toggle_tree_mode(),
            Command::ToggleHelp => {
                self.help.open();
                self.help.clamp_scroll();
            }
            Command::ToggleHelpLanguage => {}
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
        if self.help.visible {
            return None;
        }

        if !self.select_tree_index_at(terminal_area, column, row) {
            return None;
        }

        self.ensure_tree_selection_visible();

        if self.tree.selected_is_dir() {
            return self.handle_command(Command::ExpandOrOpen);
        }

        None
    }

    pub fn handle_tree_right_click(&mut self, terminal_area: Rect, column: u16, row: u16) {
        if self.help.visible {
            return;
        }

        if !self.select_tree_index_at(terminal_area, column, row) {
            return;
        }

        self.ensure_tree_selection_visible();
        self.context_menu = Some(ContextMenu::new(column, row));
    }

    pub fn handle_context_menu_left_click(
        &mut self,
        terminal_area: Rect,
        column: u16,
        row: u16,
    ) -> Option<AppEffect> {
        if let Some(index) = ui::context_menu_item_at(terminal_area, self, column, row) {
            if let Some(menu) = self.context_menu.as_mut() {
                menu.selected = index;
            }
            self.execute_context_menu_selection()
        } else {
            self.context_menu = None;
            None
        }
    }

    pub fn update_context_menu_hover(&mut self, terminal_area: Rect, column: u16, row: u16) {
        let hovered = ui::context_menu_item_at(terminal_area, self, column, row);
        if let Some(menu) = self.context_menu.as_mut() {
            menu.hovered = hovered;
        }
    }

    fn execute_context_menu_selection(&mut self) -> Option<AppEffect> {
        let selected = self.context_menu.as_ref().map(|m| m.selected).unwrap_or(0);
        self.context_menu = None;
        match selected {
            0 => self.copy_relative_path(),
            1 => self.copy_cat_command(),
            2 => self.copy_vi_command(),
            3 => return self.open_in_vi(),
            4 => {} // cancel
            _ => {}
        }
        None
    }

    fn copy_cat_command(&mut self) {
        self.copy_shell_command("cat");
    }

    fn copy_vi_command(&mut self) {
        self.copy_shell_command("vi");
    }

    fn copy_shell_command(&mut self, command: &str) {
        let selected = self.tree.selected_path();
        match format_relative_path(&self.startup_root, selected) {
            Ok(rel_path) => {
                let text = format!("{command} {rel_path}");
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

    pub fn update_tree_hover(&mut self, terminal_area: Rect, column: u16, row: u16) {
        if self.help.visible {
            self.hovered_tree_index = None;
            return;
        }

        let tree_area = ui::tree_area(terminal_area, self);
        self.hovered_tree_index = ui::tree_index_at(tree_area, self, column, row);
    }

    pub fn handle_mouse_wheel(
        &mut self,
        terminal_area: Rect,
        column: u16,
        row: u16,
        scroll_up: bool,
    ) {
        if self.help.visible {
            if !ui::help_contains(terminal_area, column, row) {
                return;
            }

            if scroll_up {
                self.help.scroll_up(HELP_WHEEL_SCROLL_AMOUNT);
            } else {
                self.help.scroll_down(HELP_WHEEL_SCROLL_AMOUNT);
            }
            self.help.clamp_scroll();
            return;
        }

        if !ui::tree_contains(terminal_area, self, column, row) {
            return;
        }

        if scroll_up {
            self.scroll_tree_up(TREE_WHEEL_SCROLL_AMOUNT);
        } else {
            self.scroll_tree_down(TREE_WHEEL_SCROLL_AMOUNT);
        }
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
        if !self.tree.selected_exists_on_disk() {
            self.set_temporary_status("deleted entry selected; open skipped");
            return;
        }

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

        if !self.tree.selected_exists_on_disk() {
            self.set_temporary_status("deleted entry selected; vi skipped");
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

    pub fn tree_title(&self) -> String {
        format!(
            "Root: {} [{}]",
            self.tree.root_label(),
            self.tree.mode.label()
        )
    }

    pub fn set_tree_viewport_size(&mut self, height: usize) {
        self.tree_viewport_height = height.max(1);
        self.clamp_tree_scroll();
    }

    pub fn set_help_viewport_size(&mut self, width: usize, height: usize) {
        self.help.set_viewport_size(width, height);
        self.help.clamp_scroll();
    }

    fn set_temporary_status(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
        self.status_expires_at = Some(Instant::now() + COPY_STATUS_DURATION);
    }

    pub fn tree_scroll(&self) -> usize {
        self.tree_scroll
    }

    fn scroll_tree_up(&mut self, amount: usize) {
        self.tree_scroll = self.tree_scroll.saturating_sub(amount);
    }

    fn scroll_tree_down(&mut self, amount: usize) {
        self.tree_scroll = self.tree_scroll.saturating_add(amount);
        self.clamp_tree_scroll();
    }

    fn clamp_tree_scroll(&mut self) {
        let max_scroll = ui::tree_max_scroll(self.tree.entries.len(), self.tree_viewport_height);
        self.tree_scroll = self.tree_scroll.min(max_scroll);
    }

    fn ensure_tree_selection_visible(&mut self) {
        let selected = self.tree.selected_index();
        if self.tree_viewport_height == 0 {
            self.tree_scroll = 0;
            return;
        }

        if selected < self.tree_scroll {
            self.tree_scroll = selected;
        } else {
            let viewport_end = self
                .tree_scroll
                .saturating_add(self.tree_viewport_height.saturating_sub(1));
            if selected > viewport_end {
                self.tree_scroll = selected
                    .saturating_add(1)
                    .saturating_sub(self.tree_viewport_height);
            }
        }

        self.clamp_tree_scroll();
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
        self.clamp_tree_scroll();
        self.ensure_tree_selection_visible();
        self.refresh_visible_ignored_paths();
        self.hovered_tree_index = self
            .hovered_tree_index
            .filter(|index| *index < self.tree.entries.len());
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

    fn select_tree_index_at(&mut self, terminal_area: Rect, column: u16, row: u16) -> bool {
        let tree_area = ui::tree_area(terminal_area, self);
        let Some(index) = ui::tree_index_at(tree_area, self, column, row) else {
            return false;
        };

        self.tree.select_index(index)
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

fn format_relative_path(startup_root: &Path, selected: &Path) -> anyhow::Result<String> {
    let relative = selected.strip_prefix(startup_root)?;

    if relative.as_os_str().is_empty() {
        return Ok(String::from("."));
    }

    Ok(normalize_to_slashes(relative))
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
    use ratatui::layout::Rect;
    use tempfile::tempdir;

    use crate::config::HelpLanguage;
    use crate::git_status::GitState;
    use crate::tree::TreeMode;
    use crate::ui;

    use super::{
        format_relative_with_at, resolve_directory_to_open, App, AppEffect, Command,
        FS_REFRESH_DEBOUNCE, TREE_WHEEL_SCROLL_AMOUNT,
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
    fn expand_or_open_does_nothing_for_file() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("note.txt"), "hello").expect("write should succeed");

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "note.txt");
        let before = app.tree.entries.clone();

        let _ = app.handle_command(Command::ExpandOrOpen);

        assert_eq!(app.tree.entries.len(), before.len());
        assert_eq!(
            app.tree.selected_path(),
            tmp.path().join("note.txt").as_path()
        );
    }

    #[test]
    fn tree_wheel_scrolls_down_by_three_lines_without_changing_selection() {
        let tmp = tempdir().expect("tmpdir should exist");
        for index in 0..10 {
            fs::write(tmp.path().join(format!("file-{index}.txt")), "x")
                .expect("write should succeed");
        }

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.set_tree_viewport_size(3);
        let selected_before = app.tree.selected_index();
        let terminal_area = Rect::new(0, 0, 20, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        app.handle_mouse_wheel(terminal_area, tree_area.x + 1, tree_area.y + 1, false);

        assert_eq!(app.tree_scroll(), TREE_WHEEL_SCROLL_AMOUNT);
        assert_eq!(app.tree.selected_index(), selected_before);
    }

    #[test]
    fn tree_wheel_ignores_non_tree_region() {
        let tmp = tempdir().expect("tmpdir should exist");
        for index in 0..10 {
            fs::write(tmp.path().join(format!("file-{index}.txt")), "x")
                .expect("write should succeed");
        }

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.set_tree_viewport_size(3);
        let before_scroll = app.tree_scroll();

        app.handle_mouse_wheel(Rect::new(0, 0, 20, 10), 30, 9, false);

        assert_eq!(app.tree_scroll(), before_scroll);
    }

    #[test]
    fn tree_selection_scrolls_into_view_after_move_down() {
        let tmp = tempdir().expect("tmpdir should exist");
        for index in 0..10 {
            fs::write(tmp.path().join(format!("file-{index}.txt")), "x")
                .expect("write should succeed");
        }

        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.set_tree_viewport_size(3);

        for _ in 0..4 {
            let _ = app.handle_command(Command::MoveDown);
        }

        assert_eq!(app.tree.selected_index(), 4);
        assert_eq!(app.tree_scroll(), 2);
    }

    #[test]
    fn help_toggles_and_blocks_navigation_commands() {
        let tmp = tempdir().expect("tmpdir should exist");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let before = app.tree.selected_path().to_path_buf();

        let _ = app.handle_command(Command::ToggleHelp);
        assert!(app.help.visible);

        let _ = app.handle_command(Command::MoveDown);
        assert_eq!(app.tree.selected_path(), before.as_path());

        let _ = app.handle_command(Command::ToggleHelp);
        assert!(!app.help.visible);
    }

    #[test]
    fn help_language_toggles_only_in_help() {
        let tmp = tempdir().expect("tmpdir should exist");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");

        app.help.language = HelpLanguage::En;
        let _ = app.handle_command(Command::ToggleHelpLanguage);
        assert_eq!(app.help.language, HelpLanguage::En);

        let _ = app.handle_command(Command::ToggleHelp);
        let _ = app.handle_command(Command::ToggleHelpLanguage);
        assert_eq!(app.help.language, HelpLanguage::Ja);
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
    fn open_in_vi_skips_deleted_file_selection() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        let repo = Repository::init(root).expect("git init should succeed");
        fs::write(root.join("gone.txt"), "hello").expect("write should succeed");
        commit_all(&repo, "initial");
        fs::remove_file(root.join("gone.txt")).expect("delete should succeed");

        let mut app = App::new(root.to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "gone.txt");

        let effect = app.handle_command(Command::OpenInVi);
        assert_eq!(effect, None);
        assert_eq!(app.status_message, "deleted entry selected; vi skipped");
    }

    #[test]
    fn open_in_finder_skips_deleted_file_selection() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        let repo = Repository::init(root).expect("git init should succeed");
        fs::write(root.join("gone.txt"), "hello").expect("write should succeed");
        commit_all(&repo, "initial");
        fs::remove_file(root.join("gone.txt")).expect("delete should succeed");

        let mut app = App::new(root.to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "gone.txt");

        let _ = app.handle_command(Command::OpenInFinder);
        assert_eq!(app.status_message, "deleted entry selected; open skipped");
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
    fn selected_git_state_refreshes_when_entering_directory() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        Repository::init(root).expect("git init should succeed");
        fs::write(root.join(".gitignore"), "nested/ignored.log\n").expect("gitignore should write");
        fs::create_dir_all(root.join("nested")).expect("nested dir should create");
        fs::write(root.join("nested/ignored.log"), "skip").expect("ignored file should write");

        let mut app = App::new(root.to_path_buf(), TreeMode::Normal).expect("app should build");
        let nested_index = app
            .tree
            .entries
            .iter()
            .position(|entry| entry.name == "nested")
            .expect("nested should exist");
        assert!(app.tree.select_index(nested_index));
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
            .any(|entry| entry.name == "clean.txt"));
    }

    #[test]
    fn tree_left_click_selects_file_without_other_side_effects() {
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
        assert_eq!(app.status_message, "ready");
    }

    #[test]
    fn tree_left_click_on_same_file_does_not_copy() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        fs::write(tmp.path().join("b.txt"), "b").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.clipboard = None;
        let terminal_area = Rect::new(0, 0, 20, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        let _ = app.handle_tree_left_click(terminal_area, tree_area.x + 1, tree_area.y + 2);
        let effect = app.handle_tree_left_click(terminal_area, tree_area.x + 1, tree_area.y + 2);

        assert_eq!(effect, None);
        assert_eq!(app.status_message, "ready");
    }

    #[test]
    fn tree_click_on_different_file_does_not_copy() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        fs::write(tmp.path().join("b.txt"), "b").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let terminal_area = Rect::new(0, 0, 20, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        let _ = app.handle_tree_left_click(terminal_area, tree_area.x + 1, tree_area.y + 1);
        let _ = app.handle_tree_left_click(terminal_area, tree_area.x + 1, tree_area.y + 2);

        assert_eq!(app.status_message, "ready");
        assert_eq!(app.tree.selected_path(), tmp.path().join("b.txt").as_path());
    }

    #[test]
    fn tree_right_click_opens_context_menu_for_deleted_file() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path();
        let repo = Repository::init(root).expect("git init should succeed");
        fs::write(root.join("gone.txt"), "hello").expect("write should succeed");
        commit_all(&repo, "initial");
        fs::remove_file(root.join("gone.txt")).expect("delete should succeed");

        let mut app = App::new(root.to_path_buf(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "gone.txt");
        let index = app.tree.selected_index();
        let terminal_area = Rect::new(0, 0, 40, 10);
        let tree_area = ui::tree_area(terminal_area, &app);
        let row = tree_area.y + 1 + index as u16;

        app.handle_tree_right_click(terminal_area, tree_area.x + 1, row);

        assert!(app.context_menu.is_some());
        assert_eq!(app.context_menu.as_ref().unwrap().selected, 0);
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
        assert_eq!(app.tree.entries.len(), 2);
        assert_eq!(app.tree.entries[1].name, "note.txt");
    }

    #[test]
    fn tree_right_click_opens_context_menu() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        fs::write(tmp.path().join("b.txt"), "b").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let terminal_area = Rect::new(0, 0, 40, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        app.handle_tree_right_click(terminal_area, tree_area.x + 1, tree_area.y + 2);

        assert!(app.context_menu.is_some());
        assert_eq!(app.tree.selected_path(), tmp.path().join("b.txt").as_path());
    }

    #[test]
    fn tree_right_click_on_directory_opens_menu_without_expand() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::create_dir_all(tmp.path().join("sub")).expect("create dir should succeed");
        fs::write(tmp.path().join("sub/note.txt"), "hello").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let terminal_area = Rect::new(0, 0, 40, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        app.handle_tree_right_click(terminal_area, tree_area.x + 1, tree_area.y + 1);

        assert_eq!(app.tree.entries.len(), 1);
        assert_eq!(app.tree.selected_path(), tmp.path().join("sub").as_path());
        assert!(app.context_menu.is_some());
    }

    #[test]
    fn context_menu_executes_copy_path_on_first_item() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let terminal_area = Rect::new(0, 0, 40, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        app.handle_tree_right_click(terminal_area, tree_area.x + 1, tree_area.y + 1);
        assert!(app.context_menu.is_some());

        // Execute first item (@ copy)
        let _ = app.handle_command(Command::ExpandOrOpen);
        assert!(app.context_menu.is_none());
        assert!(app.status_message.starts_with("copied: @"));
    }

    #[test]
    fn context_menu_executes_cat_command_on_second_item() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.clipboard = None;
        let terminal_area = Rect::new(0, 0, 40, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        app.handle_tree_right_click(terminal_area, tree_area.x + 1, tree_area.y + 1);
        let _ = app.handle_command(Command::MoveDown);
        assert_eq!(app.context_menu.as_ref().unwrap().selected, 1);

        let _ = app.handle_command(Command::ExpandOrOpen);
        assert!(app.context_menu.is_none());
        assert_eq!(app.status_message, "clipboard unavailable");
    }

    #[test]
    fn context_menu_closes_on_escape() {
        let tmp = tempdir().expect("tmpdir should exist");
        fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let terminal_area = Rect::new(0, 0, 40, 10);
        let tree_area = ui::tree_area(terminal_area, &app);

        app.handle_tree_right_click(terminal_area, tree_area.x + 1, tree_area.y + 1);
        assert!(app.context_menu.is_some());

        let _ = app.handle_command(Command::Quit);
        assert!(app.context_menu.is_none());
        assert!(!app.should_quit);
    }

    #[test]
    fn collapse_on_child_closes_parent_directory() {
        let tmp = tempdir().expect("tmpdir should exist");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("sub")).expect("create dir should succeed");
        fs::write(root.join("sub/file.txt"), "hello").expect("write should succeed");

        let mut app = App::new(root.clone(), TreeMode::Normal).expect("app should build");
        select_by_file_name(&mut app, "sub");
        let _ = app.handle_command(Command::ExpandOrOpen);
        select_by_file_name(&mut app, "file.txt");

        let _ = app.handle_command(Command::Collapse);

        assert_eq!(app.tree.selected_path(), root.join("sub").as_path());
        assert_eq!(app.tree.entries.len(), 1);
        assert!(!app.tree.entries[0].is_expanded);
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
    fn changed_tree_mode_reports_empty_state() {
        let tmp = tempdir().expect("tmpdir should exist");
        let app = App::new(tmp.path().to_path_buf(), TreeMode::Changed).expect("app should build");

        assert!(app.tree.entries.is_empty());
        assert_eq!(app.status_message, "changed tree: no files");
    }

    fn select_by_file_name(app: &mut App, name: &str) {
        let index = app
            .tree
            .entries
            .iter()
            .position(|entry| entry.name == name)
            .expect("entry should exist");
        assert!(app.tree.select_index(index));
    }

    fn commit_all(repo: &Repository, message: &str) {
        let mut index = repo.index().expect("index should exist");
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .expect("index add should succeed");
        index.write().expect("index write should succeed");
        let tree_id = index.write_tree().expect("write tree should succeed");
        let tree = repo.find_tree(tree_id).expect("tree should exist");
        let sig = Signature::now("minishelf", "minishelf@example.com").expect("sig should work");

        let parent = repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .and_then(|oid| repo.find_commit(oid).ok());

        if let Some(parent) = parent {
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
                .expect("commit should succeed");
        } else {
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
                .expect("initial commit should succeed");
        }
    }
}
