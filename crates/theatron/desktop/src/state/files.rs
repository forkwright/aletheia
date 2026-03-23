//! Reactive state for the workspace file tree explorer.

use std::collections::HashMap;

/// Git status of a workspace file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
    Clean,
}

impl GitStatus {
    /// Severity rank for propagation — higher means more severe.
    fn severity(self) -> u8 {
        match self {
            Self::Clean => 0,
            Self::Untracked => 1,
            Self::Added => 2,
            Self::Modified => 3,
            Self::Deleted => 4,
        }
    }

    /// Return the more severe of two statuses.
    pub(crate) fn merge(self, other: Self) -> Self {
        if other.severity() > self.severity() {
            other
        } else {
            self
        }
    }
}

/// Type of node in the file tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeType {
    File,
    Directory,
}

/// A single node in the file tree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FileNode {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) node_type: NodeType,
    pub(crate) size: u64,
    pub(crate) children: Vec<FileNode>,
    pub(crate) children_loaded: bool,
}

impl FileNode {
    pub(crate) fn new_file(path: String, name: String, size: u64) -> Self {
        Self {
            path,
            name,
            node_type: NodeType::File,
            size,
            children: Vec::new(),
            children_loaded: true,
        }
    }

    pub(crate) fn new_directory(path: String, name: String) -> Self {
        Self {
            path,
            name,
            node_type: NodeType::Directory,
            size: 0,
            children: Vec::new(),
            children_loaded: false,
        }
    }

    pub(crate) fn is_dir(&self) -> bool {
        self.node_type == NodeType::Directory
    }
}

/// Map from file path to git status.
pub(crate) type GitStatusMap = HashMap<String, GitStatus>;

/// Expanded state for directory paths.
pub(crate) type ExpandedSet = HashMap<String, bool>;

/// Propagate the most severe child git status to a directory node.
pub(crate) fn propagated_status(node: &FileNode, status_map: &GitStatusMap) -> GitStatus {
    if node.node_type == NodeType::File {
        return status_map
            .get(&node.path)
            .copied()
            .unwrap_or(GitStatus::Clean);
    }

    let mut worst = GitStatus::Clean;
    for child in &node.children {
        let child_status = propagated_status(child, status_map);
        worst = worst.merge(child_status);
    }

    // WHY: also check for direct entries in the status map that are under
    // this directory path but not yet loaded as children.
    let dir_prefix = if node.path.ends_with('/') {
        node.path.clone()
    } else {
        format!("{}/", node.path)
    };
    for (path, &status) in status_map {
        if path.starts_with(&dir_prefix) {
            worst = worst.merge(status);
        }
    }

    worst
}

/// Parse a git status character from the API response.
pub(crate) fn parse_git_status(code: &str) -> GitStatus {
    match code {
        "M" | "modified" => GitStatus::Modified,
        "A" | "added" => GitStatus::Added,
        "D" | "deleted" => GitStatus::Deleted,
        "?" | "untracked" => GitStatus::Untracked,
        _ => GitStatus::Clean,
    }
}

/// Detect whether file content is binary by checking for null bytes in the
/// first 8KB.
pub(crate) fn is_binary_content(bytes: &[u8]) -> bool {
    let check_len = bytes.len().min(8192);
    bytes
        .get(..check_len)
        .is_some_and(|slice| slice.contains(&0))
}

/// Map a file extension to a syntect language token.
pub(crate) fn extension_to_language(path: &str) -> &str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => "rust",
        "py" => "python",
        "js" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "jsx" => "jsx",
        "rb" => "ruby",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "cs" => "cs",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "sh" | "bash" => "bash",
        "fish" => "fish",
        "zsh" => "zsh",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "xml" => "xml",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "sql" => "sql",
        "md" | "markdown" => "markdown",
        "dockerfile" | "Dockerfile" => "dockerfile",
        "lua" => "lua",
        "r" | "R" => "r",
        "zig" => "zig",
        _ => ext,
    }
}

/// Derive a unicode icon for a file based on extension.
pub(crate) fn file_icon(path: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "\u{1F4C1}"; // folder
    }
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => "\u{1F980}",                                                    // crab
        "py" => "\u{1F40D}",                                                    // snake
        "js" | "ts" | "jsx" | "tsx" => "\u{1F4DC}",                             // scroll
        "toml" | "yaml" | "yml" | "json" | "xml" | "ini" => "\u{2699}\u{FE0F}", // gear
        "md" | "txt" | "rst" => "\u{1F4DD}",                                    // memo
        "sh" | "bash" | "fish" | "zsh" => "\u{1F4BB}",                          // laptop
        "lock" => "\u{1F512}",                                                  // lock
        "css" | "scss" => "\u{1F3A8}",                                          // palette
        "html" | "htm" => "\u{1F310}",                                          // globe
        _ => "\u{1F4C4}",                                                       // page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_status_merge_picks_higher_severity() {
        assert_eq!(
            GitStatus::Clean.merge(GitStatus::Modified),
            GitStatus::Modified
        );
        assert_eq!(
            GitStatus::Modified.merge(GitStatus::Clean),
            GitStatus::Modified
        );
        assert_eq!(
            GitStatus::Added.merge(GitStatus::Deleted),
            GitStatus::Deleted
        );
        assert_eq!(
            GitStatus::Untracked.merge(GitStatus::Untracked),
            GitStatus::Untracked
        );
    }

    #[test]
    fn propagated_status_file_returns_own_status() {
        let node = FileNode::new_file("src/main.rs".into(), "main.rs".into(), 100);
        let mut map = GitStatusMap::new();
        map.insert("src/main.rs".into(), GitStatus::Modified);
        assert_eq!(propagated_status(&node, &map), GitStatus::Modified);
    }

    #[test]
    fn propagated_status_directory_aggregates_children() {
        let child_a = FileNode::new_file("src/a.rs".into(), "a.rs".into(), 50);
        let child_b = FileNode::new_file("src/b.rs".into(), "b.rs".into(), 50);
        let mut dir = FileNode::new_directory("src".into(), "src".into());
        dir.children = vec![child_a, child_b];
        dir.children_loaded = true;

        let mut map = GitStatusMap::new();
        map.insert("src/a.rs".into(), GitStatus::Added);
        map.insert("src/b.rs".into(), GitStatus::Deleted);

        assert_eq!(propagated_status(&dir, &map), GitStatus::Deleted);
    }

    #[test]
    fn propagated_status_clean_when_no_entries() {
        let dir = FileNode::new_directory("lib".into(), "lib".into());
        let map = GitStatusMap::new();
        assert_eq!(propagated_status(&dir, &map), GitStatus::Clean);
    }

    #[test]
    fn parse_git_status_codes() {
        assert_eq!(parse_git_status("M"), GitStatus::Modified);
        assert_eq!(parse_git_status("A"), GitStatus::Added);
        assert_eq!(parse_git_status("D"), GitStatus::Deleted);
        assert_eq!(parse_git_status("?"), GitStatus::Untracked);
        assert_eq!(parse_git_status("X"), GitStatus::Clean);
        assert_eq!(parse_git_status("modified"), GitStatus::Modified);
    }

    #[test]
    fn binary_detection_finds_null_bytes() {
        assert!(is_binary_content(&[0x89, 0x50, 0x4E, 0x47, 0x00]));
        assert!(!is_binary_content(b"hello world"));
        assert!(!is_binary_content(&[]));
    }

    #[test]
    fn extension_to_language_known_extensions() {
        assert_eq!(extension_to_language("main.rs"), "rust");
        assert_eq!(extension_to_language("app.py"), "python");
        assert_eq!(extension_to_language("index.tsx"), "tsx");
        assert_eq!(extension_to_language("Cargo.toml"), "toml");
        assert_eq!(extension_to_language("config.yaml"), "yaml");
    }

    #[test]
    fn extension_to_language_unknown_passes_through() {
        assert_eq!(extension_to_language("file.xyz"), "xyz");
    }

    #[test]
    fn file_icon_returns_folder_for_directory() {
        assert_eq!(file_icon("src", true), "\u{1F4C1}");
    }

    #[test]
    fn file_icon_returns_crab_for_rust() {
        assert_eq!(file_icon("main.rs", false), "\u{1F980}");
    }
}
