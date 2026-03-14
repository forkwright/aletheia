//! Update handlers for the memory inspector panel.

use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::memory::{FactDetail, MemoryFact, MemorySearchResult, MemoryTab};
use crate::state::view_stack::View;

/// Open the memory inspector, pushing it onto the view stack and loading data.
pub async fn handle_open(app: &mut App) {
    app.view_stack.push(View::MemoryInspector);
    app.memory.loading = true;
    // Load facts from the API
    load_facts(app).await;
}

/// Close memory inspector (pop back).
pub fn handle_close(app: &mut App) {
    if matches!(
        app.view_stack.current(),
        View::MemoryInspector | View::FactDetail { .. }
    ) {
        app.view_stack.pop();
    }
}

pub fn handle_tab_next(app: &mut App) {
    app.memory.tab = app.memory.tab.next();
}

pub fn handle_tab_prev(app: &mut App) {
    app.memory.tab = app.memory.tab.prev();
}

pub fn handle_select_up(app: &mut App) {
    if app.memory.selected > 0 {
        app.memory.selected -= 1;
        adjust_scroll(app);
    }
}

pub fn handle_select_down(app: &mut App) {
    let max = item_count(app).saturating_sub(1);
    if app.memory.selected < max {
        app.memory.selected += 1;
        adjust_scroll(app);
    }
}

pub fn handle_select_first(app: &mut App) {
    app.memory.selected = 0;
    app.memory.scroll_offset = 0;
}

pub fn handle_select_last(app: &mut App) {
    let max = item_count(app).saturating_sub(1);
    app.memory.selected = max;
    adjust_scroll(app);
}

pub fn handle_page_up(app: &mut App) {
    let page = visible_rows(app);
    app.memory.selected = app.memory.selected.saturating_sub(page);
    adjust_scroll(app);
}

pub fn handle_page_down(app: &mut App) {
    let max = item_count(app).saturating_sub(1);
    let page = visible_rows(app);
    app.memory.selected = (app.memory.selected + page).min(max);
    adjust_scroll(app);
}

pub fn handle_sort_cycle(app: &mut App) {
    app.memory.sort = app.memory.sort.next();
}

pub fn handle_filter_open(app: &mut App) {
    app.memory.filter_editing = true;
}

pub fn handle_filter_close(app: &mut App) {
    app.memory.filter_editing = false;
    app.memory.filter_text.clear();
}

pub fn handle_filter_input(app: &mut App, c: char) {
    app.memory.filter_text.push(c);
}

pub fn handle_filter_backspace(app: &mut App) {
    if app.memory.filter_text.is_empty() {
        app.memory.filter_editing = false;
    } else {
        app.memory.filter_text.pop();
    }
}

pub async fn handle_drill_in(app: &mut App) {
    if app.memory.tab == MemoryTab::Facts
        && let Some(fact) = app.memory.selected_fact()
    {
        let fact_id = fact.id.clone();
        app.view_stack.push(View::FactDetail {
            fact_id: fact_id.clone(),
        });
        load_fact_detail(app, &fact_id).await;
    }
}

pub fn handle_pop_back(app: &mut App) {
    if matches!(app.view_stack.current(), View::FactDetail { .. }) {
        app.view_stack.pop();
        app.memory.detail = None;
    } else {
        app.view_stack.pop();
    }
}

pub async fn handle_forget(app: &mut App) {
    if let Some(fact) = app.memory.selected_fact() {
        let id = fact.id.clone();
        let client = app.client.clone();
        match client.knowledge_forget(&id).await {
            Ok(()) => {
                if let Some(f) = app.memory.facts.iter_mut().find(|f| f.id == id) {
                    f.is_forgotten = true;
                }
                app.error_toast = Some(ErrorToast::new("Fact forgotten".into()));
            }
            Err(e) => {
                app.error_toast = Some(ErrorToast::new(format!("Forget failed: {e}")));
            }
        }
    }
}

pub async fn handle_restore(app: &mut App) {
    if let Some(fact) = app.memory.selected_fact() {
        let id = fact.id.clone();
        let client = app.client.clone();
        match client.knowledge_restore(&id).await {
            Ok(()) => {
                if let Some(f) = app.memory.facts.iter_mut().find(|f| f.id == id) {
                    f.is_forgotten = false;
                }
                app.error_toast = Some(ErrorToast::new("Fact restored".into()));
            }
            Err(e) => {
                app.error_toast = Some(ErrorToast::new(format!("Restore failed: {e}")));
            }
        }
    }
}

pub fn handle_edit_confidence_start(app: &mut App) {
    let conf = app.memory.selected_fact().map(|f| f.confidence);
    if let Some(c) = conf {
        app.memory.editing_confidence = true;
        app.memory.confidence_buffer = format!("{c:.2}");
    }
}

pub fn handle_confidence_input(app: &mut App, c: char) {
    if c.is_ascii_digit() || c == '.' {
        app.memory.confidence_buffer.push(c);
    }
}

pub fn handle_confidence_backspace(app: &mut App) {
    app.memory.confidence_buffer.pop();
}

pub async fn handle_confidence_submit(app: &mut App) {
    let conf: f64 = match app.memory.confidence_buffer.parse() {
        Ok(v) if (0.0..=1.0).contains(&v) => v,
        _ => {
            app.error_toast = Some(ErrorToast::new("Confidence must be 0.0–1.0".into()));
            return;
        }
    };
    app.memory.editing_confidence = false;

    let selected = app
        .memory
        .selected_fact()
        .map(|f| (f.id.clone(), f.confidence));
    if let Some((id, prev_conf)) = selected {
        // Optimistic update: apply locally before the API round-trip.
        if let Some(f) = app.memory.facts.iter_mut().find(|f| f.id == id) {
            f.confidence = conf;
        }
        let client = app.client.clone();
        match client.knowledge_update_confidence(&id, conf).await {
            Ok(()) => {
                app.error_toast = Some(ErrorToast::new(format!("Confidence set to {conf:.2}")));
            }
            Err(e) => {
                // Revert the optimistic update on failure.
                if let Some(f) = app.memory.facts.iter_mut().find(|f| f.id == id) {
                    f.confidence = prev_conf;
                }
                app.error_toast = Some(ErrorToast::new(format!("Confidence update failed: {e}")));
            }
        }
    }
}

pub fn handle_confidence_cancel(app: &mut App) {
    app.memory.editing_confidence = false;
    app.memory.confidence_buffer.clear();
}

pub fn handle_search_open(app: &mut App) {
    app.memory.search_active = true;
    app.memory.search_query.clear();
}

pub fn handle_search_input(app: &mut App, c: char) {
    app.memory.search_query.push(c);
}

pub fn handle_search_backspace(app: &mut App) {
    if app.memory.search_query.is_empty() {
        app.memory.search_active = false;
    } else {
        app.memory.search_query.pop();
    }
}

pub async fn handle_search_submit(app: &mut App) {
    if app.memory.search_query.is_empty() {
        return;
    }
    app.memory.search_active = false;
    // Perform search via local fuzzy filtering for now
    let query = app.memory.search_query.to_lowercase();
    let terms: Vec<&str> = query.split_whitespace().collect();

    let results: Vec<MemorySearchResult> = app
        .memory
        .facts
        .iter()
        .filter(|f| !f.is_forgotten)
        .filter_map(|f| {
            let content_lower = f.content.to_lowercase();
            let mut score = 0.0_f64;
            for term in &terms {
                if content_lower.contains(term) {
                    score += 1.0;
                }
            }
            if score > 0.0 {
                score *= f.confidence;
                Some(MemorySearchResult {
                    id: f.id.clone(),
                    content: f.content.clone(),
                    confidence: f.confidence,
                    tier: f.tier.clone(),
                    fact_type: f.fact_type.clone(),
                    score,
                })
            } else {
                None
            }
        })
        .collect();

    app.memory.search_results = results;
    if !app.memory.search_results.is_empty() {
        let msg = format!(
            "{} results for '{}'",
            app.memory.search_results.len(),
            app.memory.search_query
        );
        app.error_toast = Some(ErrorToast::new(msg));
    } else {
        app.error_toast = Some(ErrorToast::new(format!(
            "No results for '{}'",
            app.memory.search_query
        )));
    }
}

pub fn handle_search_close(app: &mut App) {
    app.memory.search_active = false;
    app.memory.search_query.clear();
    app.memory.search_results.clear();
}

// --- Data loading ---

pub fn handle_facts_loaded(app: &mut App, facts: Vec<MemoryFact>, total: usize) {
    app.memory.facts = facts;
    app.memory.total_facts = total;
    app.memory.loading = false;
    app.memory.selected = 0;
    app.memory.scroll_offset = 0;
}

pub fn handle_detail_loaded(app: &mut App, detail: FactDetail) {
    app.memory.detail = Some(detail);
}

pub fn handle_action_result(app: &mut App, message: String) {
    app.error_toast = Some(ErrorToast::new(message));
}

// --- Internal helpers ---

async fn load_facts(app: &mut App) {
    let client = app.client.clone();
    let sort = app.memory.sort.as_str();
    let order = if app.memory.sort_asc { "asc" } else { "desc" };

    match client.knowledge_facts(sort, order, 500).await {
        Ok(json) => {
            if let Ok(resp) = serde_json::from_value::<FactsListResponse>(json) {
                app.memory.facts = resp.facts;
                app.memory.total_facts = resp.total;
            }
            app.memory.loading = false;
        }
        Err(e) => {
            tracing::debug!("failed to load facts: {e}");
            app.memory.loading = false;
        }
    }
}

async fn load_fact_detail(app: &mut App, fact_id: &str) {
    let client = app.client.clone();

    match client.knowledge_fact_detail(fact_id).await {
        Ok(json) => {
            if let Ok(detail) = serde_json::from_value::<FactDetail>(json) {
                app.memory.detail = Some(detail);
                return;
            }
        }
        Err(e) => {
            tracing::debug!("failed to load fact detail: {e}");
        }
    }
    // Fallback: create minimal detail from local data
    if let Some(fact) = app.memory.facts.iter().find(|f| f.id == fact_id) {
        app.memory.detail = Some(FactDetail {
            fact: fact.clone(),
            relationships: Vec::new(),
            similar: Vec::new(),
        });
    }
}

fn item_count(app: &App) -> usize {
    match app.memory.tab {
        MemoryTab::Facts => app.memory.facts.len(),
        MemoryTab::Graph => app.memory.entities.len(),
        MemoryTab::Timeline => app.memory.timeline_events.len(),
    }
}

fn visible_rows(app: &App) -> usize {
    app.terminal_height.saturating_sub(8) as usize
}

fn adjust_scroll(app: &mut App) {
    let visible = visible_rows(app);
    if app.memory.selected < app.memory.scroll_offset {
        app.memory.scroll_offset = app.memory.selected;
    } else if app.memory.selected >= app.memory.scroll_offset + visible {
        app.memory.scroll_offset = app.memory.selected.saturating_sub(visible) + 1;
    }
}

#[derive(serde::Deserialize)]
struct FactsListResponse {
    facts: Vec<MemoryFact>,
    total: usize,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;
    use crate::state::memory::MemoryFact;

    fn sample_fact(id: &str, content: &str, confidence: f64) -> MemoryFact {
        MemoryFact {
            id: id.into(),
            nous_id: "syn".into(),
            content: content.into(),
            confidence,
            tier: "verified".into(),
            valid_from: "2026-01-01".into(),
            valid_to: "9999-12-31".into(),
            superseded_by: None,
            source_session_id: Some("ses-1".into()),
            recorded_at: "2026-01-01T00:00:00Z".into(),
            access_count: 5,
            last_accessed_at: "2026-03-09T12:00:00Z".into(),
            stability_hours: 720.0,
            fact_type: "knowledge".into(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    #[test]
    fn handle_select_down_increments() {
        let mut app = test_app();
        app.memory.facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
        ];
        app.memory.selected = 0;
        handle_select_down(&mut app);
        assert_eq!(app.memory.selected, 1);
    }

    #[test]
    fn handle_select_down_clamps_at_end() {
        let mut app = test_app();
        app.memory.facts = vec![sample_fact("f1", "first", 0.9)];
        app.memory.selected = 0;
        handle_select_down(&mut app);
        assert_eq!(app.memory.selected, 0);
    }

    #[test]
    fn handle_select_up_decrements() {
        let mut app = test_app();
        app.memory.facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
        ];
        app.memory.selected = 1;
        handle_select_up(&mut app);
        assert_eq!(app.memory.selected, 0);
    }

    #[test]
    fn handle_select_up_clamps_at_zero() {
        let mut app = test_app();
        app.memory.facts = vec![sample_fact("f1", "first", 0.9)];
        app.memory.selected = 0;
        handle_select_up(&mut app);
        assert_eq!(app.memory.selected, 0);
    }

    #[test]
    fn handle_select_first_goes_to_zero() {
        let mut app = test_app();
        app.memory.facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
        ];
        app.memory.selected = 1;
        handle_select_first(&mut app);
        assert_eq!(app.memory.selected, 0);
    }

    #[test]
    fn handle_select_last_goes_to_end() {
        let mut app = test_app();
        app.memory.facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
            sample_fact("f3", "third", 0.7),
        ];
        app.memory.selected = 0;
        handle_select_last(&mut app);
        assert_eq!(app.memory.selected, 2);
    }

    #[test]
    fn handle_sort_cycle_advances() {
        let mut app = test_app();
        assert_eq!(app.memory.sort, crate::state::memory::FactSort::Confidence);
        handle_sort_cycle(&mut app);
        assert_eq!(app.memory.sort, crate::state::memory::FactSort::Recency);
    }

    #[test]
    fn handle_tab_next_cycles() {
        let mut app = test_app();
        assert_eq!(app.memory.tab, crate::state::memory::MemoryTab::Facts);
        handle_tab_next(&mut app);
        assert_eq!(app.memory.tab, crate::state::memory::MemoryTab::Graph);
        handle_tab_next(&mut app);
        assert_eq!(app.memory.tab, crate::state::memory::MemoryTab::Timeline);
        handle_tab_next(&mut app);
        assert_eq!(app.memory.tab, crate::state::memory::MemoryTab::Facts);
    }

    #[test]
    fn handle_tab_prev_cycles() {
        let mut app = test_app();
        handle_tab_prev(&mut app);
        assert_eq!(app.memory.tab, crate::state::memory::MemoryTab::Timeline);
    }

    #[test]
    fn handle_filter_lifecycle() {
        let mut app = test_app();
        assert!(!app.memory.filter_editing);

        handle_filter_open(&mut app);
        assert!(app.memory.filter_editing);

        handle_filter_input(&mut app, 'r');
        handle_filter_input(&mut app, 'u');
        handle_filter_input(&mut app, 's');
        handle_filter_input(&mut app, 't');
        assert_eq!(app.memory.filter_text, "rust");

        handle_filter_backspace(&mut app);
        assert_eq!(app.memory.filter_text, "rus");

        handle_filter_close(&mut app);
        assert!(!app.memory.filter_editing);
        assert!(app.memory.filter_text.is_empty());
    }

    #[test]
    fn handle_filter_backspace_on_empty_closes() {
        let mut app = test_app();
        app.memory.filter_editing = true;
        handle_filter_backspace(&mut app);
        assert!(!app.memory.filter_editing);
    }

    #[test]
    fn handle_facts_loaded_sets_data() {
        let mut app = test_app();
        let facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
        ];
        handle_facts_loaded(&mut app, facts, 2);
        assert_eq!(app.memory.facts.len(), 2);
        assert_eq!(app.memory.total_facts, 2);
        assert!(!app.memory.loading);
    }

    #[test]
    fn handle_close_pops_memory_view() {
        let mut app = test_app();
        app.view_stack.push(View::MemoryInspector);
        handle_close(&mut app);
        assert!(app.view_stack.is_home());
    }

    #[test]
    fn handle_pop_back_from_detail_goes_to_inspector() {
        let mut app = test_app();
        app.view_stack.push(View::MemoryInspector);
        app.view_stack.push(View::FactDetail {
            fact_id: "f1".into(),
        });
        handle_pop_back(&mut app);
        assert_eq!(app.view_stack.current(), &View::MemoryInspector);
        assert!(app.memory.detail.is_none());
    }

    #[test]
    fn handle_confidence_input_accepts_digits_and_dot() {
        let mut app = test_app();
        app.memory.editing_confidence = true;
        app.memory.confidence_buffer.clear();
        handle_confidence_input(&mut app, '0');
        handle_confidence_input(&mut app, '.');
        handle_confidence_input(&mut app, '8');
        assert_eq!(app.memory.confidence_buffer, "0.8");
    }

    #[test]
    fn handle_confidence_input_rejects_letters() {
        let mut app = test_app();
        app.memory.editing_confidence = true;
        app.memory.confidence_buffer.clear();
        handle_confidence_input(&mut app, 'a');
        assert!(app.memory.confidence_buffer.is_empty());
    }

    #[test]
    fn handle_confidence_cancel_clears() {
        let mut app = test_app();
        app.memory.editing_confidence = true;
        app.memory.confidence_buffer = "0.5".into();
        handle_confidence_cancel(&mut app);
        assert!(!app.memory.editing_confidence);
        assert!(app.memory.confidence_buffer.is_empty());
    }

    #[test]
    fn handle_search_lifecycle() {
        let mut app = test_app();
        handle_search_open(&mut app);
        assert!(app.memory.search_active);

        handle_search_input(&mut app, 'r');
        handle_search_input(&mut app, 'u');
        assert_eq!(app.memory.search_query, "ru");

        handle_search_backspace(&mut app);
        assert_eq!(app.memory.search_query, "r");

        handle_search_close(&mut app);
        assert!(!app.memory.search_active);
        assert!(app.memory.search_query.is_empty());
    }

    #[test]
    fn handle_search_backspace_on_empty_closes() {
        let mut app = test_app();
        app.memory.search_active = true;
        handle_search_backspace(&mut app);
        assert!(!app.memory.search_active);
    }

    #[test]
    fn handle_action_result_sets_toast() {
        let mut app = test_app();
        handle_action_result(&mut app, "done".into());
        assert!(app.error_toast.is_some());
        assert_eq!(app.error_toast.as_ref().unwrap().message, "done");
    }

    #[test]
    fn adjust_scroll_scrolls_down() {
        let mut app = test_app();
        app.terminal_height = 20; // visible_rows = 12
        app.memory.facts = (0..20)
            .map(|i| sample_fact(&format!("f{i}"), &format!("fact {i}"), 0.9))
            .collect();
        app.memory.selected = 15;
        adjust_scroll(&mut app);
        assert!(app.memory.scroll_offset > 0);
    }

    #[test]
    fn item_count_for_each_tab() {
        let mut app = test_app();
        app.memory.facts = vec![sample_fact("f1", "a", 0.9)];
        app.memory.tab = crate::state::memory::MemoryTab::Facts;
        assert_eq!(item_count(&app), 1);

        app.memory.tab = crate::state::memory::MemoryTab::Graph;
        assert_eq!(item_count(&app), 0);

        app.memory.tab = crate::state::memory::MemoryTab::Timeline;
        assert_eq!(item_count(&app), 0);
    }
}
