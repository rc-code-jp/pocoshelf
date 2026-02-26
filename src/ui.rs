use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, TREE_RATIO_PERCENT};
use crate::git_status::GitState;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    let body = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(TREE_RATIO_PERCENT),
            Constraint::Percentage(100 - TREE_RATIO_PERCENT),
        ])
        .split(outer[0]);

    render_tree(frame, app, body[0]);
    render_preview(frame, app, body[1]);
    render_status(frame, app, outer[1]);
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
    for (absolute_index, node) in app.tree.entries[scroll_offset..end_index].iter().enumerate() {
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
    let start = app.preview.scroll.min(app.preview.lines.len().saturating_sub(1));
    let end = (start + height).min(app.preview.lines.len());

    let mut lines: Vec<Line<'_>> = app.preview.lines[start..end]
        .iter()
        .map(|line| {
            let style = if app.preview.is_diff_view() {
                style_for_diff_line(line)
            } else {
                Style::default()
            };
            let sanitized = line.replace('\t', "    ");
            let padded = format!("{:<width$}", sanitized, width = inner_width);
            Line::from(Span::styled(padded, style))
        })
        .collect();

    let blank = " ".repeat(inner_width);
    while lines.len() < height {
        lines.push(Line::from(Span::raw(blank.clone())));
    }

    let mut block = Block::default().title(app.preview_title()).borders(Borders::ALL);
    if app.is_preview_focused() {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }
    let preview = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, area);
    frame.render_widget(preview, area);
}

fn render_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let status = format!(
        "{} | selected: {} | focus: {} | keys: j/k h/l enter/esc arrows r y q",
        app.status_message,
        app.tree.selected_path().display(),
        if app.is_tree_focused() { "tree" } else { "preview" }
    );

    let line = Line::from(Span::styled(status, Style::default().fg(Color::DarkGray)));
    frame.render_widget(Paragraph::new(line), area);
}

fn style_for_diff_line(line: &str) -> Style {
    if line.starts_with('+') && !line.starts_with("+++") {
        return Style::default().fg(Color::Green).bg(Color::Rgb(20, 45, 20));
    }

    if line.starts_with('-') && !line.starts_with("---") {
        return Style::default().fg(Color::Red).bg(Color::Rgb(50, 20, 20));
    }

    if line.starts_with(' ') {
        return Style::default().fg(Color::Gray).bg(Color::Rgb(30, 30, 30));
    }

    Style::default()
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
