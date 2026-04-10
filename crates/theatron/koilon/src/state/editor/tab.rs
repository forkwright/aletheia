//! Single editor tab with file content, cursor, and editing operations.

use std::path::{Path, PathBuf};

const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

/// A single open file tab with editing state.
#[derive(Debug, Clone)]
pub(crate) struct EditorTab {
    pub(crate) path: PathBuf,
    pub(crate) content: Vec<String>,
    pub(crate) cursor_row: usize,
    pub(crate) cursor_col: usize,
    pub(crate) scroll_row: usize,
    pub(crate) dirty: bool,
    pub(crate) language: String,
    pub(crate) last_saved_at: Option<std::time::Instant>,
}

impl EditorTab {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        if let Ok(metadata) = std::fs::metadata(path)
            && metadata.len() > MAX_FILE_SIZE_BYTES
        {
            return None;
        }

        let content_str = std::fs::read_to_string(path).ok()?;
        let content: Vec<String> = content_str.lines().map(String::from).collect();
        let content = if content.is_empty() {
            vec![String::new()]
        } else {
            content
        };

        let language = detect_language(path);

        Some(Self {
            path: path.to_path_buf(),
            content,
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            language,
            last_saved_at: Some(std::time::Instant::now()),
        })
    }

    pub(crate) fn file_name(&self) -> &str {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("untitled")
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "editor save is a user-initiated sync write to the local filesystem"
    )]
    pub(crate) fn save(&mut self) -> Result<(), String> {
        let mut content = self.content.join("\n");
        if !content.ends_with('\n') {
            content.push('\n');
        }
        std::fs::write(&self.path, &content).map_err(|e| format!("Save failed: {e}"))?;
        // WHY: restrict saved files to owner-only (0600) — may contain sensitive content
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("Failed to set permissions: {e}"))?;
        }
        self.dirty = false;
        self.last_saved_at = Some(std::time::Instant::now());
        Ok(())
    }

    pub(crate) fn insert_char(&mut self, c: char) {
        if let Some(line) = self.content.get_mut(self.cursor_row) {
            let byte_pos = char_to_byte_pos(line, self.cursor_col);
            line.insert(byte_pos, c);
            self.cursor_col += 1;
            self.dirty = true;
        }
    }

    pub(crate) fn insert_newline(&mut self) {
        if let Some(line) = self.content.get(self.cursor_row).cloned() {
            let byte_pos = char_to_byte_pos(&line, self.cursor_col);
            let remainder = line.get(byte_pos..).unwrap_or("").to_string();
            if let Some(current) = self.content.get_mut(self.cursor_row) {
                current.truncate(byte_pos);
            }
            self.cursor_row += 1;
            self.content.insert(self.cursor_row, remainder);
            self.cursor_col = 0;
            self.dirty = true;
        }
    }

    pub(crate) fn backspace(&mut self) {
        if self.cursor_col > 0 {
            if let Some(line) = self.content.get_mut(self.cursor_row) {
                self.cursor_col -= 1;
                let byte_pos = char_to_byte_pos(line, self.cursor_col);
                let next_byte = char_to_byte_pos(line, self.cursor_col + 1);
                line.drain(byte_pos..next_byte);
                self.dirty = true;
            }
        } else if self.cursor_row > 0 {
            let current_line = self.content.remove(self.cursor_row);
            self.cursor_row -= 1;
            if let Some(prev) = self.content.get_mut(self.cursor_row) {
                self.cursor_col = prev.chars().count();
                prev.push_str(&current_line);
            }
            self.dirty = true;
        }
    }

    pub(crate) fn delete_char(&mut self) {
        let char_count = self
            .content
            .get(self.cursor_row)
            .map(|l| l.chars().count())
            .unwrap_or(0);

        if self.cursor_col < char_count {
            if let Some(line) = self.content.get_mut(self.cursor_row) {
                let byte_pos = char_to_byte_pos(line, self.cursor_col);
                let next_byte = char_to_byte_pos(line, self.cursor_col + 1);
                line.drain(byte_pos..next_byte);
                self.dirty = true;
            }
        } else if self.cursor_row + 1 < self.content.len() {
            let next_line = self.content.remove(self.cursor_row + 1);
            if let Some(current) = self.content.get_mut(self.cursor_row) {
                current.push_str(&next_line);
            }
            self.dirty = true;
        }
    }

    pub(crate) fn delete_line(&mut self) -> Vec<String> {
        let cut = self
            .content
            .get(self.cursor_row)
            .cloned()
            .into_iter()
            .collect();

        if self.content.len() > 1 {
            self.content.remove(self.cursor_row);
            if self.cursor_row >= self.content.len() {
                self.cursor_row = self.content.len() - 1;
            }
            self.clamp_cursor_col();
            self.dirty = true;
        } else if let Some(line) = self.content.get_mut(0) {
            line.clear();
            self.cursor_col = 0;
            self.dirty = true;
        }

        cut
    }

    pub(crate) fn copy_line(&self) -> Vec<String> {
        self.content
            .get(self.cursor_row)
            .cloned()
            .into_iter()
            .collect()
    }

    pub(crate) fn paste_lines(&mut self, lines: &[String]) {
        for (i, line) in lines.iter().enumerate() {
            let insert_row = self.cursor_row + 1 + i;
            if insert_row <= self.content.len() {
                self.content.insert(insert_row, line.clone());
            }
        }
        if !lines.is_empty() {
            self.cursor_row += 1;
            self.cursor_col = 0;
            self.dirty = true;
        }
    }

    pub(crate) fn cursor_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_cursor_col();
        }
    }

    pub(crate) fn cursor_down(&mut self) {
        if self.cursor_row + 1 < self.content.len() {
            self.cursor_row += 1;
            self.clamp_cursor_col();
        }
    }

    pub(crate) fn cursor_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.current_line_char_count();
        }
    }

    pub(crate) fn cursor_right(&mut self) {
        let line_len = self.current_line_char_count();
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.content.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    pub(crate) fn cursor_home(&mut self) {
        self.cursor_col = 0;
    }

    pub(crate) fn cursor_end(&mut self) {
        self.cursor_col = self.current_line_char_count();
    }

    pub(crate) fn page_up(&mut self, page_size: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(page_size);
        self.clamp_cursor_col();
    }

    pub(crate) fn page_down(&mut self, page_size: usize) {
        self.cursor_row = (self.cursor_row + page_size).min(self.content.len().saturating_sub(1));
        self.clamp_cursor_col();
    }

    fn current_line_char_count(&self) -> usize {
        self.content
            .get(self.cursor_row)
            .map(|l| l.chars().count())
            .unwrap_or(0)
    }

    fn clamp_cursor_col(&mut self) {
        let max = self.current_line_char_count();
        if self.cursor_col > max {
            self.cursor_col = max;
        }
    }

    pub(crate) fn ensure_cursor_visible(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.cursor_row < self.scroll_row {
            self.scroll_row = self.cursor_row;
        }
        if self.cursor_row >= self.scroll_row + viewport_height {
            self.scroll_row = self.cursor_row - viewport_height + 1;
        }
    }
}

fn detect_language(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| match ext {
            "rs" => "rust",
            "py" => "python",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "toml" => "toml",
            "yaml" | "yml" => "yaml",
            "md" | "markdown" => "markdown",
            "json" => "json",
            "sh" | "bash" => "bash",
            "css" => "css",
            "html" | "htm" => "html",
            "sql" => "sql",
            "xml" => "xml",
            "go" => "go",
            "c" | "h" => "c",
            "cpp" | "cc" | "cxx" | "hpp" => "cpp",
            other => other,
        })
        .unwrap_or("plain text")
        .to_string()
}

pub(crate) fn detect_language_pub(path: &Path) -> String {
    detect_language(path)
}

pub(crate) fn char_to_byte_pos(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(byte, _)| byte)
        .unwrap_or(s.len())
}
