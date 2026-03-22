//! Editor state: open file tabs, file tree, dirty tracking, and autosave.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const DEFAULT_AUTOSAVE_SECS: u64 = 30;
const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

/// Git working-tree status for a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitFileStatus {
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
}

impl GitFileStatus {
    pub(crate) fn badge(&self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Untracked => "?",
            Self::Deleted => "D",
            Self::Renamed => "R",
        }
    }
}

/// A single entry in the file tree sidebar.
#[derive(Debug, Clone)]
pub(crate) struct FileEntry {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) is_dir: bool,
    pub(crate) depth: usize,
    pub(crate) git_status: Option<GitFileStatus>,
}

/// Navigable directory tree with expand/collapse and git status.
#[derive(Debug, Clone)]
pub(crate) struct FileTreeState {
    pub(crate) root: PathBuf,
    pub(crate) entries: Vec<FileEntry>,
    pub(crate) selected: usize,
    pub(crate) expanded: HashSet<PathBuf>,
    pub(crate) scroll_offset: usize,
}

impl FileTreeState {
    pub(crate) fn new(root: PathBuf) -> Self {
        let mut state = Self {
            root: root.clone(),
            entries: Vec::new(),
            selected: 0,
            expanded: HashSet::new(),
            scroll_offset: 0,
        };
        state.expanded.insert(root);
        state.refresh();
        state
    }

    pub(crate) fn refresh(&mut self) {
        let git_statuses = load_git_status(&self.root);
        self.entries.clear();
        build_tree(
            &self.root,
            0,
            &self.expanded,
            &git_statuses,
            &mut self.entries,
        );
        if !self.entries.is_empty() && self.selected >= self.entries.len() {
            self.selected = self.entries.len() - 1;
        }
    }

    pub(crate) fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.ensure_visible();
    }

    pub(crate) fn select_down(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1).min(self.entries.len() - 1);
        }
        self.ensure_visible();
    }

    pub(crate) fn toggle_expand(&mut self) {
        if let Some(entry) = self.entries.get(self.selected)
            && entry.is_dir
        {
            let path = entry.path.clone();
            if self.expanded.contains(&path) {
                self.expanded.remove(&path);
            } else {
                self.expanded.insert(path);
            }
            self.refresh();
        }
    }

    pub(crate) fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }

    fn ensure_visible(&mut self) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
    }

    /// Adjust scroll so that `selected` stays within the visible window.
    pub(crate) fn adjust_scroll(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        if self.selected >= self.scroll_offset + visible_height {
            self.scroll_offset = self.selected - visible_height + 1;
        }
    }
}

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
    /// Open a file from disk and create a tab.
    /// Returns `None` if the file is too large, binary, or unreadable.
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

    /// Write content back to disk.
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

    /// Ensure cursor row is visible given the viewport height.
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

/// Full editor state: tabs, file tree, clipboard, and modal inputs.
#[derive(Debug)]
pub(crate) struct EditorState {
    pub(crate) tabs: Vec<EditorTab>,
    pub(crate) active_tab: usize,
    pub(crate) tree: FileTreeState,
    pub(crate) tree_visible: bool,
    pub(crate) tree_focused: bool,
    pub(crate) autosave_secs: u64,
    pub(crate) clipboard: Vec<String>,
    pub(crate) confirm_delete: Option<PathBuf>,
    pub(crate) rename_input: Option<String>,
    pub(crate) new_file_input: Option<String>,
}

impl EditorState {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
            tree: FileTreeState::new(root),
            tree_visible: true,
            tree_focused: true,
            autosave_secs: DEFAULT_AUTOSAVE_SECS,
            clipboard: Vec::new(),
            confirm_delete: None,
            rename_input: None,
            new_file_input: None,
        }
    }

    pub(crate) fn open_file(&mut self, path: &Path) {
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = idx;
            self.tree_focused = false;
            return;
        }
        if let Some(tab) = EditorTab::from_path(path) {
            self.tabs.push(tab);
            self.active_tab = self.tabs.len() - 1;
            self.tree_focused = false;
        }
    }

    pub(crate) fn close_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() && !self.tabs.is_empty() {
                self.active_tab = self.tabs.len() - 1;
            }
            if self.tabs.is_empty() {
                self.tree_focused = true;
            }
        }
    }

    pub(crate) fn active_tab(&self) -> Option<&EditorTab> {
        self.tabs.get(self.active_tab)
    }

    pub(crate) fn active_tab_mut(&mut self) -> Option<&mut EditorTab> {
        self.tabs.get_mut(self.active_tab)
    }

    pub(crate) fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    pub(crate) fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
        }
    }

    /// Check and perform autosave for dirty tabs past their interval.
    pub(crate) fn autosave_tick(&mut self) {
        let interval = std::time::Duration::from_secs(self.autosave_secs);
        for tab in &mut self.tabs {
            if tab.dirty {
                let should_save = tab
                    .last_saved_at
                    .map(|saved_at| saved_at.elapsed() >= interval)
                    .unwrap_or(true);
                if should_save {
                    let _ = tab.save();
                }
            }
        }
    }

    /// Whether any modal input (rename, new file, delete confirm) is active.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "used in tests; keyboard handler checks fields directly"
        )
    )]
    pub(crate) fn has_modal_input(&self) -> bool {
        self.rename_input.is_some()
            || self.new_file_input.is_some()
            || self.confirm_delete.is_some()
    }
}

impl Default for EditorState {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new(cwd)
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

fn char_to_byte_pos(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(byte, _)| byte)
        .unwrap_or(s.len())
}

fn build_tree(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    git_statuses: &HashMap<PathBuf, GitFileStatus>,
    entries: &mut Vec<FileEntry>,
) {
    let mut children: Vec<_> = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy();
                !name_str.starts_with('.')
                    && name_str != "target"
                    && name_str != "node_modules"
                    && name_str != "__pycache__"
            })
            .collect(),
        Err(_) => return,
    };

    children.sort_by(|a, b| {
        let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    for child in children {
        let path = child.path();
        let is_dir = child.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let name = child.file_name().to_string_lossy().to_string();
        let git_status = git_statuses.get(&path).cloned();

        entries.push(FileEntry {
            path: path.clone(),
            name,
            is_dir,
            depth,
            git_status,
        });

        if is_dir && expanded.contains(&path) {
            build_tree(&path, depth + 1, expanded, git_statuses, entries);
        }
    }
}

fn load_git_status(root: &Path) -> HashMap<PathBuf, GitFileStatus> {
    let mut statuses = HashMap::new();

    let output = match std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
    {
        Ok(o) => o,
        Err(_) => return statuses,
    };

    if !output.status.success() {
        return statuses;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        // WHY: porcelain format is "XY filename" where XY is 2 status chars + space.
        // Lines shorter than 4 chars are malformed.
        let Some(status_part) = line.get(..2) else {
            continue;
        };
        let Some(file_part) = line.get(3..) else {
            continue;
        };
        let file_path_str = file_part.trim();

        // WHY: renames show as "old -> new"; we want the new name.
        let file_path_str = file_path_str.split(" -> ").last().unwrap_or(file_path_str);

        let status = match status_part {
            "M " | " M" | "MM" => GitFileStatus::Modified,
            "A " | "AM" => GitFileStatus::Added,
            "??" => GitFileStatus::Untracked,
            "D " | " D" => GitFileStatus::Deleted,
            s if s.starts_with('R') => GitFileStatus::Renamed,
            _ => continue,
        };

        let full_path = root.join(file_path_str);
        statuses.insert(full_path, status);
    }

    statuses
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests create temporary files on disk for editor state verification"
)]
mod tests {
    use super::*;

    #[test]
    fn detect_language_rust() {
        assert_eq!(detect_language(Path::new("main.rs")), "rust");
    }

    #[test]
    fn detect_language_python() {
        assert_eq!(detect_language(Path::new("script.py")), "python");
    }

    #[test]
    fn detect_language_typescript() {
        assert_eq!(detect_language(Path::new("app.tsx")), "typescript");
    }

    #[test]
    fn detect_language_no_extension() {
        assert_eq!(detect_language(Path::new("Makefile")), "plain text");
    }

    #[test]
    fn char_to_byte_pos_ascii() {
        assert_eq!(char_to_byte_pos("hello", 0), 0);
        assert_eq!(char_to_byte_pos("hello", 3), 3);
        assert_eq!(char_to_byte_pos("hello", 5), 5);
    }

    #[test]
    fn char_to_byte_pos_multibyte() {
        let s = "h\u{00e9}llo";
        assert_eq!(char_to_byte_pos(s, 0), 0);
        assert_eq!(char_to_byte_pos(s, 1), 1);
        assert_eq!(char_to_byte_pos(s, 2), 3);
    }

    #[test]
    fn char_to_byte_pos_past_end() {
        assert_eq!(char_to_byte_pos("ab", 5), 2);
    }

    #[test]
    fn editor_tab_insert_char() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: vec!["hello".to_string()],
            cursor_row: 0,
            cursor_col: 5,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };
        tab.insert_char('!');
        assert_eq!(tab.content.first().map(String::as_str), Some("hello!"));
        assert_eq!(tab.cursor_col, 6);
        assert!(tab.dirty);
    }

    #[test]
    fn editor_tab_insert_newline() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: vec!["hello world".to_string()],
            cursor_row: 0,
            cursor_col: 5,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };
        tab.insert_newline();
        assert_eq!(tab.content.len(), 2);
        assert_eq!(tab.content.first().map(String::as_str), Some("hello"));
        assert_eq!(tab.content.get(1).map(String::as_str), Some(" world"));
        assert_eq!(tab.cursor_row, 1);
        assert_eq!(tab.cursor_col, 0);
    }

    #[test]
    fn editor_tab_backspace_middle() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: vec!["hello".to_string()],
            cursor_row: 0,
            cursor_col: 3,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };
        tab.backspace();
        assert_eq!(tab.content.first().map(String::as_str), Some("helo"));
        assert_eq!(tab.cursor_col, 2);
    }

    #[test]
    fn editor_tab_backspace_joins_lines() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: vec!["hello".to_string(), "world".to_string()],
            cursor_row: 1,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };
        tab.backspace();
        assert_eq!(tab.content.len(), 1);
        assert_eq!(tab.content.first().map(String::as_str), Some("helloworld"));
        assert_eq!(tab.cursor_row, 0);
        assert_eq!(tab.cursor_col, 5);
    }

    #[test]
    fn editor_tab_delete_char() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: vec!["hello".to_string()],
            cursor_row: 0,
            cursor_col: 2,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };
        tab.delete_char();
        assert_eq!(tab.content.first().map(String::as_str), Some("helo"));
    }

    #[test]
    fn editor_tab_delete_line() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: vec!["line1".to_string(), "line2".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };
        let cut = tab.delete_line();
        assert_eq!(cut, vec!["line1"]);
        assert_eq!(tab.content.len(), 1);
        assert_eq!(tab.content.first().map(String::as_str), Some("line2"));
    }

    #[test]
    fn editor_tab_cursor_movement() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: vec!["abc".to_string(), "de".to_string(), "fghij".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };

        tab.cursor_down();
        assert_eq!(tab.cursor_row, 1);

        tab.cursor_end();
        assert_eq!(tab.cursor_col, 2);

        tab.cursor_down();
        assert_eq!(tab.cursor_row, 2);
        assert_eq!(tab.cursor_col, 2);

        tab.cursor_home();
        assert_eq!(tab.cursor_col, 0);

        tab.cursor_up();
        assert_eq!(tab.cursor_row, 1);
    }

    #[test]
    fn editor_tab_page_movement() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: (0..50).map(|i| format!("line {i}")).collect(),
            cursor_row: 25,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };

        tab.page_up(10);
        assert_eq!(tab.cursor_row, 15);

        tab.page_down(20);
        assert_eq!(tab.cursor_row, 35);
    }

    #[test]
    fn editor_tab_ensure_cursor_visible() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: (0..50).map(|i| format!("line {i}")).collect(),
            cursor_row: 30,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };

        tab.ensure_cursor_visible(20);
        assert_eq!(tab.scroll_row, 11);
    }

    #[test]
    fn editor_state_open_file_deduplicates() {
        let dir = std::env::temp_dir().join("editor_test_dedup");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("test.txt");
        let _ = std::fs::write(&file_path, "content");

        let mut state = EditorState::new(dir.clone());
        state.open_file(&file_path);
        state.open_file(&file_path);
        assert_eq!(state.tabs.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editor_state_close_tab() {
        let dir = std::env::temp_dir().join("editor_test_close");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        let _ = std::fs::write(&f1, "a");
        let _ = std::fs::write(&f2, "b");

        let mut state = EditorState::new(dir.clone());
        state.open_file(&f1);
        state.open_file(&f2);
        assert_eq!(state.tabs.len(), 2);

        state.close_tab(0);
        assert_eq!(state.tabs.len(), 1);
        assert_eq!(state.active_tab, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editor_state_tab_cycling() {
        let mut state = EditorState {
            tabs: vec![],
            active_tab: 0,
            tree: FileTreeState {
                root: PathBuf::from("."),
                entries: Vec::new(),
                selected: 0,
                expanded: HashSet::new(),
                scroll_offset: 0,
            },
            tree_visible: true,
            tree_focused: true,
            autosave_secs: 30,
            clipboard: Vec::new(),
            confirm_delete: None,
            rename_input: None,
            new_file_input: None,
        };

        // Build 3 fake tabs
        for name in ["a", "b", "c"] {
            state.tabs.push(EditorTab {
                path: PathBuf::from(name),
                content: vec![String::new()],
                cursor_row: 0,
                cursor_col: 0,
                scroll_row: 0,
                dirty: false,
                language: "plain text".to_string(),
                last_saved_at: None,
            });
        }
        state.active_tab = 0;

        state.next_tab();
        assert_eq!(state.active_tab, 1);
        state.next_tab();
        assert_eq!(state.active_tab, 2);
        state.next_tab();
        assert_eq!(state.active_tab, 0);

        state.prev_tab();
        assert_eq!(state.active_tab, 2);
    }

    #[test]
    fn git_file_status_badges() {
        assert_eq!(GitFileStatus::Modified.badge(), "M");
        assert_eq!(GitFileStatus::Added.badge(), "A");
        assert_eq!(GitFileStatus::Untracked.badge(), "?");
        assert_eq!(GitFileStatus::Deleted.badge(), "D");
        assert_eq!(GitFileStatus::Renamed.badge(), "R");
    }

    #[test]
    fn file_tree_state_select_up_saturates() {
        let mut tree = FileTreeState {
            root: PathBuf::from("."),
            entries: vec![FileEntry {
                path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                is_dir: false,
                depth: 0,
                git_status: None,
            }],
            selected: 0,
            expanded: HashSet::new(),
            scroll_offset: 0,
        };
        tree.select_up();
        assert_eq!(tree.selected, 0);
    }

    #[test]
    fn file_tree_state_select_down_clamps() {
        let mut tree = FileTreeState {
            root: PathBuf::from("."),
            entries: vec![
                FileEntry {
                    path: PathBuf::from("a.txt"),
                    name: "a.txt".to_string(),
                    is_dir: false,
                    depth: 0,
                    git_status: None,
                },
                FileEntry {
                    path: PathBuf::from("b.txt"),
                    name: "b.txt".to_string(),
                    is_dir: false,
                    depth: 0,
                    git_status: None,
                },
            ],
            selected: 1,
            expanded: HashSet::new(),
            scroll_offset: 0,
        };
        tree.select_down();
        assert_eq!(tree.selected, 1);
    }

    #[test]
    fn editor_tab_copy_line() {
        let tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: vec!["hello".to_string(), "world".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };
        let copied = tab.copy_line();
        assert_eq!(copied, vec!["hello"]);
    }

    #[test]
    fn editor_tab_paste_lines() {
        let mut tab = EditorTab {
            path: PathBuf::from("test.txt"),
            content: vec!["line1".to_string(), "line2".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            language: "plain text".to_string(),
            last_saved_at: None,
        };
        tab.paste_lines(&["pasted".to_string()]);
        assert_eq!(tab.content.len(), 3);
        assert_eq!(tab.content.get(1).map(String::as_str), Some("pasted"));
        assert!(tab.dirty);
    }

    #[test]
    fn editor_state_has_modal_input_false_by_default() {
        let state = EditorState::new(PathBuf::from("."));
        assert!(!state.has_modal_input());
    }
}
