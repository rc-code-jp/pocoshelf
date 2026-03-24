mod app;
mod config;
mod git_status;
mod input;
mod tree;
mod ui;

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{
    self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use crate::app::{App, AppEffect};
use crate::input::map_event;
use crate::tree::TreeMode;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Startup directory. Defaults to current directory.
    path: Option<PathBuf>,
    /// Initial tree mode.
    #[arg(long, value_enum, default_value_t = TreeMode::Normal)]
    tree_mode: TreeMode,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let startup_root = resolve_startup_root(args.path)?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableFocusChange,
        EnableMouseCapture
    )?;
    let _cleanup = TerminalCleanup;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = run(&mut terminal, startup_root, args.tree_mode);
    let cursor_result = terminal.show_cursor();

    match (run_result, cursor_result) {
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err.into()),
        (Ok(_), Ok(_)) => Ok(()),
    }
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    startup_root: PathBuf,
    initial_tree_mode: TreeMode,
) -> Result<()> {
    let mut app = App::new(startup_root, initial_tree_mode)?;

    while !app.should_quit {
        app.poll_background_tasks();
        terminal.draw(|f| {
            app.set_tree_viewport_size(
                ui::tree_area(f.area(), &app).height.saturating_sub(2) as usize
            );
            app.set_help_viewport_size(
                ui::help_viewport_width(f.area()),
                ui::help_viewport_height(f.area()),
            );
            ui::render(f, &app);
        })?;

        if event::poll(EVENT_POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key_event) => {
                    if let Some(command) = map_event(key_event) {
                        if let Some(effect) = app.handle_command(command) {
                            match effect {
                                AppEffect::OpenInVi(path) => match open_in_vi(terminal, &path) {
                                    Ok(()) => {
                                        app.set_external_status(format!(
                                            "opened in vi: {}",
                                            path.display()
                                        ));
                                    }
                                    Err(err) => {
                                        app.set_external_status(format!("open failed: {err}"));
                                    }
                                },
                            }
                        }
                    }
                }
                Event::FocusGained => app.on_focus_gained(),
                Event::Mouse(mouse_event) => {
                    let terminal_size = terminal.size()?;
                    let terminal_area = Rect::new(0, 0, terminal_size.width, terminal_size.height);

                    if app.context_menu.is_some() {
                        if matches!(mouse_event.kind, MouseEventKind::Down(MouseButton::Left)) {
                            if let Some(effect) = app.handle_context_menu_left_click(
                                terminal_area,
                                mouse_event.column,
                                mouse_event.row,
                            ) {
                                match effect {
                                    AppEffect::OpenInVi(path) => {
                                        match open_in_vi(terminal, &path) {
                                            Ok(()) => {
                                                app.set_external_status(format!(
                                                    "opened in vi: {}",
                                                    path.display()
                                                ));
                                            }
                                            Err(err) => {
                                                app.set_external_status(format!(
                                                    "open failed: {err}"
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        } else if matches!(mouse_event.kind, MouseEventKind::Moved) {
                            app.update_context_menu_hover(
                                terminal_area,
                                mouse_event.column,
                                mouse_event.row,
                            );
                        } else if matches!(mouse_event.kind, MouseEventKind::Down(MouseButton::Right)) {
                            app.context_menu = None;
                        }
                    } else if matches!(mouse_event.kind, MouseEventKind::Down(MouseButton::Left)) {
                        if let Some(effect) = app.handle_tree_left_click(
                            terminal_area,
                            mouse_event.column,
                            mouse_event.row,
                        ) {
                            match effect {
                                AppEffect::OpenInVi(path) => match open_in_vi(terminal, &path) {
                                    Ok(()) => {
                                        app.set_external_status(format!(
                                            "opened in vi: {}",
                                            path.display()
                                        ));
                                    }
                                    Err(err) => {
                                        app.set_external_status(format!("open failed: {err}"));
                                    }
                                },
                            }
                        }
                    } else if matches!(mouse_event.kind, MouseEventKind::Down(MouseButton::Right)) {
                        app.handle_tree_right_click(
                            terminal_area,
                            mouse_event.column,
                            mouse_event.row,
                        );
                    } else if matches!(mouse_event.kind, MouseEventKind::ScrollUp) {
                        app.handle_mouse_wheel(
                            terminal_area,
                            mouse_event.column,
                            mouse_event.row,
                            true,
                        );
                    } else if matches!(mouse_event.kind, MouseEventKind::ScrollDown) {
                        app.handle_mouse_wheel(
                            terminal_area,
                            mouse_event.column,
                            mouse_event.row,
                            false,
                        );
                    } else if matches!(mouse_event.kind, MouseEventKind::Moved) {
                        app.update_tree_hover(terminal_area, mouse_event.column, mouse_event.row);
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn open_in_vi(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, path: &Path) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let vi_result = ProcessCommand::new("vi").arg(path).status();

    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;

    match vi_result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => anyhow::bail!("vi exited with status: {status}"),
        Err(err) => anyhow::bail!("failed to launch vi: {err}"),
    }
}

fn resolve_startup_root(path: Option<PathBuf>) -> Result<PathBuf> {
    let candidate = match path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };

    let canonical = std::fs::canonicalize(candidate)?;
    if !canonical.is_dir() {
        anyhow::bail!("startup path must be a directory");
    }

    Ok(canonical)
}

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            DisableMouseCapture,
            DisableFocusChange,
            LeaveAlternateScreen
        );
    }
}
