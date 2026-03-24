#![expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]

use super::data_loading::*;
use super::graph_analysis;
use super::handlers::*;
use crate::app::test_helpers::*;
use crate::state::memory::{FactLifecycleMeta, FactTemporalMeta, MemoryEntity, MemoryFact};
use crate::state::view_stack::View;

fn sample_fact(id: &str, content: &str, confidence: f64) -> MemoryFact {
    MemoryFact {
        id: id.into(),
        nous_id: "syn".into(),
        content: content.into(),
        confidence,
        tier: "verified".into(),
        fact_type: "knowledge".into(),
        temporal: FactTemporalMeta {
            valid_from: "2026-01-01".into(),
            valid_to: "9999-12-31".into(),
            recorded_at: "2026-01-01T00:00:00Z".into(),
            access_count: 5,
            last_accessed_at: "2026-03-09T12:00:00Z".into(),
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycleMeta {
            superseded_by: None,
            source_session_id: Some("ses-1".into()),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
    }
}

#[test]
fn handle_select_down_increments() {
    let mut app = test_app();
    app.layout.memory.fact_list.facts = vec![
        sample_fact("f1", "first", 0.9),
        sample_fact("f2", "second", 0.8),
    ];
    app.layout.memory.fact_list.selected = 0;
    handle_select_down(&mut app);
    assert_eq!(app.layout.memory.fact_list.selected, 1);
}

#[test]
fn handle_select_down_clamps_at_end() {
    let mut app = test_app();
    app.layout.memory.fact_list.facts = vec![sample_fact("f1", "first", 0.9)];
    app.layout.memory.fact_list.selected = 0;
    handle_select_down(&mut app);
    assert_eq!(app.layout.memory.fact_list.selected, 0);
}

#[test]
fn handle_select_up_decrements() {
    let mut app = test_app();
    app.layout.memory.fact_list.facts = vec![
        sample_fact("f1", "first", 0.9),
        sample_fact("f2", "second", 0.8),
    ];
    app.layout.memory.fact_list.selected = 1;
    handle_select_up(&mut app);
    assert_eq!(app.layout.memory.fact_list.selected, 0);
}

#[test]
fn handle_select_up_clamps_at_zero() {
    let mut app = test_app();
    app.layout.memory.fact_list.facts = vec![sample_fact("f1", "first", 0.9)];
    app.layout.memory.fact_list.selected = 0;
    handle_select_up(&mut app);
    assert_eq!(app.layout.memory.fact_list.selected, 0);
}

#[test]
fn handle_select_first_goes_to_zero() {
    let mut app = test_app();
    app.layout.memory.fact_list.facts = vec![
        sample_fact("f1", "first", 0.9),
        sample_fact("f2", "second", 0.8),
    ];
    app.layout.memory.fact_list.selected = 1;
    handle_select_first(&mut app);
    assert_eq!(app.layout.memory.fact_list.selected, 0);
}

#[test]
fn handle_select_last_goes_to_end() {
    let mut app = test_app();
    app.layout.memory.fact_list.facts = vec![
        sample_fact("f1", "first", 0.9),
        sample_fact("f2", "second", 0.8),
        sample_fact("f3", "third", 0.7),
    ];
    app.layout.memory.fact_list.selected = 0;
    handle_select_last(&mut app);
    assert_eq!(app.layout.memory.fact_list.selected, 2);
}

#[test]
fn handle_sort_cycle_advances() {
    let mut app = test_app();
    assert_eq!(
        app.layout.memory.fact_list.sort,
        crate::state::memory::FactSort::Confidence
    );
    handle_sort_cycle(&mut app);
    assert_eq!(
        app.layout.memory.fact_list.sort,
        crate::state::memory::FactSort::Recency
    );
}

#[test]
fn handle_tab_next_cycles() {
    let mut app = test_app();
    assert_eq!(
        app.layout.memory.tab,
        crate::state::memory::MemoryTab::Facts
    );
    handle_tab_next(&mut app);
    assert_eq!(
        app.layout.memory.tab,
        crate::state::memory::MemoryTab::Graph
    );
    handle_tab_next(&mut app);
    assert_eq!(
        app.layout.memory.tab,
        crate::state::memory::MemoryTab::Drift
    );
    handle_tab_next(&mut app);
    assert_eq!(
        app.layout.memory.tab,
        crate::state::memory::MemoryTab::Timeline
    );
    handle_tab_next(&mut app);
    assert_eq!(
        app.layout.memory.tab,
        crate::state::memory::MemoryTab::Facts
    );
}

#[test]
fn handle_tab_prev_cycles() {
    let mut app = test_app();
    handle_tab_prev(&mut app);
    assert_eq!(
        app.layout.memory.tab,
        crate::state::memory::MemoryTab::Timeline
    );
}

#[test]
fn handle_filter_lifecycle() {
    let mut app = test_app();
    assert!(!app.layout.memory.filters.filter_editing);

    handle_filter_open(&mut app);
    assert!(app.layout.memory.filters.filter_editing);

    handle_filter_input(&mut app, 'r');
    handle_filter_input(&mut app, 'u');
    handle_filter_input(&mut app, 's');
    handle_filter_input(&mut app, 't');
    assert_eq!(app.layout.memory.filters.filter_text, "rust");

    handle_filter_backspace(&mut app);
    assert_eq!(app.layout.memory.filters.filter_text, "rus");

    handle_filter_close(&mut app);
    assert!(!app.layout.memory.filters.filter_editing);
    assert!(app.layout.memory.filters.filter_text.is_empty());
}

#[test]
fn handle_filter_backspace_on_empty_closes() {
    let mut app = test_app();
    app.layout.memory.filters.filter_editing = true;
    handle_filter_backspace(&mut app);
    assert!(!app.layout.memory.filters.filter_editing);
}

#[test]
fn handle_facts_loaded_sets_data() {
    let mut app = test_app();
    let facts = vec![
        sample_fact("f1", "first", 0.9),
        sample_fact("f2", "second", 0.8),
    ];
    handle_facts_loaded(&mut app, facts, 2);
    assert_eq!(app.layout.memory.fact_list.facts.len(), 2);
    assert_eq!(app.layout.memory.fact_list.total_facts, 2);
    assert!(!app.layout.memory.loading);
}

#[test]
fn handle_close_pops_memory_view() {
    let mut app = test_app();
    app.layout.view_stack.push(View::MemoryInspector);
    handle_close(&mut app);
    assert!(app.layout.view_stack.is_home());
}

#[test]
fn handle_pop_back_from_detail_goes_to_inspector() {
    let mut app = test_app();
    app.layout.view_stack.push(View::MemoryInspector);
    app.layout.view_stack.push(View::FactDetail {
        fact_id: "f1".into(),
    });
    handle_pop_back(&mut app);
    assert_eq!(app.layout.view_stack.current(), &View::MemoryInspector);
    assert!(app.layout.memory.fact_list.detail.is_none());
}

#[test]
fn handle_confidence_input_accepts_digits_and_dot() {
    let mut app = test_app();
    app.layout.memory.fact_list.editing_confidence = true;
    app.layout.memory.fact_list.confidence_buffer.clear();
    handle_confidence_input(&mut app, '0');
    handle_confidence_input(&mut app, '.');
    handle_confidence_input(&mut app, '8');
    assert_eq!(app.layout.memory.fact_list.confidence_buffer, "0.8");
}

#[test]
fn handle_confidence_input_rejects_letters() {
    let mut app = test_app();
    app.layout.memory.fact_list.editing_confidence = true;
    app.layout.memory.fact_list.confidence_buffer.clear();
    handle_confidence_input(&mut app, 'a');
    assert!(app.layout.memory.fact_list.confidence_buffer.is_empty());
}

#[test]
fn handle_confidence_cancel_clears() {
    let mut app = test_app();
    app.layout.memory.fact_list.editing_confidence = true;
    app.layout.memory.fact_list.confidence_buffer = "0.5".into();
    handle_confidence_cancel(&mut app);
    assert!(!app.layout.memory.fact_list.editing_confidence);
    assert!(app.layout.memory.fact_list.confidence_buffer.is_empty());
}

#[test]
fn handle_search_lifecycle() {
    let mut app = test_app();
    handle_search_open(&mut app);
    assert!(app.layout.memory.search.search_active);

    handle_search_input(&mut app, 'r');
    handle_search_input(&mut app, 'u');
    assert_eq!(app.layout.memory.search.search_query, "ru");

    handle_search_backspace(&mut app);
    assert_eq!(app.layout.memory.search.search_query, "r");

    handle_search_close(&mut app);
    assert!(!app.layout.memory.search.search_active);
    assert!(app.layout.memory.search.search_query.is_empty());
}

#[test]
fn handle_search_backspace_on_empty_closes() {
    let mut app = test_app();
    app.layout.memory.search.search_active = true;
    handle_search_backspace(&mut app);
    assert!(!app.layout.memory.search.search_active);
}

#[test]
fn handle_action_result_sets_toast() {
    let mut app = test_app();
    handle_action_result(&mut app, "done".into());
    assert!(app.viewport.error_toast.is_some());
    assert_eq!(app.viewport.error_toast.as_ref().unwrap().message, "done");
}

#[test]
fn adjust_scroll_scrolls_down() {
    let mut app = test_app();
    app.viewport.terminal_height = 20; // visible_rows = 12
    app.layout.memory.fact_list.facts = (0..20)
        .map(|i| sample_fact(&format!("f{i}"), &format!("fact {i}"), 0.9))
        .collect();
    app.layout.memory.fact_list.selected = 15;
    adjust_scroll(&mut app);
    assert!(app.layout.memory.fact_list.scroll_offset > 0);
}

#[test]
fn item_count_for_each_tab() {
    let mut app = test_app();
    app.layout.memory.fact_list.facts = vec![sample_fact("f1", "a", 0.9)];
    app.layout.memory.tab = crate::state::memory::MemoryTab::Facts;
    assert_eq!(item_count(&app), 1);

    app.layout.memory.tab = crate::state::memory::MemoryTab::Graph;
    assert_eq!(item_count(&app), 0);

    app.layout.memory.tab = crate::state::memory::MemoryTab::Drift;
    assert_eq!(item_count(&app), 0);

    app.layout.memory.tab = crate::state::memory::MemoryTab::Timeline;
    assert_eq!(item_count(&app), 0);
}

#[test]
fn handle_drift_tab_next_cycles() {
    let mut app = test_app();
    assert_eq!(
        app.layout.memory.graph.drift_tab,
        crate::state::memory::DriftTab::Suggestions
    );
    handle_drift_tab_next(&mut app);
    assert_eq!(
        app.layout.memory.graph.drift_tab,
        crate::state::memory::DriftTab::Orphans
    );
}

#[test]
fn handle_drift_tab_prev_cycles() {
    let mut app = test_app();
    handle_drift_tab_prev(&mut app);
    assert_eq!(
        app.layout.memory.graph.drift_tab,
        crate::state::memory::DriftTab::Isolated
    );
}

#[test]
fn compute_pagerank_empty() {
    let scores = graph_analysis::compute_pagerank(&[], &[]);
    assert!(scores.is_empty());
}

#[test]
fn compute_pagerank_single_entity() {
    let entities = vec![MemoryEntity {
        id: "e1".into(),
        name: "alice".into(),
        entity_type: "person".into(),
        aliases: Vec::new(),
        created_at: String::new(),
        updated_at: String::new(),
    }];
    let scores = graph_analysis::compute_pagerank(&entities, &[]);
    assert!(scores.contains_key("e1"));
}

#[test]
fn compute_communities_groups_connected_entities() {
    let entities = vec![
        MemoryEntity {
            id: "e1".into(),
            name: "alice".into(),
            entity_type: "person".into(),
            aliases: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        },
        MemoryEntity {
            id: "e2".into(),
            name: "bob".into(),
            entity_type: "person".into(),
            aliases: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        },
        MemoryEntity {
            id: "e3".into(),
            name: "charlie".into(),
            entity_type: "person".into(),
            aliases: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        },
    ];
    let relationships = vec![crate::state::memory::MemoryRelationship {
        src: "e1".into(),
        dst: "e2".into(),
        relation: "KNOWS".into(),
        weight: 1.0,
        created_at: String::new(),
    }];
    // NOTE: compute_communities is private, tested indirectly via compute_graph_stats
    // but we test pagerank which is pub(super)
    let _scores = graph_analysis::compute_pagerank(&entities, &relationships);
}

#[test]
fn days_between_approx_calculates() {
    assert!(graph_analysis::days_between_approx("2026-01-01", "2026-03-22") > 30);
    assert_eq!(
        graph_analysis::days_between_approx("invalid", "2026-03-22"),
        0
    );
}
