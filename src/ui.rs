use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::git_status::GitState;
use crate::preview::PreviewRenderMode;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    let tree_ratio = if app.is_preview_focused() {
        app.config.layout.tree_ratio_preview_focused
    } else {
        app.config.layout.tree_ratio_normal
    };

    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(tree_ratio),
            Constraint::Percentage(100 - tree_ratio),
        ])
        .split(outer[0]);

    render_tree(frame, app, body[0]);
    render_preview(frame, app, body[1]);
    render_status(frame, outer[1]);
    if app.show_help {
        render_help(frame, frame.area());
    }
}

fn render_tree(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let viewport_height = area.height.saturating_sub(2) as usize;
    let selected_index = app.tree.selected_index();
    let scroll_offset = if viewport_height == 0 || selected_index < viewport_height {
        0
    } else {
        selected_index - viewport_height + 1
    };

    let end_index = app
        .tree
        .entries
        .len()
        .min(scroll_offset.saturating_add(viewport_height.max(1)));

    let mut lines = Vec::with_capacity(end_index.saturating_sub(scroll_offset));
    for (absolute_index, node) in app.tree.entries[scroll_offset..end_index]
        .iter()
        .enumerate()
    {
        let absolute_index = scroll_offset + absolute_index;
        let label = if node.is_dir {
            format!("{}/", node.name)
        } else {
            node.name.clone()
        };

        let mut style = style_for_git(app.selected_git_state(&node.path, node.is_dir));
        if absolute_index == selected_index {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }

        lines.push(Line::from(Span::styled(label, style)));
    }

    let title = format!("Dir: {}", app.tree.current_dir.display());
    let mut block = Block::default().title(title).borders(Borders::ALL);
    if app.is_tree_focused() {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }
    let tree = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, area);
    frame.render_widget(tree, area);
}

fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let line_no_width = app.preview.lines.len().max(1).to_string().len().max(3);
    let start = app
        .preview
        .scroll
        .min(app.preview.lines.len().saturating_sub(1));
    let end = (start + height).min(app.preview.lines.len());

    let mut lines: Vec<Line<'_>> = Vec::with_capacity(end.saturating_sub(start));
    for (offset, line) in app.preview.lines[start..end].iter().enumerate() {
        let absolute_index = start + offset;
        let style = match app.preview.render_mode {
            PreviewRenderMode::Diff => {
                style_for_diff_line(app.preview.is_changed_line(absolute_index))
            }
            PreviewRenderMode::Raw => Style::default(),
        };
        let sanitized = line.replace('\t', "    ");
        let numbered = format!(
            "{:>line_no_width$} | {}",
            absolute_index + 1,
            sanitized,
            line_no_width = line_no_width
        );
        let padded = format!("{:<width$}", numbered, width = inner_width);
        lines.push(Line::from(Span::styled(padded, style)));
    }

    let blank = " ".repeat(inner_width);
    while lines.len() < height {
        lines.push(Line::from(Span::raw(blank.clone())));
    }

    let mut block = Block::default()
        .title(app.preview_title())
        .borders(Borders::ALL);
    if app.is_preview_focused() {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }
    let preview = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, area);
    frame.render_widget(preview, area);
}

fn render_status(frame: &mut Frame<'_>, area: Rect) {
    let line = Line::from(Span::styled(
        "?: help",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(line), area);
}

fn style_for_diff_line(changed: bool) -> Style {
    if !changed {
        return Style::default().fg(Color::Gray).bg(Color::Rgb(30, 30, 30));
    }

    Style::default()
        .fg(Color::Yellow)
        .bg(Color::Rgb(55, 45, 10))
        .add_modifier(Modifier::BOLD)
}

fn style_for_git(state: GitState) -> Style {
    match state {
        GitState::Clean => Style::default().fg(Color::White),
        GitState::Untracked => Style::default().fg(Color::Blue),
        GitState::Added => Style::default().fg(Color::Green),
        GitState::Modified => Style::default().fg(Color::Yellow),
        GitState::Deleted => Style::default().fg(Color::Red),
    }
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(76, 80, area);
    let help_lines = vec![
        Line::from("Navigation"),
        Line::from("  j / k, Down / Up      Move selection or preview scroll"),
        Line::from("  h / Left / Esc         Collapse dir or move focus back to tree"),
        Line::from("  l / Right / Enter      Expand dir or open file preview"),
        Line::from(""),
        Line::from("Preview"),
        Line::from("  Ctrl+u / PageUp        Scroll preview up"),
        Line::from("  Ctrl+d / PageDown      Scroll preview down"),
        Line::from("  v                      Toggle preview mode (raw <-> diff)"),
        Line::from("  n / N                  Jump to next / previous change in diff mode"),
        Line::from(""),
        Line::from("General"),
        Line::from("  r                      Refresh git status"),
        Line::from("  y                      Copy @-relative path"),
        Line::from("  q                      Quit"),
        Line::from("  ? / F1                 Toggle this help"),
        Line::from(""),
        Line::from("Close help: Esc, h, or ?"),
    ];

    let block = Block::default()
        .title("Help")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(help_lines).block(block), popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);

    horizontal[1]
}
