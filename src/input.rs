use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Command;

pub fn map_event(key: KeyEvent) -> Option<Command> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) => Some(Command::Quit),
        (KeyCode::Char('k'), _) | (KeyCode::Up, _) => Some(Command::MoveUp),
        (KeyCode::Char('j'), _) | (KeyCode::Down, _) => Some(Command::MoveDown),
        (KeyCode::Char('l'), _) | (KeyCode::Right, _) | (KeyCode::Enter, _) => {
            Some(Command::ExpandOrOpen)
        }
        (KeyCode::Char('h'), _) | (KeyCode::Left, _) | (KeyCode::Esc, _) => Some(Command::Collapse),
        (KeyCode::Char('r'), _) => Some(Command::RefreshGit),
        (KeyCode::Char('v'), _) => Some(Command::TogglePreviewMode),
        (KeyCode::Char('?'), _) | (KeyCode::F(1), _) => Some(Command::ToggleHelp),
        (KeyCode::Char('n'), _) => Some(Command::NextChange),
        (KeyCode::Char('N'), _) => Some(Command::PrevChange),
        (KeyCode::Char('y'), _) => Some(Command::CopyRelativePath),
        (KeyCode::Char('o'), _) => Some(Command::OpenInEditor),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) | (KeyCode::PageUp, _) => {
            Some(Command::PreviewUp)
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) | (KeyCode::PageDown, _) => {
            Some(Command::PreviewDown)
        }
        _ => None,
    }
}
