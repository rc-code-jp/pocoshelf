use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Command;

pub fn map_event(key: KeyEvent) -> Option<Command> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) => Some(Command::Quit),
        (KeyCode::Esc, _) => Some(Command::Quit),
        (KeyCode::Char('k'), _) | (KeyCode::Up, _) => Some(Command::MoveUp),
        (KeyCode::Char('j'), _) | (KeyCode::Down, _) => Some(Command::MoveDown),
        (KeyCode::Char('l'), _) | (KeyCode::Right, _) | (KeyCode::Enter, _) => {
            Some(Command::ExpandOrOpen)
        }
        (KeyCode::Char('h'), _) | (KeyCode::Left, _) => Some(Command::Collapse),
        (KeyCode::Char('r'), _) => Some(Command::RefreshGit),
        (KeyCode::Char('p'), _) => Some(Command::TogglePreviewMode),
        (KeyCode::Char('v'), _) => Some(Command::OpenInVi),
        (KeyCode::Char('?'), _) | (KeyCode::F(1), _) => Some(Command::ToggleHelp),
        (KeyCode::Char('n'), _) => Some(Command::NextChange),
        (KeyCode::Char('N'), _) => Some(Command::PrevChange),
        (KeyCode::Char('c'), _) => Some(Command::CopyRelativePath),
        (KeyCode::Char('o'), _) => Some(Command::OpenInFinder),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) | (KeyCode::PageUp, _) => {
            Some(Command::PreviewUp)
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) | (KeyCode::PageDown, _) => {
            Some(Command::PreviewDown)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::map_event;
    use crate::app::Command;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn esc_maps_to_quit() {
        let event = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(map_event(event), Some(Command::Quit)));
    }

    #[test]
    fn c_maps_to_copy_relative_path() {
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(matches!(map_event(event), Some(Command::CopyRelativePath)));
    }

    #[test]
    fn p_maps_to_toggle_preview_mode() {
        let event = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE);
        assert!(matches!(map_event(event), Some(Command::TogglePreviewMode)));
    }

    #[test]
    fn v_maps_to_open_in_vi() {
        let event = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE);
        assert!(matches!(map_event(event), Some(Command::OpenInVi)));
    }
}
