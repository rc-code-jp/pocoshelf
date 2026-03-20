use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::git_status::GitState;
use crate::preview::PreviewKind;
use crate::preview::PreviewRenderMode;
use crate::tree::DirEntryNode;

const TREE_COLUMN_GAP: usize = 2;
const TREE_DATE_WIDTH: usize = 10;
const TREE_MIN_NAME_WIDTH: usize = 12;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let outer = outer_layout(frame.area());

    let body = body_layout(outer[0], app);

    render_tree(frame, app, body[0]);
    render_preview(frame, app, body[1]);
    render_status(frame, app, outer[1]);
    if app.show_help {
        render_help(frame, frame.area());
    }
}

pub fn preview_viewport_height(area: Rect, app: &App) -> usize {
    let outer = outer_layout(area);
    let body = body_layout(outer[0], app);
    body[1].height.saturating_sub(2) as usize
}

pub fn tree_area(area: Rect, app: &App) -> Rect {
    let outer = outer_layout(area);
    let body = body_layout(outer[0], app);
    body[0]
}

pub fn tree_scroll_offset(viewport_height: usize, selected_index: usize) -> usize {
    if viewport_height == 0 || selected_index < viewport_height {
        0
    } else {
        selected_index - viewport_height + 1
    }
}

pub fn tree_index_at(area: Rect, app: &App, column: u16, row: u16) -> Option<usize> {
    if !area.contains(ratatui::layout::Position { x: column, y: row }) {
        return None;
    }

    let inner_x = column.checked_sub(area.x + 1)?;
    let inner_y = row.checked_sub(area.y + 1)?;
    let viewport_height = area.height.saturating_sub(2) as usize;
    if usize::from(inner_y) >= viewport_height {
        return None;
    }

    let inner_width = area.width.saturating_sub(2);
    if inner_width == 0 || inner_x >= inner_width {
        return None;
    }

    let scroll_offset = tree_scroll_offset(viewport_height, app.tree.selected_index());
    let absolute_index = scroll_offset + usize::from(inner_y);
    if absolute_index >= app.tree.entries.len() {
        return None;
    }

    Some(absolute_index)
}

fn outer_layout(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area)
}

fn body_layout(area: Rect, app: &App) -> std::rc::Rc<[Rect]> {
    let tree_ratio = if app.is_preview_focused() {
        app.config.layout.tree_ratio_preview_focused
    } else {
        app.config.layout.tree_ratio_normal
    };

    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(tree_ratio),
            Constraint::Percentage(100 - tree_ratio),
        ])
        .split(area)
}

fn render_tree(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let viewport_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let selected_index = app.tree.selected_index();
    let scroll_offset = tree_scroll_offset(viewport_height, selected_index);

    let end_index = app
        .tree
        .entries
        .len()
        .min(scroll_offset.saturating_add(viewport_height.max(1)));
    let columns = tree_columns(inner_width, &app.tree.entries);

    let mut lines = Vec::with_capacity(end_index.saturating_sub(scroll_offset));
    for (absolute_index, node) in app.tree.entries[scroll_offset..end_index]
        .iter()
        .enumerate()
    {
        let absolute_index = scroll_offset + absolute_index;
        let mut style = style_for_git(app.selected_git_state(&node.path, node.is_dir));
        if absolute_index == selected_index {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        } else if app.hovered_tree_index == Some(absolute_index) {
            style = style.bg(Color::DarkGray);
        }
        lines.push(render_tree_line(
            node,
            &columns,
            style,
            absolute_index == selected_index,
        ));
    }

    let mut block = Block::default()
        .title(app.tree_title())
        .borders(Borders::ALL);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TreeColumns {
    name_width: usize,
    size_width: Option<usize>,
    date_width: Option<usize>,
}

fn tree_columns(inner_width: usize, entries: &[DirEntryNode]) -> TreeColumns {
    if inner_width == 0 {
        return TreeColumns {
            name_width: 0,
            size_width: None,
            date_width: None,
        };
    }

    let size_width = entries
        .iter()
        .map(tree_size_text)
        .map(|text| text.chars().count())
        .max()
        .unwrap_or(1);
    let min_name_width = TREE_MIN_NAME_WIDTH.min(inner_width);
    let both_required = min_name_width + size_width + TREE_DATE_WIDTH + TREE_COLUMN_GAP * 2;
    if inner_width >= both_required {
        return TreeColumns {
            name_width: inner_width - size_width - TREE_DATE_WIDTH - TREE_COLUMN_GAP * 2,
            size_width: Some(size_width),
            date_width: Some(TREE_DATE_WIDTH),
        };
    }

    let size_required = min_name_width + size_width + TREE_COLUMN_GAP;
    if inner_width >= size_required {
        return TreeColumns {
            name_width: inner_width - size_width - TREE_COLUMN_GAP,
            size_width: Some(size_width),
            date_width: None,
        };
    }

    TreeColumns {
        name_width: inner_width,
        size_width: None,
        date_width: None,
    }
}

fn render_tree_line(
    node: &DirEntryNode,
    columns: &TreeColumns,
    style: Style,
    is_selected: bool,
) -> Line<'static> {
    let mut spans = Vec::new();
    let name_text = pad_to_width(
        &truncate_to_width(&tree_name_text(node), columns.name_width),
        columns.name_width,
    );
    spans.push(Span::styled(name_text, style));

    if let Some(size_width) = columns.size_width {
        spans.push(Span::styled(" ".repeat(TREE_COLUMN_GAP), style));
        spans.push(Span::styled(
            format!("{:>width$}", tree_size_text(node), width = size_width),
            tree_meta_style(is_selected),
        ));
    }

    if let Some(date_width) = columns.date_width {
        spans.push(Span::styled(" ".repeat(TREE_COLUMN_GAP), style));
        spans.push(Span::styled(
            format!(
                "{:>width$}",
                node.modified_date.as_deref().unwrap_or(""),
                width = date_width
            ),
            tree_meta_style(is_selected),
        ));
    }

    Line::from(spans)
}

fn tree_name_text(node: &DirEntryNode) -> String {
    if node.is_dir {
        return format!("{}/", node.name);
    }

    if node.is_symlink {
        let target_text = std::fs::read_link(&node.path)
            .map(|t| format!(" -> {}", t.display()))
            .unwrap_or_default();
        return format!("{}{}", node.name, target_text);
    }

    node.name.clone()
}

fn tree_size_text(node: &DirEntryNode) -> String {
    if node.is_dir {
        return String::from("-");
    }

    node.size_bytes.map(format_bytes).unwrap_or_default()
}

fn format_bytes(size_bytes: u64) -> String {
    let units = ["B", "K", "M", "G", "T", "P"];
    let mut value = size_bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index + 1 < units.len() {
        value /= 1024.0;
        unit_index += 1;
    }

    let unit = units[unit_index];
    if unit_index == 0 || value >= 10.0 {
        format!("{value:.0}{unit}")
    } else {
        format!("{value:.1}{unit}")
    }
}

fn tree_meta_style(is_selected: bool) -> Style {
    let mut style = Style::default().fg(Color::DarkGray);
    if is_selected {
        style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
    }
    style
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

fn pad_to_width(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.to_string();
    }

    format!("{text}{}", " ".repeat(width - len))
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
        Line::from("  Left click             Same behavior as Right / Enter in tree"),
        Line::from(""),
        Line::from("Preview"),
        Line::from("  Ctrl+u / Ctrl+d        Half-page preview scroll"),
        Line::from("  PageUp / PageDown      Full-page preview scroll"),
        Line::from("  p                      Toggle preview mode (raw <-> diff)"),
        Line::from("  n / N                  Jump to next / previous change in diff mode"),
        Line::from(""),
        Line::from("General"),
        Line::from("  Tab                    Toggle tree mode (normal <-> changed)"),
        Line::from("  r                      Refresh git status"),
        Line::from("  c                      Copy @-relative path"),
        Line::from("  v                      Open selected file in vi"),
        Line::from("  o                      Open selected location in Finder"),
        Line::from("  q / Esc / Ctrl+c       Quit"),
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
    use std::path::PathBuf;

    use super::{
        format_bytes, tree_columns, tree_index_at, tree_scroll_offset, tree_size_text,
        wrap_numbered_preview_line, DirEntryNode,
    };
    use crate::app::App;
    use crate::tree::TreeMode;
    use ratatui::layout::Rect;
    use tempfile::tempdir;

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

    #[test]
    fn tree_columns_show_name_size_and_date_when_wide_enough() {
        let entries = vec![sample_node(
            "note.txt",
            false,
            Some(1_024),
            Some("2026-03-20"),
        )];
        let columns = tree_columns(40, &entries);

        assert_eq!(columns.size_width, Some(4));
        assert_eq!(columns.date_width, Some(10));
        assert_eq!(columns.name_width, 22);
    }

    #[test]
    fn tree_columns_hide_date_before_size() {
        let entries = vec![sample_node(
            "note.txt",
            false,
            Some(1_024),
            Some("2026-03-20"),
        )];
        let columns = tree_columns(18, &entries);

        assert_eq!(columns.size_width, Some(4));
        assert_eq!(columns.date_width, None);
    }

    #[test]
    fn tree_columns_hide_all_metadata_when_too_narrow() {
        let entries = vec![sample_node(
            "note.txt",
            false,
            Some(1_024),
            Some("2026-03-20"),
        )];
        let columns = tree_columns(8, &entries);

        assert_eq!(columns.size_width, None);
        assert_eq!(columns.date_width, None);
        assert_eq!(columns.name_width, 8);
    }

    #[test]
    fn tree_scroll_offset_keeps_selected_row_visible() {
        assert_eq!(tree_scroll_offset(4, 0), 0);
        assert_eq!(tree_scroll_offset(4, 3), 0);
        assert_eq!(tree_scroll_offset(4, 4), 1);
    }

    #[test]
    fn tree_index_at_maps_click_to_visible_entry() {
        let tmp = tempdir().expect("tmpdir should exist");
        std::fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        std::fs::write(tmp.path().join("b.txt"), "b").expect("write should succeed");
        let app = App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let area = Rect::new(0, 0, 20, 4);

        assert_eq!(tree_index_at(area, &app, 1, 1), Some(0));
        assert_eq!(tree_index_at(area, &app, 1, 2), Some(1));
    }

    #[test]
    fn tree_index_at_ignores_border_and_blank_rows() {
        let tmp = tempdir().expect("tmpdir should exist");
        std::fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        let app = App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let area = Rect::new(0, 0, 20, 5);

        assert_eq!(tree_index_at(area, &app, 0, 0), None);
        assert_eq!(tree_index_at(area, &app, 1, 3), None);
        assert_eq!(tree_index_at(area, &app, 25, 1), None);
    }

    #[test]
    fn tree_index_at_uses_scrolled_selection_as_origin() {
        let tmp = tempdir().expect("tmpdir should exist");
        std::fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        std::fs::write(tmp.path().join("b.txt"), "b").expect("write should succeed");
        std::fs::write(tmp.path().join("c.txt"), "c").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.tree.select_index(2);
        let area = Rect::new(0, 0, 20, 4);

        assert_eq!(tree_index_at(area, &app, 1, 1), Some(1));
        assert_eq!(tree_index_at(area, &app, 1, 2), Some(2));
    }

    #[test]
    fn format_bytes_uses_compact_units() {
        assert_eq!(format_bytes(812), "812B");
        assert_eq!(format_bytes(2_048), "2.0K");
        assert_eq!(format_bytes(10 * 1024 * 1024), "10M");
    }

    #[test]
    fn tree_size_text_uses_dash_for_directories() {
        let dir = sample_node("src", true, None, Some("2026-03-20"));
        assert_eq!(tree_size_text(&dir), "-");
    }

    fn sample_node(
        name: &str,
        is_dir: bool,
        size_bytes: Option<u64>,
        modified_date: Option<&str>,
    ) -> DirEntryNode {
        DirEntryNode {
            path: PathBuf::from(name),
            name: name.to_string(),
            is_dir,
            is_symlink: false,
            size_bytes,
            modified_date: modified_date.map(str::to_string),
        }
    }
}
