//! Editor state: open file tabs, file tree, dirty tracking, and autosave.

mod tab;
mod tree;

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests create temporary files on disk for editor state verification"
)]
mod tests;

pub(crate) use tab::{EditorTab, char_to_byte_pos, detect_language_pub};
pub(crate) use tree::{FileEntry, FileTreeState, GitFileStatus};

use std::path::PathBuf;

const DEFAULT_AUTOSAVE_SECS: u64 = 30;

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

    pub(crate) fn open_file(&mut self, path: &std::path::Path) {
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
