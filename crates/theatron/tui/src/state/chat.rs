use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::id::ToolId;

/// Stream lifecycle phase, modeled on Claude Code's state machine.
///
/// Drives the status indicator rendering: each phase has a distinct visual
/// representation in the TUI status bar and streaming section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StreamPhase {
    /// No active turn. Input is editable.
    #[default]
    Idle,
    /// HTTP request sent, waiting for first SSE event.
    Requesting,
    /// Receiving text deltas from the model.
    Streaming,
    /// Model is in an extended thinking block.
    Thinking,
    /// Context window compaction in progress.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "constructed when server emits compaction SSE events"
        )
    )]
    Compacting,
    /// Waiting for tool approval or external input.
    Waiting,
    /// Turn ended with an error.
    Error,
    /// Turn completed successfully. Transitions to Idle on next tick.
    Done,
}

impl std::fmt::Display for StreamPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Requesting => write!(f, "requesting"),
            Self::Streaming => write!(f, "streaming"),
            Self::Thinking => write!(f, "thinking"),
            Self::Compacting => write!(f, "compacting"),
            Self::Waiting => write!(f, "waiting"),
            Self::Error => write!(f, "error"),
            Self::Done => write!(f, "done"),
        }
    }
}

/// Semantic message category for rendering differentiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum MessageKind {
    /// Normal user or assistant message with full markdown rendering.
    #[default]
    Standard,
    /// Compact one-line tool status summary (not full JSON).
    #[expect(
        dead_code,
        reason = "SSE mapper variant, not yet constructed in TUI crate"
    )]
    ToolStatusLine,
    /// Compact thinking indicator line.
    #[expect(
        dead_code,
        reason = "SSE mapper variant, not yet constructed in TUI crate"
    )]
    ThinkingStatusLine,
    /// Distillation summary boundary marker.
    #[expect(
        dead_code,
        reason = "SSE mapper variant, not yet constructed in TUI crate"
    )]
    DistillationMarker,
    /// Visual separator between conversation topics.
    #[expect(
        dead_code,
        reason = "SSE mapper variant, not yet constructed in TUI crate"
    )]
    TopicBoundary,
}

/// Shared-ownership message history vector.
///
/// `Clone` increments an `Arc` reference count: O(1) regardless of message count.
/// `DerefMut` uses `Arc::make_mut` for copy-on-write semantics: mutation is free
/// when this is the sole owner, and clones the `Vec` exactly once when another
/// `Arc` still holds a reference.
///
/// PERF: Tab switches call `ArcVec::clone` (O(1)) instead of `Vec::clone` (O(n)).
#[derive(Debug)]
pub(crate) struct ArcVec<T>(Arc<Vec<T>>);

impl<T> Clone for ArcVec<T> {
    fn clone(&self) -> Self {
        ArcVec(Arc::clone(&self.0))
    }
}

impl<T> Default for ArcVec<T> {
    fn default() -> Self {
        ArcVec(Arc::new(Vec::new()))
    }
}

impl<T> Deref for ArcVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T: Clone> DerefMut for ArcVec<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        Arc::make_mut(&mut self.0)
    }
}

impl<T: Clone> FromIterator<T> for ArcVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        ArcVec(Arc::new(iter.into_iter().collect()))
    }
}

impl<T: Clone> From<Vec<T>> for ArcVec<T> {
    fn from(v: Vec<T>) -> Self {
        ArcVec(Arc::new(v))
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub name: String,
    pub tool_id: Option<ToolId>,
    pub duration_ms: Option<u64>,
    pub is_error: bool,
    /// Tool result text, stored for collapsible card rendering.
    pub output: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SavedScrollState {
    pub(crate) scroll_offset: usize,
    pub(crate) auto_scroll: bool,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
    /// Pre-lowercased `text`, cached at ingestion to avoid per-frame allocation in view code.
    pub text_lower: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
    /// Semantic category for rendering differentiation.
    pub kind: MessageKind,
}

/// Paired streaming-markdown text and its pre-rendered line cache.
///
/// Invariant: `lines` always corresponds to the rendering of `text` at `width`.
/// All fields are cleared or updated together via [`MarkdownCache::clear`].
#[derive(Debug, Clone, Default)]
pub(crate) struct MarkdownCache {
    pub(crate) text: String,
    /// Render width used to produce `lines`. The view checks this to detect
    /// width mismatches (e.g. sidebar toggled mid-stream) and re-renders.
    pub(crate) width: usize,
    pub(crate) lines: Vec<ratatui::text::Line<'static>>,
}

impl MarkdownCache {
    /// Clear the cached text, width, and rendered lines atomically.
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.width = 0;
        self.lines.clear();
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
mod tests {
    use super::*;

    #[test]
    fn arc_vec_clone_is_shared_not_copied() {
        let v: ArcVec<i32> = vec![1, 2, 3].into();
        let v2 = v.clone();
        // Both point to the same allocation: pointer equality.
        assert!(std::ptr::eq(Arc::as_ptr(&v.0), Arc::as_ptr(&v2.0)));
    }

    #[test]
    fn arc_vec_deref_mut_unshares_on_write() {
        let v: ArcVec<i32> = vec![1, 2, 3].into();
        let mut v2 = v.clone();
        // Shared before mutation.
        assert!(std::ptr::eq(Arc::as_ptr(&v.0), Arc::as_ptr(&v2.0)));
        // DerefMut triggers Arc::make_mut → COW clone.
        v2.push(4);
        // Now they differ.
        assert!(!std::ptr::eq(Arc::as_ptr(&v.0), Arc::as_ptr(&v2.0)));
        assert_eq!(*v, [1, 2, 3]);
        assert_eq!(*v2, [1, 2, 3, 4]);
    }

    #[test]
    fn arc_vec_from_iterator() {
        let v: ArcVec<u32> = (0u32..5).collect();
        assert_eq!(*v, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn arc_vec_from_vec() {
        let v: ArcVec<i32> = vec![10, 20].into();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], 10);
    }

    #[test]
    fn arc_vec_default_is_empty() {
        let v: ArcVec<i32> = ArcVec::default();
        assert!(v.is_empty());
    }

    #[test]
    fn stream_phase_default_is_idle() {
        assert_eq!(StreamPhase::default(), StreamPhase::Idle);
    }

    #[test]
    fn stream_phase_display_all_variants() {
        assert_eq!(StreamPhase::Idle.to_string(), "idle");
        assert_eq!(StreamPhase::Requesting.to_string(), "requesting");
        assert_eq!(StreamPhase::Streaming.to_string(), "streaming");
        assert_eq!(StreamPhase::Thinking.to_string(), "thinking");
        assert_eq!(StreamPhase::Compacting.to_string(), "compacting");
        assert_eq!(StreamPhase::Waiting.to_string(), "waiting");
        assert_eq!(StreamPhase::Error.to_string(), "error");
        assert_eq!(StreamPhase::Done.to_string(), "done");
    }

    #[test]
    fn message_kind_default_is_standard() {
        assert_eq!(MessageKind::default(), MessageKind::Standard);
    }
}
