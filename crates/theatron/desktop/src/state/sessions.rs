//! Session list, detail, and selection state for the sessions management view.

use std::collections::HashSet;

use theatron_core::api::types::Session;
use theatron_core::id::SessionId;

/// Sort field for session list ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SessionSort {
    /// Most recently active first.
    #[default]
    LastActivity,
    /// Highest token usage first.
    TokenUsage,
    /// Most messages first.
    MessageCount,
    /// Newest created first.
    CreatedDate,
}

impl SessionSort {
    /// All available sort options in display order.
    pub(crate) const ALL: &[Self] = &[
        Self::LastActivity,
        Self::TokenUsage,
        Self::MessageCount,
        Self::CreatedDate,
    ];

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::LastActivity => "Last Activity",
            Self::TokenUsage => "Token Usage",
            Self::MessageCount => "Messages",
            Self::CreatedDate => "Created",
        }
    }
}

/// Status filter for session list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum StatusFilter {
    /// Show all sessions.
    #[default]
    All,
    /// Active sessions only.
    Active,
    /// Idle sessions only.
    Idle,
    /// Archived sessions only.
    Archived,
}

impl StatusFilter {
    /// All available filter options.
    pub(crate) const ALL: &[Self] = &[Self::All, Self::Active, Self::Idle, Self::Archived];

    /// Human-readable label.
    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Active => "Active",
            Self::Idle => "Idle",
            Self::Archived => "Archived",
        }
    }
}

/// Paginated session list with sort and filter state.
#[derive(Debug, Clone)]
pub(crate) struct SessionListStore {
    /// Currently loaded sessions.
    pub sessions: Vec<Session>,
    /// Current sort field.
    pub sort: SessionSort,
    /// Current status filter.
    pub status_filter: StatusFilter,
    /// Agent ID filter (empty = all agents).
    pub agent_filter: Vec<String>,
    /// Search query text.
    pub search_query: String,
    /// Current page (0-indexed).
    pub page: usize,
    /// Whether more pages are available.
    pub has_more: bool,
    /// Total count if known from server.
    pub total_count: Option<usize>,
}

impl Default for SessionListStore {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            sort: SessionSort::default(),
            status_filter: StatusFilter::default(),
            agent_filter: Vec::new(),
            search_query: String::new(),
            page: 0,
            has_more: false,
            total_count: None,
        }
    }
}

impl SessionListStore {
    /// Number of sessions per page.
    pub(crate) const PAGE_SIZE: usize = 50;

    /// Create a new empty store.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Replace the session list with fresh data.
    pub(crate) fn load(&mut self, sessions: Vec<Session>, has_more: bool) {
        self.sessions = sessions;
        self.has_more = has_more;
    }

    /// Append more sessions (next page).
    pub(crate) fn append(&mut self, sessions: Vec<Session>, has_more: bool) {
        self.sessions.extend(sessions);
        self.has_more = has_more;
        self.page += 1;
    }

    /// Reset filters and pagination.
    pub(crate) fn clear_filters(&mut self) {
        self.search_query.clear();
        self.status_filter = StatusFilter::default();
        self.agent_filter.clear();
        self.page = 0;
    }

    /// Whether any filter is active.
    #[must_use]
    pub(crate) fn has_active_filters(&self) -> bool {
        !self.search_query.is_empty()
            || self.status_filter != StatusFilter::All
            || !self.agent_filter.is_empty()
    }

    /// Sort the loaded sessions client-side.
    pub(crate) fn sort_sessions(&mut self) {
        match self.sort {
            SessionSort::LastActivity => {
                self.sessions.sort_by(|a, b| {
                    b.updated_at
                        .as_deref()
                        .unwrap_or("")
                        .cmp(a.updated_at.as_deref().unwrap_or(""))
                });
            }
            SessionSort::TokenUsage => {
                // NOTE: token usage not available in Session struct from API;
                // fall back to message count as a proxy until API supports it.
                self.sessions
                    .sort_by(|a, b| b.message_count.cmp(&a.message_count));
            }
            SessionSort::MessageCount => {
                self.sessions
                    .sort_by(|a, b| b.message_count.cmp(&a.message_count));
            }
            SessionSort::CreatedDate => {
                // WHY: Session struct lacks created_at; use id (ULID-based, lexicographic = chronological).
                self.sessions
                    .sort_by(|a, b| b.id.as_ref().cmp(a.id.as_ref()));
            }
        }
    }
}

/// Detail view state for a single session.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionDetailStore {
    /// The session being viewed.
    pub session: Option<Session>,
    /// Total input tokens across all turns.
    pub input_tokens: u32,
    /// Total output tokens across all turns.
    pub output_tokens: u32,
    /// User message count.
    pub user_messages: u32,
    /// Assistant message count.
    pub assistant_messages: u32,
    /// Model used in the session.
    pub model: Option<String>,
    /// Session start time (first message).
    pub started_at: Option<String>,
    /// Session last activity (last message).
    pub ended_at: Option<String>,
    /// Distillation events for this session.
    pub distillation_events: Vec<DistillationEvent>,
    /// Message preview lines.
    pub message_previews: Vec<MessagePreview>,
}

impl SessionDetailStore {
    /// Total tokens (input + output).
    #[must_use]
    pub(crate) fn total_tokens(&self) -> u32 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// Whether token breakdown is available (vs only total).
    #[must_use]
    pub(crate) fn has_token_breakdown(&self) -> bool {
        self.input_tokens > 0 || self.output_tokens > 0
    }

    /// Reset to empty state.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// A distillation (context compaction) event.
#[derive(Debug, Clone)]
pub(crate) struct DistillationEvent {
    /// When distillation occurred.
    pub timestamp: String,
    /// What triggered it: manual, auto, threshold.
    pub trigger: String,
    /// Token count before compaction.
    pub tokens_before: u32,
    /// Token count after compaction.
    pub tokens_after: u32,
}

impl DistillationEvent {
    /// Compression ratio (0.0--1.0, lower = more compressed).
    #[must_use]
    pub(crate) fn compression_ratio(&self) -> f64 {
        if self.tokens_before == 0 {
            return 0.0;
        }
        f64::from(self.tokens_after) / f64::from(self.tokens_before)
    }

    /// Tokens saved by this distillation.
    #[must_use]
    pub(crate) fn tokens_saved(&self) -> u32 {
        self.tokens_before.saturating_sub(self.tokens_after)
    }
}

/// Abbreviated message for the session detail preview list.
#[derive(Debug, Clone)]
pub(crate) struct MessagePreview {
    /// Message role.
    pub role: String,
    /// First line or truncated content.
    pub summary: String,
    /// Timestamp if available.
    pub created_at: Option<String>,
}

/// Multi-select state for bulk operations.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionSelectionStore {
    /// Selected session IDs.
    selected: HashSet<SessionId>,
    /// Whether select-all is toggled.
    pub select_all: bool,
}

impl SessionSelectionStore {
    /// Create a new empty selection.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Toggle selection of a single session.
    pub(crate) fn toggle(&mut self, id: &SessionId) {
        if self.selected.contains(id) {
            self.selected.remove(id);
            self.select_all = false;
        } else {
            self.selected.insert(id.clone());
        }
    }

    /// Whether a session is selected.
    #[must_use]
    pub(crate) fn is_selected(&self, id: &SessionId) -> bool {
        self.selected.contains(id)
    }

    /// Set select-all state from the full session list.
    pub(crate) fn toggle_all(&mut self, all_ids: &[SessionId]) {
        if self.select_all {
            self.selected.clear();
            self.select_all = false;
        } else {
            self.selected = all_ids.iter().cloned().collect();
            self.select_all = true;
        }
    }

    /// Clear all selections.
    pub(crate) fn clear(&mut self) {
        self.selected.clear();
        self.select_all = false;
    }

    /// Number of selected sessions.
    #[must_use]
    pub(crate) fn count(&self) -> usize {
        self.selected.len()
    }

    /// Whether any sessions are selected.
    #[must_use]
    pub(crate) fn has_selection(&self) -> bool {
        !self.selected.is_empty()
    }

    /// Consume the selection, returning the IDs.
    pub(crate) fn take_selected(&mut self) -> Vec<SessionId> {
        self.select_all = false;
        self.selected.drain().collect()
    }
}

/// Format a relative time string from an ISO timestamp.
pub(crate) fn format_relative_time(timestamp: &str) -> String {
    // WHY: jiff is not in desktop crate deps; parse enough of ISO 8601
    // to produce a useful relative label without pulling in a time crate.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let ts_secs = parse_iso_to_unix(timestamp).unwrap_or(0);

    if ts_secs == 0 || now_secs == 0 {
        return timestamp.to_string();
    }

    let delta = now_secs.saturating_sub(ts_secs);

    if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        let mins = delta / 60;
        format!("{mins}m ago")
    } else if delta < 86400 {
        let hours = delta / 3600;
        format!("{hours}h ago")
    } else {
        let days = delta / 86400;
        format!("{days}d ago")
    }
}

/// Minimal ISO 8601 → unix seconds parser.
pub(crate) fn parse_iso_to_unix(s: &str) -> Option<u64> {
    // Accepts: "2025-01-15T10:30:00Z" or "2025-01-15T10:30:00+00:00"
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }

    let year: u64 = s.get(0..4)?.parse().ok()?;
    let month: u64 = s.get(5..7)?.parse().ok()?;
    let day: u64 = s.get(8..10)?.parse().ok()?;
    let hour: u64 = s.get(11..13)?.parse().ok()?;
    let min: u64 = s.get(14..16)?.parse().ok()?;
    let sec: u64 = s.get(17..19)?.parse().ok()?;

    // Simplified days-since-epoch (no leap second precision needed for relative display).
    let mut days = 0u64;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    let month_days = [
        31,
        28 + u64::from(is_leap(year)),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for m in 0..(month.saturating_sub(1) as usize) {
        days += month_days.get(m).copied().unwrap_or(30);
    }
    days += day.saturating_sub(1);

    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Infer session status for display from the Session struct.
pub(crate) fn session_display_status(session: &Session) -> &'static str {
    if session.is_archived() {
        "archived"
    } else if session.status.as_deref() == Some("active") {
        "active"
    } else {
        "idle"
    }
}

/// CSS color for a session status.
pub(crate) fn status_color(status: &str) -> &'static str {
    match status {
        "active" => "#22c55e",
        "idle" => "#d4a017",
        "archived" => "#666",
        _ => "#888",
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn make_session(id: &str, key: &str) -> Session {
        Session {
            id: id.into(),
            nous_id: "agent-1".into(),
            key: key.to_string(),
            status: Some("active".to_string()),
            message_count: 10,
            session_type: None,
            updated_at: Some("2025-06-15T10:00:00Z".to_string()),
            display_name: None,
        }
    }

    #[test]
    fn session_list_store_defaults() {
        let store = SessionListStore::new();
        assert!(store.sessions.is_empty());
        assert_eq!(store.sort, SessionSort::LastActivity);
        assert_eq!(store.status_filter, StatusFilter::All);
        assert!(!store.has_active_filters());
    }

    #[test]
    fn session_list_store_load() {
        let mut store = SessionListStore::new();
        store.load(vec![make_session("s1", "chat")], false);
        assert_eq!(store.sessions.len(), 1);
        assert!(!store.has_more);
    }

    #[test]
    fn session_list_store_append() {
        let mut store = SessionListStore::new();
        store.load(vec![make_session("s1", "chat")], true);
        store.append(vec![make_session("s2", "debug")], false);
        assert_eq!(store.sessions.len(), 2);
        assert_eq!(store.page, 1);
        assert!(!store.has_more);
    }

    #[test]
    fn session_list_store_clear_filters() {
        let mut store = SessionListStore::new();
        store.search_query = "test".to_string();
        store.status_filter = StatusFilter::Active;
        store.agent_filter = vec!["agent-1".to_string()];
        store.page = 3;
        assert!(store.has_active_filters());
        store.clear_filters();
        assert!(!store.has_active_filters());
        assert_eq!(store.page, 0);
    }

    #[test]
    fn session_list_store_sort_by_message_count() {
        let mut store = SessionListStore::new();
        let mut s1 = make_session("s1", "few");
        s1.message_count = 5;
        let mut s2 = make_session("s2", "many");
        s2.message_count = 50;
        store.load(vec![s1, s2], false);
        store.sort = SessionSort::MessageCount;
        store.sort_sessions();
        assert_eq!(store.sessions[0].message_count, 50);
        assert_eq!(store.sessions[1].message_count, 5);
    }

    #[test]
    fn session_list_store_sort_by_last_activity() {
        let mut store = SessionListStore::new();
        let mut s1 = make_session("s1", "old");
        s1.updated_at = Some("2025-01-01T00:00:00Z".to_string());
        let mut s2 = make_session("s2", "new");
        s2.updated_at = Some("2025-06-01T00:00:00Z".to_string());
        store.load(vec![s1, s2], false);
        store.sort = SessionSort::LastActivity;
        store.sort_sessions();
        assert_eq!(store.sessions[0].key, "new");
    }

    #[test]
    fn session_detail_store_total_tokens() {
        let mut detail = SessionDetailStore::default();
        detail.input_tokens = 1000;
        detail.output_tokens = 500;
        assert_eq!(detail.total_tokens(), 1500);
        assert!(detail.has_token_breakdown());
    }

    #[test]
    fn session_detail_store_no_token_breakdown() {
        let detail = SessionDetailStore::default();
        assert_eq!(detail.total_tokens(), 0);
        assert!(!detail.has_token_breakdown());
    }

    #[test]
    fn distillation_event_compression_ratio() {
        let event = DistillationEvent {
            timestamp: "2025-06-15T10:00:00Z".to_string(),
            trigger: "auto".to_string(),
            tokens_before: 10000,
            tokens_after: 3000,
        };
        assert!((event.compression_ratio() - 0.3).abs() < f64::EPSILON);
        assert_eq!(event.tokens_saved(), 7000);
    }

    #[test]
    fn distillation_event_zero_before() {
        let event = DistillationEvent {
            timestamp: String::new(),
            trigger: String::new(),
            tokens_before: 0,
            tokens_after: 0,
        };
        assert!((event.compression_ratio()).abs() < f64::EPSILON);
    }

    #[test]
    fn selection_store_toggle() {
        let mut sel = SessionSelectionStore::new();
        let id: SessionId = "s1".into();
        assert!(!sel.is_selected(&id));
        sel.toggle(&id);
        assert!(sel.is_selected(&id));
        assert_eq!(sel.count(), 1);
        sel.toggle(&id);
        assert!(!sel.is_selected(&id));
        assert_eq!(sel.count(), 0);
    }

    #[test]
    fn selection_store_toggle_all() {
        let mut sel = SessionSelectionStore::new();
        let ids: Vec<SessionId> = vec!["s1".into(), "s2".into(), "s3".into()];
        sel.toggle_all(&ids);
        assert!(sel.select_all);
        assert_eq!(sel.count(), 3);
        sel.toggle_all(&ids);
        assert!(!sel.select_all);
        assert_eq!(sel.count(), 0);
    }

    #[test]
    fn selection_store_take_selected() {
        let mut sel = SessionSelectionStore::new();
        sel.toggle(&"s1".into());
        sel.toggle(&"s2".into());
        let taken = sel.take_selected();
        assert_eq!(taken.len(), 2);
        assert!(!sel.has_selection());
    }

    #[test]
    fn format_relative_time_iso() {
        // Cannot test exact output without controlling system time,
        // but verify it doesn't panic and returns a string.
        let result = format_relative_time("2025-01-01T00:00:00Z");
        assert!(!result.is_empty());
    }

    #[test]
    fn format_relative_time_unparseable() {
        let result = format_relative_time("not-a-date");
        assert_eq!(result, "not-a-date");
    }

    #[test]
    fn parse_iso_to_unix_valid() {
        let secs = parse_iso_to_unix("2025-01-01T00:00:00Z").unwrap();
        // 2025-01-01 = 20089 days since epoch → 1735689600
        assert_eq!(secs, 1735689600);
    }

    #[test]
    fn parse_iso_to_unix_invalid() {
        assert!(parse_iso_to_unix("bad").is_none());
    }

    #[test]
    fn session_display_status_active() {
        let s = make_session("s1", "chat");
        assert_eq!(session_display_status(&s), "active");
    }

    #[test]
    fn session_display_status_archived() {
        let mut s = make_session("s1", "chat");
        s.status = Some("archived".to_string());
        assert_eq!(session_display_status(&s), "archived");
    }

    #[test]
    fn session_display_status_idle() {
        let mut s = make_session("s1", "chat");
        s.status = Some("idle".to_string());
        assert_eq!(session_display_status(&s), "idle");
    }

    #[test]
    fn status_color_values() {
        assert_eq!(status_color("active"), "#22c55e");
        assert_eq!(status_color("idle"), "#d4a017");
        assert_eq!(status_color("archived"), "#666");
        assert_eq!(status_color("unknown"), "#888");
    }

    #[test]
    fn session_sort_labels() {
        for sort in SessionSort::ALL {
            assert!(!sort.label().is_empty());
        }
    }

    #[test]
    fn status_filter_labels() {
        for filter in StatusFilter::ALL {
            assert!(!filter.label().is_empty());
        }
    }
}
