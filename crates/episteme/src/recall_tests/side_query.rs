//! Tests for side-query memory relevance selection, caching, and pre-filter
//! integration with the recall pipeline.

use std::collections::HashSet;

use crate::manifest::{MAX_MEMORY_ENTRIES, MemoryHeader, MemoryManifest};
use crate::recall::{FactorScores, ScoredResult, pre_filter_by_side_query};
use crate::side_query::{
    RankerFailedSnafu, SideQueryConfig, SideQueryError, SideQueryRanker, SideQuerySelector,
};

// ── Mock rankers ──────────────────────────────────────────────────────────

/// Returns a fixed list of source IDs, respecting `max_results`.
struct MockRanker {
    response: Vec<String>,
}

impl MockRanker {
    fn new(ids: Vec<&str>) -> Self {
        Self {
            response: ids.into_iter().map(String::from).collect(),
        }
    }
}

impl SideQueryRanker for MockRanker {
    fn rank_memories(
        &self,
        _query: &str,
        _manifest_text: &str,
        max_results: usize,
    ) -> Result<Vec<String>, SideQueryError> {
        Ok(self.response.iter().take(max_results).cloned().collect())
    }
}

/// Always returns an error.
struct FailingRanker;

impl SideQueryRanker for FailingRanker {
    fn rank_memories(
        &self,
        _query: &str,
        _manifest_text: &str,
        _max_results: usize,
    ) -> Result<Vec<String>, SideQueryError> {
        RankerFailedSnafu {
            message: "mock failure",
        }
        .fail()
    }
}

/// Captures the manifest text that was sent to the ranker.
struct CapturingRanker {
    captured: std::sync::Mutex<Vec<String>>,
    response: Vec<String>,
}

impl CapturingRanker {
    fn new(response: Vec<&str>) -> Self {
        Self {
            captured: std::sync::Mutex::new(Vec::new()),
            response: response.into_iter().map(String::from).collect(),
        }
    }

    fn captured_manifests(&self) -> Vec<String> {
        self.captured
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl SideQueryRanker for CapturingRanker {
    fn rank_memories(
        &self,
        _query: &str,
        manifest_text: &str,
        max_results: usize,
    ) -> Result<Vec<String>, SideQueryError> {
        self.captured
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(manifest_text.to_owned());
        Ok(self.response.iter().take(max_results).cloned().collect())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_header(id: &str, name: &str, mtime: i64) -> MemoryHeader {
    MemoryHeader::new(id, name, mtime)
}

fn make_manifest(entries: &[(&str, &str, i64)]) -> MemoryManifest {
    let headers: Vec<MemoryHeader> = entries
        .iter()
        .map(|(id, name, mtime)| make_header(id, name, *mtime))
        .collect();
    MemoryManifest::from_headers(headers)
}

fn make_scored_result(source_id: &str, score: f64) -> ScoredResult {
    ScoredResult {
        content: format!("content for {source_id}"),
        source_type: "fact".to_owned(),
        source_id: source_id.to_owned(),
        nous_id: String::new(),
        factors: FactorScores::default(),
        score,
    }
}

fn default_selector() -> SideQuerySelector {
    SideQuerySelector::new(SideQueryConfig::default())
}

fn selector_with_capacity(cap: usize) -> SideQuerySelector {
    SideQuerySelector::new(SideQueryConfig {
        cache_capacity: cap,
        ..SideQueryConfig::default()
    })
}

// ── Manifest tests ────────────────────────────────────────────────────────

#[test]
fn manifest_cap_is_200() {
    assert_eq!(MAX_MEMORY_ENTRIES, 200, "cap constant should be 200");
}

// ── Selector: basic operation ─────────────────────────────────────────────

#[test]
fn select_returns_empty_when_disabled() {
    let selector = SideQuerySelector::new(SideQueryConfig {
        enabled: false,
        ..SideQueryConfig::default()
    });
    let manifest = make_manifest(&[("a", "alpha", 1)]);
    let ranker = MockRanker::new(vec!["a"]);

    let result = selector.select("query", &manifest, &ranker);
    assert!(result.is_ok(), "disabled selector should return Ok");
    let result = result.unwrap_or_else(|_| unreachable!());
    assert!(
        result.selected_ids.is_empty(),
        "disabled selector should return empty ids"
    );
    assert!(
        !result.from_cache,
        "disabled result should not be FROM cache"
    );
}

#[test]
fn select_returns_empty_when_manifest_empty() {
    let selector = default_selector();
    let manifest = MemoryManifest::from_headers(vec![]);
    let ranker = MockRanker::new(vec!["a"]);

    let result = selector.select("query", &manifest, &ranker);
    assert!(result.is_ok(), "empty manifest should return Ok");
    assert!(
        result
            .unwrap_or_else(|_| unreachable!())
            .selected_ids
            .is_empty(),
        "empty manifest should yield empty selection"
    );
}

#[test]
fn select_calls_ranker_and_returns_results() {
    let selector = default_selector();
    let manifest = make_manifest(&[("a", "alpha", 3), ("b", "beta", 2), ("c", "gamma", 1)]);
    let ranker = MockRanker::new(vec!["a", "c"]);

    let result = selector
        .select("what is alpha?", &manifest, &ranker)
        .unwrap_or_else(|_| unreachable!());
    assert_eq!(
        result.selected_ids,
        vec!["a", "c"],
        "selector should return IDs ranked by the ranker"
    );
    assert!(!result.from_cache, "first call should not be cached");
}

#[test]
fn select_respects_max_results() {
    let selector = SideQuerySelector::new(SideQueryConfig {
        max_results: 2,
        ..SideQueryConfig::default()
    });
    let manifest = make_manifest(&[("a", "a", 3), ("b", "b", 2), ("c", "c", 1)]);
    let ranker = MockRanker::new(vec!["a", "b", "c"]);

    let result = selector
        .select("query", &manifest, &ranker)
        .unwrap_or_else(|_| unreachable!());
    assert_eq!(result.selected_ids.len(), 2, "should respect max_results=2");
}

#[test]
fn select_returns_error_on_ranker_failure() {
    let selector = default_selector();
    let manifest = make_manifest(&[("a", "alpha", 1)]);
    let ranker = FailingRanker;

    let result = selector.select("query", &manifest, &ranker);
    assert!(result.is_err(), "ranker failure should propagate");
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("mock failure"),
        "error should contain ranker's message"
    );
}

// ── Selector: already_surfaced tracking ───────────────────────────────────

#[test]
fn mark_surfaced_prevents_reselection() {
    let selector = default_selector();
    let manifest = make_manifest(&[("a", "alpha", 3), ("b", "beta", 2), ("c", "gamma", 1)]);

    // NOTE: mark "a" as surfaced before selection.
    selector.mark_surfaced(&["a".to_owned()]);

    let ranker = CapturingRanker::new(vec!["b"]);
    let result = selector
        .select("query", &manifest, &ranker)
        .unwrap_or_else(|_| unreachable!());

    // WHY: "a" was surfaced, so the manifest sent to the ranker should not contain it.
    let manifests = ranker.captured_manifests();
    assert_eq!(manifests.len(), 1, "ranker should be called once");
    assert!(
        !manifests.first().unwrap_or(&String::new()).contains(" a "),
        "surfaced entry 'a' should not appear in manifest sent to ranker"
    );

    assert_eq!(
        result.selected_ids,
        vec!["b"],
        "only non-surfaced entry 'b' should be selected"
    );
}

#[test]
fn is_surfaced_reflects_state() {
    let selector = default_selector();
    assert!(
        !selector.is_surfaced("x"),
        "fresh selector should have nothing surfaced"
    );
    selector.mark_surfaced(&["x".to_owned()]);
    assert!(selector.is_surfaced("x"), "marked ID should be surfaced");
    assert!(
        !selector.is_surfaced("y"),
        "unmarked ID should not be surfaced"
    );
}

#[test]
fn all_surfaced_returns_empty_without_calling_ranker() {
    let selector = default_selector();
    let manifest = make_manifest(&[("a", "alpha", 1), ("b", "beta", 2)]);

    selector.mark_surfaced(&["a".to_owned(), "b".to_owned()]);

    let ranker = CapturingRanker::new(vec!["a"]);
    let result = selector
        .select("query", &manifest, &ranker)
        .unwrap_or_else(|_| unreachable!());

    assert!(
        result.selected_ids.is_empty(),
        "all-surfaced should return empty"
    );
    assert!(
        ranker.captured_manifests().is_empty(),
        "ranker should not be called when all entries surfaced"
    );
}

// ── Selector: caching ─────────────────────────────────────────────────────

#[test]
fn second_identical_call_uses_cache() {
    let selector = default_selector();
    let manifest = make_manifest(&[("a", "alpha", 1)]);
    let ranker = MockRanker::new(vec!["a"]);

    // NOTE: first call — cache miss.
    let r1 = selector
        .select("query", &manifest, &ranker)
        .unwrap_or_else(|_| unreachable!());
    assert!(!r1.from_cache, "first call should not be cached");

    // NOTE: second call — cache hit.
    let r2 = selector
        .select("query", &manifest, &ranker)
        .unwrap_or_else(|_| unreachable!());
    assert!(r2.from_cache, "second identical call should use cache");
    assert_eq!(
        r2.selected_ids, r1.selected_ids,
        "cached result should match"
    );
}

#[test]
fn different_query_causes_cache_miss() {
    let selector = default_selector();
    let manifest = make_manifest(&[("a", "alpha", 1)]);
    let ranker = MockRanker::new(vec!["a"]);

    let _ = selector.select("query-1", &manifest, &ranker);
    let r2 = selector
        .select("query-2", &manifest, &ranker)
        .unwrap_or_else(|_| unreachable!());
    assert!(!r2.from_cache, "different query should cause cache miss");
}

#[test]
fn cache_evicts_lru_at_capacity() {
    let selector = selector_with_capacity(2);
    let m1 = make_manifest(&[("a", "alpha", 1)]);
    let m2 = make_manifest(&[("b", "beta", 2)]);
    let m3 = make_manifest(&[("c", "gamma", 3)]);
    let ranker = MockRanker::new(vec!["a", "b", "c"]);

    // NOTE: fill the cache with 2 entries.
    let _ = selector.select("q1", &m1, &ranker);
    let _ = selector.select("q2", &m2, &ranker);
    assert_eq!(selector.cache_len(), 2, "cache should have 2 entries");

    // NOTE: third entry evicts the first.
    let _ = selector.select("q3", &m3, &ranker);
    assert_eq!(
        selector.cache_len(),
        2,
        "cache should still have 2 entries after eviction"
    );

    // NOTE: q1 should be evicted (LRU), q2 and q3 remain.
    let r1_retry = selector
        .select("q1", &m1, &ranker)
        .unwrap_or_else(|_| unreachable!());
    assert!(
        !r1_retry.from_cache,
        "evicted entry q1 should cause cache miss"
    );
}

#[test]
fn cache_len_reflects_insertions() {
    let selector = default_selector();
    assert_eq!(selector.cache_len(), 0, "fresh selector has empty cache");

    let manifest = make_manifest(&[("a", "alpha", 1)]);
    let ranker = MockRanker::new(vec!["a"]);
    let _ = selector.select("q", &manifest, &ranker);
    assert_eq!(
        selector.cache_len(),
        1,
        "cache should have 1 entry after first SELECT"
    );
}

// ── Pre-filter integration ────────────────────────────────────────────────

#[test]
fn pre_filter_retains_selected_candidates() {
    let candidates = vec![
        make_scored_result("a", 0.9),
        make_scored_result("b", 0.8),
        make_scored_result("c", 0.7),
    ];
    let selected: HashSet<String> = ["a", "c"].iter().map(|s| String::from(*s)).collect();

    let filtered = pre_filter_by_side_query(candidates, &selected);
    let ids: Vec<&str> = filtered.iter().map(|r| r.source_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["a", "c"],
        "only selected candidates should remain"
    );
}

#[test]
fn pre_filter_removes_unselected_candidates() {
    let candidates = vec![make_scored_result("a", 0.9), make_scored_result("b", 0.8)];
    let selected: HashSet<String> = ["a"].iter().map(|s| String::from(*s)).collect();

    let filtered = pre_filter_by_side_query(candidates, &selected);
    assert_eq!(filtered.len(), 1, "only 'a' should remain");
    assert_eq!(
        filtered.first().map(|r| r.source_id.as_str()),
        Some("a"),
        "pre-filter should keep only selected source 'a'"
    );
}

#[test]
fn pre_filter_with_empty_selection_passes_all() {
    let candidates = vec![make_scored_result("a", 0.9), make_scored_result("b", 0.8)];
    let selected: HashSet<String> = HashSet::new();

    let filtered = pre_filter_by_side_query(candidates, &selected);
    assert_eq!(
        filtered.len(),
        2,
        "empty selection should pass all candidates through"
    );
}

#[test]
fn pre_filter_preserves_order() {
    let candidates = vec![
        make_scored_result("c", 0.7),
        make_scored_result("a", 0.9),
        make_scored_result("b", 0.8),
    ];
    let selected: HashSet<String> = ["a", "b", "c"].iter().map(|s| String::from(*s)).collect();

    let filtered = pre_filter_by_side_query(candidates, &selected);
    let ids: Vec<&str> = filtered.iter().map(|r| r.source_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["c", "a", "b"],
        "pre-filter should preserve original ORDER"
    );
}

#[test]
fn pre_filter_with_no_matching_ids_returns_empty() {
    let candidates = vec![make_scored_result("a", 0.9), make_scored_result("b", 0.8)];
    let selected: HashSet<String> = ["x", "y"].iter().map(|s| String::from(*s)).collect();

    let filtered = pre_filter_by_side_query(candidates, &selected);
    assert!(
        filtered.is_empty(),
        "no matches should produce empty result"
    );
}

#[test]
fn pre_filter_with_empty_candidates_returns_empty() {
    let candidates: Vec<ScoredResult> = vec![];
    let selected: HashSet<String> = ["a"].iter().map(|s| String::from(*s)).collect();

    let filtered = pre_filter_by_side_query(candidates, &selected);
    assert!(
        filtered.is_empty(),
        "empty candidates should produce empty result"
    );
}

// ── SideQueryResult display ───────────────────────────────────────────────

#[test]
fn side_query_result_display() {
    let result = crate::side_query::SideQueryResult {
        selected_ids: vec!["a".to_owned(), "b".to_owned()],
        from_cache: true,
    };
    let display = result.to_string();
    assert!(display.contains("2 selected"), "display should show count");
    assert!(
        display.contains("cached=true"),
        "display should show cache status"
    );
}

// ── SideQuerySelector debug ───────────────────────────────────────────────

#[test]
fn selector_debug_shows_config() {
    let selector = default_selector();
    let debug = format!("{selector:?}");
    assert!(
        debug.contains("SideQuerySelector"),
        "debug should identify the struct"
    );
    assert!(
        debug.contains("surfaced_count"),
        "debug should show surfaced count"
    );
}
