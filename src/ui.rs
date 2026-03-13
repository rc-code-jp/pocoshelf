use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::git_status::GitState;
use crate::preview::PreviewKind;
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
    render_status(frame, app, outer[1]);
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
        let mut style = style_for_git(app.selected_git_state(&node.path, node.is_dir));
        if absolute_index == selected_index {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }

        let line = if node.is_dir {
            Line::from(Span::styled(format!("{}/", node.name), style))
        } else if node.is_symlink {
            let target_text = std::fs::read_link(&node.path)
                .map(|t| format!(" → {}", t.display()))
                .unwrap_or_default();
            let mut target_style = Style::default().fg(Color::Gray);
            if absolute_index == selected_index {
                target_style = target_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(node.name.clone(), style),
                Span::styled(target_text, target_style),
            ])
        } else {
            Line::from(Span::styled(node.name.clone(), style))
        };

        lines.push(line);
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
    let start = app
        .preview
        .scroll
        .min(app.preview.lines.len().saturating_sub(1));

    let mut lines: Vec<Line<'_>> = Vec::with_capacity(height);
    let line_no_width = app.preview.lines.len().max(1).to_string().len().max(3);
    for absolute_index in start..app.preview.lines.len() {
        if lines.len() >= height {
            break;
        }

        let line = &app.preview.lines[absolute_index];
        match app.preview.kind {
            PreviewKind::Directory => {
                let entry = &app.preview.directory_entries[absolute_index];
                let style = style_for_git(app.selected_git_state(&entry.path, entry.is_dir));
                let padded = format!("{:<width$}", line, width = inner_width);
                lines.push(Line::from(Span::styled(padded, style)));
            }
            PreviewKind::Text | PreviewKind::Message => {
                let style = match app.preview.render_mode {
                    PreviewRenderMode::Diff => {
                        style_for_diff_line(app.preview.is_changed_line(absolute_index))
                    }
                    PreviewRenderMode::Raw => Style::default(),
                };

                for visual_line in
                    wrap_numbered_preview_line(absolute_index + 1, line, line_no_width, inner_width)
                {
                    if lines.len() >= height {
                        break;
                    }
                    lines.push(Line::from(Span::styled(visual_line, style)));
                }
            }
        }
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

fn wrap_numbered_preview_line(
    line_number: usize,
    line: &str,
    line_no_width: usize,
    inner_width: usize,
) -> Vec<String> {
    if inner_width == 0 {
        return Vec::new();
    }

    let prefix = format!(
        "{:>line_no_width$} | ",
        line_number,
        line_no_width = line_no_width
    );
    let continuation_prefix = format!("{:>line_no_width$} | ", "", line_no_width = line_no_width);

    if inner_width <= prefix.len() {
        return vec![truncate_to_width(&prefix, inner_width)];
    }

    let content_width = inner_width - prefix.len();
    let sanitized = line.replace('\t', "    ");
    let wrapped_content = wrap_text_chunks(&sanitized, content_width);
    let mut visual_lines = Vec::with_capacity(wrapped_content.len().max(1));

    for (index, chunk) in wrapped_content.iter().enumerate() {
        let current_prefix = if index == 0 {
            prefix.as_str()
        } else {
            continuation_prefix.as_str()
        };
        let combined = format!("{current_prefix}{chunk}");
        visual_lines.push(format!("{combined:<width$}", width = inner_width));
    }

    visual_lines
}

fn wrap_text_chunks(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    if text.is_empty() {
        return vec![String::new()];
    }

    let chars: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + width).min(chars.len());
        chunks.push(chars[start..end].iter().collect());
        start = end;
    }

    chunks
}

fn truncate_to_width(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn render_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let status_text = format!("{} | ?: help", app.status_message);
    let line = Line::from(Span::styled(
        status_text,
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(line), area);
}

fn style_for_diff_line(changed: bool) -> Style {
    if !changed {
        return Style::default().fg(Color::DarkGray);
    }

    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

fn style_for_git(state: GitState) -> Style {
    match state {
        GitState::Clean => Style::default(),
        GitState::Ignored => Style::default().fg(Color::Gray),
        GitState::Untracked => Style::default().fg(Color::Cyan),
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
        Line::from("  h / Left               Collapse dir or move focus back to tree"),
        Line::from("  l / Right / Enter      Expand dir or open file preview"),
        Line::from(""),
        Line::from("Preview"),
        Line::from("  Ctrl+u / PageUp        Scroll preview up"),
        Line::from("  Ctrl+d / PageDown      Scroll preview down"),
        Line::from("  p                      Toggle preview mode (raw <-> diff)"),
        Line::from("  n / N                  Jump to next / previous change in diff mode"),
        Line::from(""),
        Line::from("General"),
        Line::from("  r                      Refresh git status"),
        Line::from("  c                      Copy @-relative path"),
        Line::from("  v                      Open selected file in vi"),
        Line::from("  o                      Open selected location in Finder"),
        Line::from("  q / Esc                Quit"),
        Line::from("  ? / F1                 Toggle this help"),
        Line::from(""),
        Line::from("Close help: h or ?"),
    ];

    let block = Block::default()
        .title("Help")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(help_lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        popup,
    );
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

#[cfg(test)]
mod tests {
    use super::wrap_numbered_preview_line;

    #[test]
    fn wrap_numbered_preview_line_keeps_line_number_only_on_first_visual_line() {
        let lines = wrap_numbered_preview_line(12, "abcdefghijkl", 3, 10);

        assert_eq!(lines, vec![" 12 | abcd", "    | efgh", "    | ijkl"]);
    }

    #[test]
    fn wrap_numbered_preview_line_expands_tabs_before_wrapping() {
        let lines = wrap_numbered_preview_line(1, "\ta", 3, 10);

        assert_eq!(lines, vec!["  1 |     ", "    | a   "]);
    }
}
