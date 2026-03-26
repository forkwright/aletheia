//! Unified diff parser for post-merge lesson extraction.
//!
//! Parses standard unified diff format (as produced by `git diff`) into
//! structured `DiffFile` and `DiffHunk` records.

use serde::{Deserialize, Serialize};

/// A complete parsed diff containing one or more file changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedDiff {
    /// Individual file diffs.
    pub files: Vec<DiffFile>,
}

/// A single file's diff within a unified diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFile {
    /// Path of the file before the change (may be `/dev/null` for new files).
    pub old_path: String,
    /// Path of the file after the change (may be `/dev/null` for deleted files).
    pub new_path: String,
    /// Whether this is a new file.
    pub is_new: bool,
    /// Whether this file was deleted.
    pub is_deleted: bool,
    /// Individual change hunks within the file.
    pub hunks: Vec<DiffHunk>,
}

impl DiffFile {
    /// Return the effective file path (prefers `new_path` unless deleted).
    #[must_use]
    pub(crate) fn effective_path(&self) -> &str {
        if self.is_deleted {
            &self.old_path
        } else {
            &self.new_path
        }
    }
}

/// A single hunk within a file diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Starting line number in the old file.
    pub old_start: u32,
    /// Number of lines in the old file.
    pub old_count: u32,
    /// Starting line number in the new file.
    pub new_start: u32,
    /// Number of lines in the new file.
    pub new_count: u32,
    /// Optional hunk header context (function/class name).
    pub context: String,
    /// Lines added in this hunk.
    pub additions: Vec<String>,
    /// Lines removed in this hunk.
    pub deletions: Vec<String>,
}

/// Parse a unified diff string into structured records.
///
/// Handles standard `git diff` output including `---`/`+++` file headers,
/// `@@ ... @@` hunk headers, and `+`/`-` change lines.
///
/// # Errors
///
/// Returns `None` lines or sections gracefully (empty files/hunks).
/// Does not fail on malformed input; skips unrecognized lines.
#[must_use]
pub(crate) fn parse_unified_diff(diff: &str) -> ParsedDiff {
    let mut files = Vec::new();
    let mut current_file: Option<DiffFile> = None;
    let mut current_hunk: Option<DiffHunk> = None;

    for line in diff.lines() {
        if line.starts_with("diff --git") {
            // Flush previous hunk and file.
            flush_hunk(&mut current_file, &mut current_hunk);
            flush_file(&mut files, &mut current_file);

            // Start new file (paths will be set by --- and +++ lines).
            current_file = Some(DiffFile {
                old_path: String::new(),
                new_path: String::new(),
                is_new: false,
                is_deleted: false,
                hunks: Vec::new(),
            });
        } else if line.starts_with("--- ") {
            if let Some(ref mut file) = current_file {
                let path = strip_prefix(line.get(4..).unwrap_or(""));
                if path == "/dev/null" {
                    file.is_new = true;
                }
                path.clone_into(&mut file.old_path);
            }
        } else if line.starts_with("+++ ") {
            if let Some(ref mut file) = current_file {
                let path = strip_prefix(line.get(4..).unwrap_or(""));
                if path == "/dev/null" {
                    file.is_deleted = true;
                }
                path.clone_into(&mut file.new_path);
            }
        } else if line.starts_with("@@ ") {
            flush_hunk(&mut current_file, &mut current_hunk);
            current_hunk = Some(parse_hunk_header(line));
        } else if let Some(ref mut hunk) = current_hunk {
            if let Some(added) = line.strip_prefix('+') {
                hunk.additions.push(added.to_owned());
            } else if let Some(removed) = line.strip_prefix('-') {
                hunk.deletions.push(removed.to_owned());
            }
            // Context lines (starting with ' ') are ignored for extraction purposes.
        }
    }

    flush_hunk(&mut current_file, &mut current_hunk);
    flush_file(&mut files, &mut current_file);

    ParsedDiff { files }
}

/// Strip `a/` or `b/` prefix from git diff paths.
fn strip_prefix(path: &str) -> &str {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
}

/// Parse `@@ -old_start,old_count +new_start,new_count @@ context`.
fn parse_hunk_header(line: &str) -> DiffHunk {
    let mut old_start = 0u32;
    let mut old_count = 0u32;
    let mut new_start = 0u32;
    let mut new_count = 0u32;
    let mut context = String::new();

    // Find the range between @@ markers.
    if let Some(rest) = line.strip_prefix("@@ ")
        && let Some(end_idx) = rest.find(" @@")
    {
        let range_part = rest.get(..end_idx).unwrap_or("");
        rest.get(end_idx + 3..)
            .unwrap_or("")
            .trim()
            .clone_into(&mut context);

        let parts: Vec<&str> = range_part.split_whitespace().collect();
        if let Some(old_range) = parts.first() {
            let old_range = old_range.strip_prefix('-').unwrap_or(old_range);
            parse_range(old_range, &mut old_start, &mut old_count);
        }
        if let Some(new_range) = parts.get(1) {
            let new_range = new_range.strip_prefix('+').unwrap_or(new_range);
            parse_range(new_range, &mut new_start, &mut new_count);
        }
    }

    DiffHunk {
        old_start,
        old_count,
        new_start,
        new_count,
        context,
        additions: Vec::new(),
        deletions: Vec::new(),
    }
}

/// Parse `start,count` or `start` into the output variables.
fn parse_range(s: &str, start: &mut u32, count: &mut u32) {
    if let Some((s_str, c_str)) = s.split_once(',') {
        *start = s_str.parse().unwrap_or(0);
        *count = c_str.parse().unwrap_or(0);
    } else {
        *start = s.parse().unwrap_or(0);
        *count = 1;
    }
}

fn flush_hunk(file: &mut Option<DiffFile>, hunk: &mut Option<DiffHunk>) {
    if let (Some(f), Some(h)) = (file, hunk.take()) {
        f.hunks.push(h);
    }
}

fn flush_file(files: &mut Vec<DiffFile>, file: &mut Option<DiffFile>) {
    if let Some(f) = file.take() {
        files.push(f);
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::needless_raw_string_hashes,
    reason = "test assertions"
)]
mod tests {
    use super::*;

    const SAMPLE_DIFF: &str = r#"diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,7 +10,8 @@ fn main() {
     let config = load_config();
-    let server = Server::new(config);
+    let server = Server::new(config.clone());
+    let monitor = Monitor::new(config);
     server.run();
diff --git a/src/monitor.rs b/src/monitor.rs
--- /dev/null
+++ b/src/monitor.rs
@@ -0,0 +1,5 @@
+pub struct Monitor {
+    config: Config,
+}
+
+impl Monitor {}
"#;

    #[test]
    fn parses_multi_file_diff() {
        let parsed = parse_unified_diff(SAMPLE_DIFF);
        assert_eq!(parsed.files.len(), 2, "should parse two file diffs");

        let first = &parsed.files[0];
        assert_eq!(first.old_path, "src/main.rs", "old path for first file");
        assert_eq!(first.new_path, "src/main.rs", "new path for first file");
        assert!(!first.is_new, "first file is not new");
        assert!(!first.is_deleted, "first file is not deleted");
        assert_eq!(first.hunks.len(), 1, "one hunk in first file");
        assert_eq!(
            first.hunks[0].additions.len(),
            2,
            "two additions in first hunk"
        );
        assert_eq!(
            first.hunks[0].deletions.len(),
            1,
            "one deletion in first hunk"
        );

        let second = &parsed.files[1];
        assert_eq!(
            second.new_path, "src/monitor.rs",
            "new path for second file"
        );
        assert!(second.is_new, "second file is new");
        assert_eq!(
            second.hunks[0].additions.len(),
            5,
            "five additions in new file"
        );
    }

    #[test]
    fn parses_hunk_header_with_context() {
        let hunk = parse_hunk_header("@@ -10,7 +10,8 @@ fn main() {");
        assert_eq!(hunk.old_start, 10, "old start line");
        assert_eq!(hunk.old_count, 7, "old line count");
        assert_eq!(hunk.new_start, 10, "new start line");
        assert_eq!(hunk.new_count, 8, "new line count");
        assert_eq!(hunk.context, "fn main() {", "hunk context");
    }

    #[test]
    fn handles_empty_diff() {
        let parsed = parse_unified_diff("");
        assert!(parsed.files.is_empty(), "empty diff has no files");
    }

    #[test]
    fn handles_deleted_file() {
        let diff = r#"diff --git a/src/old.rs b/src/old.rs
--- a/src/old.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-pub fn old_function() {}
-pub fn another() {}
-pub fn third() {}
"#;
        let parsed = parse_unified_diff(diff);
        assert_eq!(parsed.files.len(), 1, "one file in diff");
        assert!(parsed.files[0].is_deleted, "file is deleted");
        assert_eq!(
            parsed.files[0].hunks[0].deletions.len(),
            3,
            "three deletions"
        );
    }
}
