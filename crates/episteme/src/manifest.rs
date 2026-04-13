//! Memory manifest: lightweight headers for side-query pre-filtering.
//!
//! Generates a compact index of available memory entries (name + description)
//! without reading full content. Capped at [`MAX_MEMORY_ENTRIES`], sorted
//! by modification time descending (newest first).
//!
//! Adapted from CC's `memoryScan.ts` pattern: single-pass header extraction
//! followed by mtime sort, avoiding full content reads for irrelevant entries.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum number of memory entries in a single manifest.
///
/// Limits the side-query prompt size and LLM processing cost.
/// Matches CC's `MAX_MEMORY_FILES` constant.
pub(crate) const MAX_MEMORY_ENTRIES: usize = 200;

/// A lightweight header for a single memory entry.
///
/// Contains only identification and metadata fields — never the full content.
/// Analogous to CC's `MemoryHeader` but adapted for knowledge-store facts
/// rather than filesystem files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "MemoryHeaderRaw")]
pub struct MemoryHeader {
    /// Source identifier (fact ID, document reference, or path).
    pub source_id: String,
    /// Short name or title for this memory entry.
    pub name: String,
    /// Brief description extracted from entry metadata.
    pub description: Option<String>,
    /// Modification time in milliseconds since epoch.
    pub mtime_ms: i64,
}

/// Raw deserialization type for [`MemoryHeader`].
#[derive(Debug, Clone, Deserialize)]
struct MemoryHeaderRaw {
    source_id: String,
    name: String,
    description: Option<String>,
    mtime_ms: i64,
}

impl From<MemoryHeaderRaw> for MemoryHeader {
    fn from(raw: MemoryHeaderRaw) -> Self {
        Self {
            source_id: raw.source_id,
            name: raw.name,
            description: raw.description,
            mtime_ms: raw.mtime_ms,
        }
    }
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "test-only constructors; MemoryHeader is built via serde in lib builds")
)]
impl MemoryHeader {
    /// Create a new header with the required fields.
    #[must_use]
    pub(crate) fn new(source_id: impl Into<String>, name: impl Into<String>, mtime_ms: i64) -> Self {
        Self {
            source_id: source_id.into(),
            name: name.into(),
            description: None,
            mtime_ms,
        }
    }

    /// Set the description (builder pattern).
    #[must_use]
    pub(crate) fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl fmt::Display for MemoryHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.source_id, self.name)
    }
}

/// A manifest of memory entry headers for side-query pre-filtering.
///
/// Provides a compact summary of available memories that can be sent to a
/// lightweight model for relevance ranking without transferring full content.
///
/// # Invariants
///
/// - Entries are sorted by `mtime_ms` descending (newest first).
/// - Length never exceeds [`MAX_MEMORY_ENTRIES`].
#[derive(Debug, Clone)]
pub struct MemoryManifest {
    headers: Vec<MemoryHeader>,
}

impl MemoryManifest {
    /// Build a manifest from raw headers.
    ///
    /// Sorts by modification time descending and enforces the
    /// [`MAX_MEMORY_ENTRIES`] cap.
    #[must_use]
    pub(crate) fn from_headers(mut headers: Vec<MemoryHeader>) -> Self {
        headers.sort_by_key(|h| std::cmp::Reverse(h.mtime_ms));
        headers.truncate(MAX_MEMORY_ENTRIES);
        Self { headers }
    }

    /// Format the manifest as a text block suitable for the side-query prompt.
    ///
    /// Each entry occupies a single line:
    /// `- <source_id> <name>: <description>` (with description)
    /// `- <source_id> <name>` (without description)
    #[must_use]
    pub(crate) fn format(&self) -> String {
        use std::fmt::Write;
        let mut out = String::with_capacity(self.headers.len() * 80);
        for h in &self.headers {
            let _ = write!(out, "- {} {}", h.source_id, h.name);
            if let Some(desc) = &h.description {
                let _ = write!(out, ": {desc}");
            }
            out.push('\n');
        }
        out
    }

    /// The headers in this manifest, in mtime-descending order.
    #[must_use]
    pub(crate) fn headers(&self) -> &[MemoryHeader] {
        &self.headers
    }

    /// Number of entries in this manifest.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.headers.len()
    }

    /// Whether this manifest contains no entries.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }
}

impl fmt::Display for MemoryManifest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MemoryManifest({} entries)", self.headers.len())
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn make_header(id: &str, name: &str, mtime: i64) -> MemoryHeader {
        MemoryHeader::new(id, name, mtime)
    }

    fn make_header_with_desc(id: &str, name: &str, desc: &str, mtime: i64) -> MemoryHeader {
        MemoryHeader::new(id, name, mtime).with_description(desc)
    }

    #[test]
    fn empty_headers_produces_empty_manifest() {
        let manifest = MemoryManifest::from_headers(vec![]);
        assert!(
            manifest.is_empty(),
            "manifest from empty headers should be empty"
        );
        assert_eq!(manifest.len(), 0, "manifest length should be 0");
    }

    #[test]
    fn sorts_by_mtime_descending() {
        let headers = vec![
            make_header("old", "oldest", 100),
            make_header("new", "newest", 300),
            make_header("mid", "middle", 200),
        ];
        let manifest = MemoryManifest::from_headers(headers);
        let ids: Vec<&str> = manifest
            .headers()
            .iter()
            .map(|h| h.source_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["new", "mid", "old"],
            "headers should be sorted newest first"
        );
    }

    #[test]
    fn caps_at_max_entries() {
        let headers: Vec<MemoryHeader> = (0..250)
            .map(|i| make_header(&format!("id-{i}"), &format!("entry-{i}"), i64::from(i)))
            .collect();
        let manifest = MemoryManifest::from_headers(headers);
        assert_eq!(
            manifest.len(),
            MAX_MEMORY_ENTRIES,
            "manifest should cap at {MAX_MEMORY_ENTRIES}"
        );
    }

    #[test]
    fn cap_retains_newest_entries() {
        let headers: Vec<MemoryHeader> = (0..250)
            .map(|i| make_header(&format!("id-{i}"), &format!("entry-{i}"), i64::from(i)))
            .collect();
        let manifest = MemoryManifest::from_headers(headers);
        // WHY: after sorting desc and truncating, the first entry should be the newest.
        let first = manifest.headers().first();
        assert!(
            first.is_some(),
            "manifest should not be empty after capping"
        );
        assert_eq!(
            first.map(|h| h.source_id.as_str()),
            Some("id-249"),
            "first entry should be the newest (id-249)"
        );
    }

    #[test]
    fn format_includes_source_id_and_name() {
        let headers = vec![make_header("fact-001", "user preferences", 1000)];
        let manifest = MemoryManifest::from_headers(headers);
        let text = manifest.format();
        assert!(
            text.contains("fact-001"),
            "formatted manifest should contain source_id"
        );
        assert!(
            text.contains("user preferences"),
            "formatted manifest should contain name"
        );
    }

    #[test]
    fn format_includes_description_when_present() {
        let headers = vec![make_header_with_desc(
            "fact-002",
            "coding style",
            "Prefers Rust with snafu errors",
            2000,
        )];
        let manifest = MemoryManifest::from_headers(headers);
        let text = manifest.format();
        assert!(
            text.contains("Prefers Rust with snafu errors"),
            "formatted manifest should include description"
        );
    }

    #[test]
    fn format_omits_description_when_none() {
        let headers = vec![make_header("fact-003", "meeting notes", 3000)];
        let manifest = MemoryManifest::from_headers(headers);
        let text = manifest.format();
        // NOTE: without description, the line should end after the name.
        assert!(
            !text.contains(':'),
            "formatted manifest should not contain colon separator without description"
        );
    }

    #[test]
    fn header_display_shows_id_and_name() {
        let header = make_header("fact-010", "project setup", 5000);
        let display = header.to_string();
        assert!(
            display.contains("fact-010"),
            "display should include source_id"
        );
        assert!(
            display.contains("project setup"),
            "display should include name"
        );
    }

    #[test]
    fn manifest_display_shows_count() {
        let headers = vec![make_header("a", "alpha", 1), make_header("b", "beta", 2)];
        let manifest = MemoryManifest::from_headers(headers);
        assert_eq!(manifest.to_string(), "MemoryManifest(2 entries)");
    }

    #[test]
    fn header_builder_pattern() {
        let header = MemoryHeader::new("id-1", "name-1", 100).with_description("a description");
        assert_eq!(header.source_id, "id-1");
        assert_eq!(header.name, "name-1");
        assert_eq!(header.description.as_deref(), Some("a description"));
        assert_eq!(header.mtime_ms, 100);
    }

    #[test]
    fn from_headers_with_single_entry() {
        let manifest = MemoryManifest::from_headers(vec![make_header("only", "the one", 42)]);
        assert_eq!(manifest.len(), 1, "single entry manifest");
        assert!(!manifest.is_empty(), "single entry is not empty");
    }

    #[test]
    fn from_headers_preserves_all_within_cap() {
        let count = MAX_MEMORY_ENTRIES - 1;
        let headers: Vec<MemoryHeader> = (0..count)
            .map(|i| make_header(&format!("id-{i}"), &format!("n-{i}"), i.try_into().expect("count < MAX_MEMORY_ENTRIES (200) fits i64")))
            .collect();
        let manifest = MemoryManifest::from_headers(headers);
        assert_eq!(
            manifest.len(),
            count,
            "all entries within cap should be preserved"
        );
    }
}
