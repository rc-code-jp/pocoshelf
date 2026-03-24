use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, ContextMenu};
use crate::config::HelpLanguage;
use crate::git_status::GitState;
use crate::tree::DirEntryNode;

const TREE_COLUMN_GAP: usize = 2;
const TREE_DATE_WIDTH: usize = 10;
const TREE_MIN_NAME_WIDTH: usize = 12;
const CONTEXT_MENU_WIDTH: u16 = 24;
const CONTEXT_MENU_HEIGHT: u16 = 8; // 6 items + 2 border lines

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let outer = outer_layout(frame.area());

    render_tree(frame, app, outer[0]);
    render_status(frame, app, outer[1]);
    if app.help.visible {
        render_help(frame, app, frame.area());
    }
    if let Some(menu) = &app.context_menu {
        render_context_menu(frame, menu, frame.area());
    }
}

pub fn help_area(area: Rect) -> Rect {
    centered_rect(76, 80, area)
}

pub fn tree_area(area: Rect, _app: &App) -> Rect {
    outer_layout(area)[0]
}

pub fn tree_contains(area: Rect, app: &App, column: u16, row: u16) -> bool {
    tree_area(area, app).contains(ratatui::layout::Position { x: column, y: row })
}

pub fn help_contains(area: Rect, column: u16, row: u16) -> bool {
    help_area(area).contains(ratatui::layout::Position { x: column, y: row })
}

pub fn help_viewport_height(area: Rect) -> usize {
    help_area(area).height.saturating_sub(2) as usize
}

pub fn help_viewport_width(area: Rect) -> usize {
    help_area(area).width.saturating_sub(2) as usize
}

pub fn tree_max_scroll(entry_count: usize, viewport_height: usize) -> usize {
    if viewport_height == 0 {
        return 0;
    }

    entry_count.saturating_sub(viewport_height)
}

pub fn tree_scroll_offset(viewport_height: usize, scroll: usize, entry_count: usize) -> usize {
    if viewport_height == 0 {
        0
    } else {
        scroll.min(tree_max_scroll(entry_count, viewport_height))
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

    let scroll_offset =
        tree_scroll_offset(viewport_height, app.tree_scroll(), app.tree.entries.len());
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

fn render_tree(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let viewport_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let selected_index = app.tree.selected_index();
    let scroll_offset =
        tree_scroll_offset(viewport_height, app.tree_scroll(), app.tree.entries.len());

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

    let blank = " ".repeat(inner_width);
    while lines.len() < viewport_height {
        lines.push(Line::from(Span::raw(blank.clone())));
    }

    let tree = Paragraph::new(lines).block(
        Block::default()
            .title(app.tree_title())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(tree, area);
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
    let indent = "  ".repeat(node.depth);
    let marker = tree_marker(node);

    if node.is_dir {
        return format!("{indent}{marker} {}/", node.name);
    }

    if node.is_symlink {
        let target_text = std::fs::read_link(&node.path)
            .map(|t| format!(" -> {}", t.display()))
            .unwrap_or_default();
        return format!("{indent}{marker} {}{}", node.name, target_text);
    }

    format!("{indent}{marker} {}", node.name)
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

fn tree_marker(node: &DirEntryNode) -> &'static str {
    if node.is_dir {
        if node.is_expanded {
            "▼"
        } else {
            "▶"
        }
    } else {
        " "
    }
}

fn tree_meta_style(is_selected: bool) -> Style {
    let mut style = Style::default().fg(Color::DarkGray);
    if is_selected {
        style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
    }
    style
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
    let help_label = match app.help.language {
        HelpLanguage::En => "?: help",
        HelpLanguage::Ja => "?: ヘルプ",
    };
    let status_text = format!("{} | {}", app.status_message, help_label);
    let line = Line::from(Span::styled(
        status_text,
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(line), area);
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

fn help_section(title: &str) -> Vec<Line<'static>> {
    vec![Line::from(title.to_string())]
}

fn help_entry(keys: &str, description: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![Span::styled(
            format!("  {keys}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(format!("    {description}")),
    ]
}

fn help_blank() -> Vec<Line<'static>> {
    vec![Line::from("")]
}

fn help_content(language: HelpLanguage) -> (&'static str, Vec<Line<'static>>) {
    match language {
        HelpLanguage::En => {
            let mut lines = Vec::new();
            lines.extend(help_section("Navigation"));
            lines.extend(help_entry("j / k, Down / Up", "Move selection"));
            lines.extend(help_entry("h / Left", "Collapse dir or move to parent"));
            lines.extend(help_entry("l / Right / Enter", "Toggle selected directory"));
            lines.extend(help_entry("Left click", "Select files, toggle directories"));
            lines.extend(help_entry("Right click", "Open copy menu"));
            lines.extend(help_entry("Mouse wheel on tree", "Scroll tree by 3 lines"));
            lines.extend(help_blank());
            lines.extend(help_section("General"));
            lines.extend(help_entry("Tab", "Toggle tree mode (normal <-> changed)"));
            lines.extend(help_entry("r", "Refresh git status"));
            lines.extend(help_entry("c", "Copy @-relative path"));
            lines.extend(help_entry("v", "Open selected file in vi"));
            lines.extend(help_entry("o", "Open selected location in Finder"));
            lines.extend(help_entry("t", "Switch help language (English <-> 日本語)"));
            lines.extend(help_entry("q / Esc / Ctrl+c", "Quit"));
            lines.extend(help_entry("? / F1", "Toggle this help"));
            lines.extend(help_blank());
            lines.extend(help_section("Close help"));
            lines.extend(help_entry("h or ?", "Close this modal"));
            ("Help", lines)
        }
        HelpLanguage::Ja => {
            let mut lines = Vec::new();
            lines.extend(help_section("ナビゲーション"));
            lines.extend(help_entry("j / k, Down / Up", "選択を移動"));
            lines.extend(help_entry(
                "h / Left",
                "ディレクトリを閉じる、または親へ移動",
            ));
            lines.extend(help_entry("l / Right / Enter", "選択ディレクトリを開閉"));
            lines.extend(help_entry(
                "左クリック",
                "ファイルを選択し、ディレクトリなら開閉",
            ));
            lines.extend(help_entry("右クリック", "コピーメニューを表示"));
            lines.extend(help_entry("ツリー上のマウスホイール", "3行ずつスクロール"));
            lines.extend(help_blank());
            lines.extend(help_section("一般"));
            lines.extend(help_entry(
                "Tab",
                "ツリーモード切り替え (normal <-> changed)",
            ));
            lines.extend(help_entry("r", "Git ステータス更新"));
            lines.extend(help_entry("c", "@ 付き相対パスをコピー"));
            lines.extend(help_entry("v", "選択ファイルを vi で開く"));
            lines.extend(help_entry("o", "選択位置を Finder で開く"));
            lines.extend(help_entry("t", "ヘルプ言語を切り替え (English <-> 日本語)"));
            lines.extend(help_entry("q / Esc / Ctrl+c", "終了"));
            lines.extend(help_entry("? / F1", "このヘルプを表示"));
            lines.extend(help_blank());
            lines.extend(help_section("ヘルプを閉じる"));
            lines.extend(help_entry("h または ?", "このモーダルを閉じる"));
            ("ヘルプ", lines)
        }
    }
}

pub fn help_max_scroll(
    language: HelpLanguage,
    viewport_height: usize,
    inner_width: usize,
) -> usize {
    if viewport_height == 0 || inner_width == 0 {
        return 0;
    }

    let (_, lines) = help_content(language);
    let visual_line_count = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(inner_width))
        .sum::<usize>();

    visual_line_count.saturating_sub(viewport_height)
}

fn render_help(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let popup = help_area(area);
    let (title, help_lines) = help_content(app.help.language);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(help_lines)
            .block(block)
            .scroll((app.help.scroll as u16, 0))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn context_menu_rect(menu: &ContextMenu, area: Rect) -> Rect {
    let (click_x, click_y) = menu.position;
    let x = if click_x + CONTEXT_MENU_WIDTH <= area.width {
        click_x
    } else {
        click_x.saturating_sub(CONTEXT_MENU_WIDTH)
    };
    let y = if click_y + CONTEXT_MENU_HEIGHT <= area.height {
        click_y
    } else {
        click_y.saturating_sub(CONTEXT_MENU_HEIGHT)
    };
    Rect::new(x, y, CONTEXT_MENU_WIDTH, CONTEXT_MENU_HEIGHT)
}

pub fn context_menu_item_at(
    area: Rect,
    app: &App,
    column: u16,
    row: u16,
) -> Option<usize> {
    let menu = app.context_menu.as_ref()?;
    let rect = context_menu_rect(menu, area);
    let inner_x = column.checked_sub(rect.x + 1)?;
    let inner_y = row.checked_sub(rect.y + 1)?;
    let inner_width = rect.width.saturating_sub(2);
    if inner_width == 0 || inner_x >= inner_width {
        return None;
    }
    let index = usize::from(inner_y);
    if index < ContextMenu::ITEM_COUNT {
        Some(index)
    } else {
        None
    }
}

fn render_context_menu(frame: &mut Frame<'_>, menu: &ContextMenu, area: Rect) {
    let rect = context_menu_rect(menu, area);
    let inner_width = rect.width.saturating_sub(2) as usize;

    let labels = ["@ copy path", "cat command copy", "vi command copy", "open in vi", "open in Finder", "cancel"];
    let mut lines = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        let is_selected = i == menu.selected;
        let is_hovered = menu.hovered == Some(i) && !is_selected;
        let mut style = Style::default();
        if is_selected {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        } else if is_hovered {
            style = style.bg(Color::DarkGray);
        }
        let text = pad_to_width(&truncate_to_width(label, inner_width), inner_width);
        lines.push(Line::from(Span::styled(text, style)));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
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

    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use tempfile::tempdir;

    use super::{
        format_bytes, help_entry, help_max_scroll, tree_area, tree_columns, tree_contains,
        tree_index_at, tree_max_scroll, tree_name_text, tree_scroll_offset, tree_size_text,
        DirEntryNode,
    };
    use crate::app::App;
    use crate::config::HelpLanguage;
    use crate::tree::TreeMode;
    use ratatui::layout::Rect;

    #[test]
    fn help_entry_uses_two_lines() {
        let lines = help_entry("j / k", "Move selection");

        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            Line::from(vec![Span::styled(
                "  j / k",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )])
        );
        assert_eq!(lines[1], Line::from("    Move selection"));
    }

    #[test]
    fn help_max_scroll_grows_when_viewport_is_short() {
        let max_scroll = help_max_scroll(HelpLanguage::En, 4, 24);
        assert!(max_scroll > 0);
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
    fn tree_max_scroll_stops_at_last_full_page() {
        assert_eq!(tree_max_scroll(2, 4), 0);
        assert_eq!(tree_max_scroll(4, 4), 0);
        assert_eq!(tree_max_scroll(6, 4), 2);
    }

    #[test]
    fn tree_scroll_offset_clamps_to_max_scroll() {
        assert_eq!(tree_scroll_offset(4, 0, 6), 0);
        assert_eq!(tree_scroll_offset(4, 1, 6), 1);
        assert_eq!(tree_scroll_offset(4, 5, 6), 2);
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
    fn tree_index_at_uses_scrolled_view_as_origin() {
        let tmp = tempdir().expect("tmpdir should exist");
        std::fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        std::fs::write(tmp.path().join("b.txt"), "b").expect("write should succeed");
        std::fs::write(tmp.path().join("c.txt"), "c").expect("write should succeed");
        let mut app =
            App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        app.set_tree_viewport_size(2);
        app.handle_mouse_wheel(Rect::new(0, 0, 20, 4), 1, 1, false);
        let area = Rect::new(0, 0, 20, 4);

        assert_eq!(tree_index_at(area, &app, 1, 1), Some(1));
        assert_eq!(tree_index_at(area, &app, 1, 2), Some(2));
    }

    #[test]
    fn tree_contains_accepts_border_and_empty_space() {
        let tmp = tempdir().expect("tmpdir should exist");
        std::fs::write(tmp.path().join("a.txt"), "a").expect("write should succeed");
        let app = App::new(tmp.path().to_path_buf(), TreeMode::Normal).expect("app should build");
        let area = Rect::new(0, 0, 20, 10);
        let tree = tree_area(area, &app);

        assert!(tree_contains(area, &app, tree.x, tree.y));
        assert!(tree_contains(area, &app, tree.x + 10, tree.y));
        assert!(tree_contains(area, &app, tree.x + 1, tree.bottom() - 1));
        assert!(!tree_contains(area, &app, 25, 1));
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

    #[test]
    fn tree_name_text_shows_indent_and_marker_for_directory() {
        let mut dir = sample_node("src", true, None, Some("2026-03-20"));
        dir.depth = 2;
        dir.is_expanded = true;

        assert_eq!(tree_name_text(&dir), "    ▼ src/");
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
            exists_on_disk: true,
            size_bytes,
            modified_date: modified_date.map(str::to_string),
            depth: 0,
            is_expanded: false,
        }
    }
}
