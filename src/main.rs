mod app;
mod config;
mod git_status;
mod input;
mod preview;
mod tree;
mod ui;

use std::io;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::{App, REFRESH_INTERVAL};
use crate::input::map_event;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Startup directory. Defaults to current directory.
    path: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let startup_root = resolve_startup_root(args.path)?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let _cleanup = TerminalCleanup;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = run(&mut terminal, startup_root);
    let cursor_result = terminal.show_cursor();

    match (run_result, cursor_result) {
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err.into()),
        (Ok(_), Ok(_)) => Ok(()),
    }
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, startup_root: PathBuf) -> Result<()> {
    let mut app = App::new(startup_root)?;
    let mut last_tick = Instant::now();

    while !app.should_quit {
        app.poll_background_tasks();
        terminal.draw(|f| ui::render(f, &app))?;

        let timeout = REFRESH_INTERVAL.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key_event) = event::read()? {
                if let Some(command) = map_event(key_event) {
                    app.handle_command(command);
                }
            }
        }

        if last_tick.elapsed() >= REFRESH_INTERVAL {
            app.periodic_refresh();
            last_tick = Instant::now();
        }
    }

    Ok(())
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
        let _ = execute!(stdout, LeaveAlternateScreen);
    }
}
