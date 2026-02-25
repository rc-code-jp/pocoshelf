use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
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
    let mut lines = Vec::new();
    for visible in app.tree.visible_items() {
        let node = visible.node;
        let indent = "  ".repeat(visible.depth);
        let marker = if node.is_dir {
            if node.expanded {
                "v "
            } else {
                "> "
            }
        } else {
            "  "
        };

        let mut style = style_for_git(app.selected_git_state(&node.path, node.is_dir));
        if visible.is_selected {
            style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }

        lines.push(Line::from(Span::styled(
            format!("{indent}{marker}{}", node.name),
            style,
        )));
    }

    let tree = Paragraph::new(lines).block(Block::default().title("Tree").borders(Borders::ALL));
    frame.render_widget(tree, area);
}

fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    let start = app.preview.scroll.min(app.preview.lines.len().saturating_sub(1));
    let end = (start + height).min(app.preview.lines.len());

    let lines = app.preview.lines[start..end]
        .iter()
        .map(|line| Line::from(Span::raw(line.as_str())))
        .collect::<Vec<_>>();

    let preview = Paragraph::new(lines)
        .block(Block::default().title(app.preview_title()).borders(Borders::ALL));

    frame.render_widget(preview, area);
}

fn render_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let status = format!(
        "{} | selected: {} | keys: j/k/h/l arrows r y q",
        app.status_message,
        app.tree.selected_path().display()
    );

    let line = Line::from(Span::styled(status, Style::default().fg(Color::DarkGray)));
    frame.render_widget(Paragraph::new(line), area);
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
