//! File tree sidebar with expand/collapse and git status.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
