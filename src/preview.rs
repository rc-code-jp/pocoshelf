use std::fs;
use std::path::Path;

pub const MAX_PREVIEW_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, Copy)]
pub enum PreviewKind {
    Text,
    Message,
}

#[derive(Debug, Clone)]
pub struct PreviewState {
    pub kind: PreviewKind,
    pub lines: Vec<String>,
    pub scroll: usize,
}

impl PreviewState {
    pub fn from_path(path: &Path, max_preview_bytes: u64) -> Self {
        if path.is_dir() {
            return Self::message("directory");
        }

        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(err) => return Self::message(format!("metadata error: {err}")),
        };

        if metadata.len() > max_preview_bytes {
            return Self::message(format!(
                "file too large: {} bytes (limit: {} bytes)",
                metadata.len(),
                max_preview_bytes
            ));
        }

        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(err) => return Self::message(format!("read error: {err}")),
        };

        if bytes.contains(&0) {
            return Self::message("binary file preview not supported");
        }

        let text = match std::str::from_utf8(&bytes) {
            Ok(t) => t,
            Err(_) => return Self::message("non-UTF-8 file preview not supported"),
        };

        let mut lines = text.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(String::new());
        }

        Self {
            kind: PreviewKind::Text,
            lines,
            scroll: 0,
        }
    }

    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            kind: PreviewKind::Message,
            lines: vec![msg.into()],
            scroll: 0,
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_add(amount);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{PreviewKind, PreviewState};

    #[test]
    fn preview_rejects_large_file() {
        let tmp = tempdir().expect("tmpdir should exist");
        let path = tmp.path().join("large.txt");
        fs::write(&path, "0123456789").expect("write should succeed");

        let preview = PreviewState::from_path(&path, 5);
        assert!(matches!(preview.kind, PreviewKind::Message));
    }

    #[test]
    fn preview_rejects_non_utf8() {
        let tmp = tempdir().expect("tmpdir should exist");
        let path = tmp.path().join("sjis.bin");
        fs::write(&path, vec![0x82, 0xa0]).expect("write should succeed");

        let preview = PreviewState::from_path(&path, 1024);
        assert!(matches!(preview.kind, PreviewKind::Message));
    }

    #[test]
    fn preview_accepts_utf8_text() {
        let tmp = tempdir().expect("tmpdir should exist");
        let path = tmp.path().join("ok.txt");
        fs::write(&path, "line1\nline2").expect("write should succeed");

        let preview = PreviewState::from_path(&path, 1024);
        assert!(matches!(preview.kind, PreviewKind::Text));
        assert_eq!(preview.lines.len(), 2);
    }
}
