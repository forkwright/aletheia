//! Public handler functions for memory inspector actions.

use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::memory::{MemorySearchResult, MemoryTab};
use crate::state::view_stack::View;

use super::data_loading;

pub(crate) async fn handle_open(app: &mut App) {
    app.layout.view_stack.push(View::MemoryInspector);
    app.layout.memory.loading = true;
    data_loading::load_facts(app).await;
    data_loading::load_graph_data(app).await;
}

pub(crate) fn handle_close(app: &mut App) {
    if matches!(
        app.layout.view_stack.current(),
        View::MemoryInspector | View::FactDetail { .. } | View::EntityDetail { .. }
    ) {
        app.layout.view_stack.pop();
    }
}

pub(crate) fn handle_tab_next(app: &mut App) {
    app.layout.memory.tab = app.layout.memory.tab.next();
}

pub(crate) fn handle_tab_prev(app: &mut App) {
    app.layout.memory.tab = app.layout.memory.tab.prev();
}

pub(crate) fn handle_select_up(app: &mut App) {
    if app.layout.memory.fact_list.selected > 0 {
        app.layout.memory.fact_list.selected -= 1;
        super::data_loading::adjust_scroll(app);
    }
}

pub(crate) fn handle_select_down(app: &mut App) {
    let max = super::data_loading::item_count(app).saturating_sub(1);
    if app.layout.memory.fact_list.selected < max {
        app.layout.memory.fact_list.selected += 1;
        super::data_loading::adjust_scroll(app);
    }
}

pub(crate) fn handle_select_first(app: &mut App) {
    app.layout.memory.fact_list.selected = 0;
    app.layout.memory.fact_list.scroll_offset = 0;
}

pub(crate) fn handle_select_last(app: &mut App) {
    let max = super::data_loading::item_count(app).saturating_sub(1);
    app.layout.memory.fact_list.selected = max;
    super::data_loading::adjust_scroll(app);
}

pub(crate) fn handle_page_up(app: &mut App) {
    let page = super::data_loading::visible_rows(app);
    app.layout.memory.fact_list.selected =
        app.layout.memory.fact_list.selected.saturating_sub(page);
    super::data_loading::adjust_scroll(app);
}

pub(crate) fn handle_page_down(app: &mut App) {
    let max = super::data_loading::item_count(app).saturating_sub(1);
    let page = super::data_loading::visible_rows(app);
    app.layout.memory.fact_list.selected = (app.layout.memory.fact_list.selected + page).min(max);
    super::data_loading::adjust_scroll(app);
}

pub(crate) fn handle_sort_cycle(app: &mut App) {
    app.layout.memory.fact_list.sort = app.layout.memory.fact_list.sort.next();
}

pub(crate) fn handle_filter_open(app: &mut App) {
    app.layout.memory.filters.filter_editing = true;
}

pub(crate) fn handle_filter_close(app: &mut App) {
    app.layout.memory.filters.filter_editing = false;
    app.layout.memory.filters.filter_text.clear();
}

pub(crate) fn handle_filter_input(app: &mut App, c: char) {
    app.layout.memory.filters.filter_text.push(c);
}

pub(crate) fn handle_filter_backspace(app: &mut App) {
    if app.layout.memory.filters.filter_text.is_empty() {
        app.layout.memory.filters.filter_editing = false;
    } else {
        app.layout.memory.filters.filter_text.pop();
    }
}

pub(crate) async fn handle_drill_in(app: &mut App) {
    if app.layout.memory.tab == MemoryTab::Facts
        && let Some(fact) = app.layout.memory.selected_fact()
    {
        let fact_id = fact.id.clone();
        app.layout.view_stack.push(View::FactDetail {
            fact_id: fact_id.clone(),
        });
        data_loading::load_fact_detail(app, &fact_id).await;
    } else if app.layout.memory.tab == MemoryTab::Graph {
        let selected = app.layout.memory.graph.selected_entity;
        if let Some(stat) = app.layout.memory.graph.entity_stats.get(selected) {
            let entity_id = stat.entity.id.clone();
            data_loading::build_node_card(app, &entity_id);
            app.layout.view_stack.push(View::EntityDetail {
                entity_id: entity_id.clone(),
            });
        }
    }
}

pub(crate) fn handle_pop_back(app: &mut App) {
    if matches!(app.layout.view_stack.current(), View::FactDetail { .. }) {
        app.layout.view_stack.pop();
        app.layout.memory.fact_list.detail = None;
    } else if matches!(app.layout.view_stack.current(), View::EntityDetail { .. }) {
        app.layout.view_stack.pop();
        app.layout.memory.graph.node_card = None;
    } else {
        app.layout.view_stack.pop();
    }
}

pub(crate) fn handle_drift_tab_next(app: &mut App) {
    app.layout.memory.graph.drift_tab = app.layout.memory.graph.drift_tab.next();
    app.layout.memory.graph.drift_selected = 0;
    app.layout.memory.graph.drift_scroll_offset = 0;
}

pub(crate) fn handle_drift_tab_prev(app: &mut App) {
    app.layout.memory.graph.drift_tab = app.layout.memory.graph.drift_tab.prev();
    app.layout.memory.graph.drift_selected = 0;
    app.layout.memory.graph.drift_scroll_offset = 0;
}

pub(crate) async fn handle_forget(app: &mut App) {
    if let Some(fact) = app.layout.memory.selected_fact() {
        let id = fact.id.clone();
        let client = app.client.clone();
        match client.knowledge_forget(&id).await {
            Ok(()) => {
                if let Some(f) = app
                    .layout
                    .memory
                    .fact_list
                    .facts
                    .iter_mut()
                    .find(|f| f.id == id)
                {
                    f.lifecycle.is_forgotten = true;
                }
                app.viewport.error_toast = Some(ErrorToast::new("Fact forgotten".into()));
            }
            Err(e) => {
                app.viewport.error_toast = Some(ErrorToast::new(format!("Forget failed: {e}")));
            }
        }
    }
}

pub(crate) async fn handle_restore(app: &mut App) {
    if let Some(fact) = app.layout.memory.selected_fact() {
        let id = fact.id.clone();
        let client = app.client.clone();
        match client.knowledge_restore(&id).await {
            Ok(()) => {
                if let Some(f) = app
                    .layout
                    .memory
                    .fact_list
                    .facts
                    .iter_mut()
                    .find(|f| f.id == id)
                {
                    f.lifecycle.is_forgotten = false;
                }
                app.viewport.error_toast = Some(ErrorToast::new("Fact restored".into()));
            }
            Err(e) => {
                app.viewport.error_toast = Some(ErrorToast::new(format!("Restore failed: {e}")));
            }
        }
    }
}

pub(crate) fn handle_edit_confidence_start(app: &mut App) {
    let conf = app.layout.memory.selected_fact().map(|f| f.confidence);
    if let Some(c) = conf {
        app.layout.memory.fact_list.editing_confidence = true;
        app.layout.memory.fact_list.confidence_buffer = format!("{c:.2}");
    }
}

pub(crate) fn handle_confidence_input(app: &mut App, c: char) {
    if c.is_ascii_digit() || c == '.' {
        app.layout.memory.fact_list.confidence_buffer.push(c);
    }
}

pub(crate) fn handle_confidence_backspace(app: &mut App) {
    app.layout.memory.fact_list.confidence_buffer.pop();
}

pub(crate) async fn handle_confidence_submit(app: &mut App) {
    let conf: f64 = match app.layout.memory.fact_list.confidence_buffer.parse() {
        Ok(v) if (0.0..=1.0).contains(&v) => v,
        _ => {
            app.viewport.error_toast = Some(ErrorToast::new("Confidence must be 0.0–1.0".into()));
            return;
        }
    };
    app.layout.memory.fact_list.editing_confidence = false;

    let selected = app
        .layout
        .memory
        .selected_fact()
        .map(|f| (f.id.clone(), f.confidence));
    if let Some((id, prev_conf)) = selected {
        // NOTE: optimistic update: applied locally before the API round-trip to avoid UI lag
        if let Some(f) = app
            .layout
            .memory
            .fact_list
            .facts
            .iter_mut()
            .find(|f| f.id == id)
        {
            f.confidence = conf;
        }
        let client = app.client.clone();
        match client.knowledge_update_confidence(&id, conf).await {
            Ok(()) => {
                app.viewport.error_toast =
                    Some(ErrorToast::new(format!("Confidence set to {conf:.2}")));
            }
            Err(e) => {
                // NOTE: revert the optimistic update on failure
                if let Some(f) = app
                    .layout
                    .memory
                    .fact_list
                    .facts
                    .iter_mut()
                    .find(|f| f.id == id)
                {
                    f.confidence = prev_conf;
                }
                app.viewport.error_toast =
                    Some(ErrorToast::new(format!("Confidence update failed: {e}")));
            }
        }
    }
}

pub(crate) fn handle_confidence_cancel(app: &mut App) {
    app.layout.memory.fact_list.editing_confidence = false;
    app.layout.memory.fact_list.confidence_buffer.clear();
}

pub(crate) fn handle_search_open(app: &mut App) {
    app.layout.memory.search.search_active = true;
    app.layout.memory.search.search_query.clear();
}

pub(crate) fn handle_search_input(app: &mut App, c: char) {
    app.layout.memory.search.search_query.push(c);
}

pub(crate) fn handle_search_backspace(app: &mut App) {
    if app.layout.memory.search.search_query.is_empty() {
        app.layout.memory.search.search_active = false;
    } else {
        app.layout.memory.search.search_query.pop();
    }
}

pub(crate) async fn handle_search_submit(app: &mut App) {
    if app.layout.memory.search.search_query.is_empty() {
        return;
    }
    app.layout.memory.search.search_active = false;
    // NOTE: local fuzzy filtering: server-side semantic search not yet wired in
    let query = app.layout.memory.search.search_query.to_lowercase();
    let terms: Vec<&str> = query.split_whitespace().collect();

    let results: Vec<MemorySearchResult> = app
        .layout
        .memory
        .fact_list
        .facts
        .iter()
        .filter(|f| !f.lifecycle.is_forgotten)
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

    app.layout.memory.search.search_results = results;
    if !app.layout.memory.search.search_results.is_empty() {
        let msg = format!(
            "{} results for '{}'",
            app.layout.memory.search.search_results.len(),
            app.layout.memory.search.search_query
        );
        app.viewport.error_toast = Some(ErrorToast::new(msg));
    } else {
        app.viewport.error_toast = Some(ErrorToast::new(format!(
            "No results for '{}'",
            app.layout.memory.search.search_query
        )));
    }
}

pub(crate) fn handle_search_close(app: &mut App) {
    app.layout.memory.search.search_active = false;
    app.layout.memory.search.search_query.clear();
    app.layout.memory.search.search_results.clear();
}
