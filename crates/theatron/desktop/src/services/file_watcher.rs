//! File change event handler: deduplicates rapid edits and triggers toast notifications.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use theatron_core::events::StreamEvent;

/// Deduplication window: suppress repeat notifications for the same file
/// within this duration (covers rapid search-replace sequences).
const DEDUP_WINDOW: Duration = Duration::from_secs(2);

/// Tool names that indicate file modifications.
const FILE_EDIT_TOOLS: &[&str] = &[
    "file_edit",
    "file_write",
    "write_file",
    "edit_file",
    "create_file",
    "patch_file",
    "str_replace_editor",
];

/// Type of file change detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileChangeKind {
    Modified,
    Created,
}

/// A deduplicated file change event ready for notification.
#[derive(Debug, Clone)]
pub(crate) struct FileChangeEvent {
    pub path: String,
    pub kind: FileChangeKind,
}

/// Tracks file modifications from streaming tool results, deduplicating
/// rapid changes to the same file.
pub(crate) struct FileChangeTracker {
    last_notified: HashMap<String, Instant>,
    /// Pending tool starts that might produce file change events.
    /// Maps tool_id → (tool_name, file_path).
    pending_tools: HashMap<String, (String, Option<String>)>,
}

impl FileChangeTracker {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            last_notified: HashMap::new(),
            pending_tools: HashMap::new(),
        }
    }

    /// Process a stream event, returning a file change event if one should
    /// be notified (after deduplication).
    pub(crate) fn process(&mut self, event: &StreamEvent) -> Option<FileChangeEvent> {
        match event {
            StreamEvent::ToolStart {
                tool_name,
                tool_id,
                input,
            } => {
                if is_file_edit_tool(tool_name) {
                    let path = input.as_ref().and_then(extract_file_path);
                    self.pending_tools
                        .insert(tool_id.to_string(), (tool_name.clone(), path));
                }
                None
            }
            StreamEvent::ToolResult {
                tool_id, is_error, ..
            } => {
                let entry = self.pending_tools.remove(&tool_id.to_string());
                if *is_error {
                    return None;
                }
                let (tool_name, path) = entry?;
                let path = path?;

                let now = Instant::now();
                if let Some(last) = self.last_notified.get(&path) {
                    if now.duration_since(*last) < DEDUP_WINDOW {
                        return None;
                    }
                }

                self.last_notified.insert(path.clone(), now);

                let kind = if tool_name.contains("create") || tool_name.contains("write") {
                    FileChangeKind::Created
                } else {
                    FileChangeKind::Modified
                };

                Some(FileChangeEvent { path, kind })
            }
            _ => None,
        }
    }

    /// Clean up stale entries older than the dedup window.
    pub(crate) fn gc(&mut self) {
        let cutoff = Instant::now() - DEDUP_WINDOW * 2;
        self.last_notified.retain(|_, t| *t > cutoff);
    }
}

impl Default for FileChangeTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a tool name indicates a file-editing operation.
fn is_file_edit_tool(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    FILE_EDIT_TOOLS.iter().any(|t| lower.contains(t))
}

/// Extract the file path from a tool input JSON value.
///
/// Looks for common field names: `path`, `file_path`, `filename`, `file`.
fn extract_file_path(input: &serde_json::Value) -> Option<String> {
    let obj = input.as_object()?;
    for key in &["path", "file_path", "filename", "file"] {
        if let Some(v) = obj.get(*key) {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// Format a toast title for a file change event.
#[must_use]
pub(crate) fn toast_title(kind: &FileChangeKind) -> &'static str {
    match kind {
        FileChangeKind::Modified => "File modified",
        FileChangeKind::Created => "File created",
    }
}

/// Truncate a path to fit toast body constraints.
#[must_use]
pub(crate) fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    // NOTE: Show the tail of the path (most informative part).
    let suffix = &path[path.len() - (max_len - 3)..];
    format!("...{suffix}")
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use theatron_core::id::ToolId;

    use super::*;

    fn tool_start(name: &str, tool_id: &str, path: &str) -> StreamEvent {
        StreamEvent::ToolStart {
            tool_name: name.to_string(),
            tool_id: ToolId::from(tool_id),
            input: Some(serde_json::json!({ "path": path })),
        }
    }

    fn tool_result(tool_id: &str, is_error: bool) -> StreamEvent {
        StreamEvent::ToolResult {
            tool_name: "file_edit".to_string(),
            tool_id: ToolId::from(tool_id),
            is_error,
            duration_ms: 10,
            result: None,
        }
    }

    #[test]
    fn detects_file_edit_and_produces_event() {
        let mut tracker = FileChangeTracker::new();

        let start = tool_start("file_edit", "t1", "src/main.rs");
        assert!(tracker.process(&start).is_none());

        let result = tool_result("t1", false);
        let event = tracker.process(&result).unwrap();
        assert_eq!(event.path, "src/main.rs");
        assert_eq!(event.kind, FileChangeKind::Modified);
    }

    #[test]
    fn deduplicates_rapid_changes() {
        let mut tracker = FileChangeTracker::new();

        // First edit
        tracker.process(&tool_start("file_edit", "t1", "src/lib.rs"));
        let first = tracker.process(&tool_result("t1", false));
        assert!(first.is_some());

        // Second edit to same file within dedup window
        tracker.process(&tool_start("file_edit", "t2", "src/lib.rs"));
        let second = tracker.process(&tool_result("t2", false));
        assert!(second.is_none(), "should be deduplicated");
    }

    #[test]
    fn different_files_not_deduplicated() {
        let mut tracker = FileChangeTracker::new();

        tracker.process(&tool_start("file_edit", "t1", "src/a.rs"));
        assert!(tracker.process(&tool_result("t1", false)).is_some());

        tracker.process(&tool_start("file_edit", "t2", "src/b.rs"));
        assert!(tracker.process(&tool_result("t2", false)).is_some());
    }

    #[test]
    fn error_result_suppresses_notification() {
        let mut tracker = FileChangeTracker::new();

        tracker.process(&tool_start("file_edit", "t1", "src/main.rs"));
        let event = tracker.process(&tool_result("t1", true));
        assert!(event.is_none());
    }

    #[test]
    fn non_file_tools_ignored() {
        let mut tracker = FileChangeTracker::new();

        let start = StreamEvent::ToolStart {
            tool_name: "web_search".to_string(),
            tool_id: ToolId::from("t1"),
            input: Some(serde_json::json!({ "query": "rust" })),
        };
        assert!(tracker.process(&start).is_none());
    }

    #[test]
    fn create_file_detected_as_created() {
        let mut tracker = FileChangeTracker::new();

        tracker.process(&tool_start("create_file", "t1", "src/new.rs"));
        let event = tracker.process(&tool_result("t1", false)).unwrap();
        assert_eq!(event.kind, FileChangeKind::Created);
    }

    #[test]
    fn truncate_path_short_unchanged() {
        assert_eq!(truncate_path("src/main.rs", 50), "src/main.rs");
    }

    #[test]
    fn truncate_path_long_shows_tail() {
        let long_path = "very/long/path/to/some/deeply/nested/file.rs";
        let truncated = truncate_path(long_path, 20);
        assert!(truncated.starts_with("..."));
        assert!(truncated.len() <= 20);
        assert!(truncated.ends_with("file.rs"));
    }

    #[test]
    fn extract_path_from_various_keys() {
        let val = serde_json::json!({ "file_path": "/tmp/foo.rs" });
        assert_eq!(extract_file_path(&val).unwrap(), "/tmp/foo.rs");

        let val = serde_json::json!({ "filename": "bar.py" });
        assert_eq!(extract_file_path(&val).unwrap(), "bar.py");
    }

    #[test]
    fn gc_removes_stale_entries() {
        let mut tracker = FileChangeTracker::new();
        tracker.last_notified.insert(
            "old.rs".to_string(),
            Instant::now() - Duration::from_secs(10),
        );
        tracker
            .last_notified
            .insert("new.rs".to_string(), Instant::now());
        tracker.gc();
        assert!(!tracker.last_notified.contains_key("old.rs"));
        assert!(tracker.last_notified.contains_key("new.rs"));
    }
}
