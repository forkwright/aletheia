use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::{Overlay, SearchResult, SearchResultKind, SessionSearchOverlay};

pub(crate) fn handle_open(app: &mut App) {
    let mut overlay = SessionSearchOverlay::new();
    // Pre-populate results with all sessions across all agents
    overlay.results = build_results(app, "");
    app.overlay = Some(Overlay::SessionSearch(overlay));
}

pub(crate) fn handle_close(app: &mut App) {
    app.overlay = None;
}

pub(crate) fn handle_input(app: &mut App, c: char) {
    if let Some(Overlay::SessionSearch(ref mut search)) = app.overlay {
        search.query.insert(search.cursor, c);
        search.cursor += c.len_utf8();
        search.selected = 0;
    }
    refresh_results(app);
}

pub(crate) fn handle_backspace(app: &mut App) {
    let should_close = if let Some(Overlay::SessionSearch(ref mut search)) = app.overlay {
        if search.cursor > 0 {
            let prev = search.query[..search.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            search.query.drain(prev..search.cursor);
            search.cursor = prev;
            search.selected = 0;
            false
        } else {
            true // close on empty backspace
        }
    } else {
        false
    };
    if should_close {
        app.overlay = None;
    } else {
        refresh_results(app);
    }
}

pub(crate) fn handle_up(app: &mut App) {
    if let Some(Overlay::SessionSearch(ref mut search)) = app.overlay {
        search.selected = search.selected.saturating_sub(1);
    }
}

pub(crate) fn handle_down(app: &mut App) {
    if let Some(Overlay::SessionSearch(ref mut search)) = app.overlay {
        let max = search.results.len().saturating_sub(1);
        search.selected = (search.selected + 1).min(max);
    }
}

pub(crate) async fn handle_select(app: &mut App) {
    let (agent_id, session_id) = match &app.overlay {
        Some(Overlay::SessionSearch(search)) => {
            if let Some(result) = search.results.get(search.selected) {
                (result.agent_id.clone(), result.session_id.clone())
            } else {
                return;
            }
        }
        _ => return,
    };

    app.overlay = None;

    // Switch to the agent if different
    if app.focused_agent.as_ref() != Some(&agent_id) {
        app.save_scroll_state();
        if let Some(a) = app.agents.iter_mut().find(|a| a.id == agent_id) {
            a.has_notification = false;
        }
        app.focused_agent = Some(agent_id.clone());

        // Load sessions if not already loaded
        if let Some(agent) = app.agents.iter().find(|a| a.id == agent_id)
            && agent.sessions.is_empty()
            && let Ok(sessions) = app.client.sessions(&agent_id).await
            && let Some(agent) = app.agents.iter_mut().find(|a| a.id == agent_id)
        {
            agent.sessions = sessions;
        }
    }

    // Switch to the target session
    app.focused_session_id = Some(session_id.clone());
    match app.client.history(&session_id).await {
        Ok(history) => {
            use crate::sanitize::sanitize_for_display;
            use crate::update::extract_text_content;
            app.messages = history
                .into_iter()
                .filter_map(|m| {
                    if m.role != "user" && m.role != "assistant" {
                        return None;
                    }
                    let text = extract_text_content(&m.content)?;
                    let text = sanitize_for_display(&text).into_owned();
                    let text_lower = text.to_lowercase();
                    Some(crate::state::ChatMessage {
                        role: sanitize_for_display(&m.role).into_owned(),
                        text,
                        text_lower,
                        timestamp: m.created_at.map(|t| sanitize_for_display(&t).into_owned()),
                        model: m.model.map(|m| sanitize_for_display(&m).into_owned()),
                        is_streaming: false,
                        tool_calls: Vec::new(),
                    })
                })
                .collect();
            app.scroll_to_bottom();
        }
        Err(e) => {
            app.error_toast = Some(ErrorToast::new(format!("Load failed: {e}")));
        }
    }
}

fn refresh_results(app: &mut App) {
    let query = match &app.overlay {
        Some(Overlay::SessionSearch(search)) => search.query.clone(),
        _ => return,
    };
    let results = build_results(app, &query);
    if let Some(Overlay::SessionSearch(ref mut search)) = app.overlay {
        search.results = results;
    }
}

fn build_results(app: &App, query: &str) -> Vec<SearchResult> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    // Search session names/labels across all agents
    for agent in &app.agents {
        for session in &agent.sessions {
            if !session.is_interactive() {
                continue;
            }
            let label = session
                .display_name
                .as_deref()
                .unwrap_or(&session.key)
                .to_string();

            if query_lower.is_empty() || label.to_lowercase().contains(&query_lower) {
                results.push(SearchResult {
                    agent_id: agent.id.clone(),
                    agent_name: agent.name.clone(),
                    session_id: session.id.clone(),
                    session_label: label,
                    snippet: format!("{} messages", session.message_count),
                    kind: SearchResultKind::SessionName,
                });
            }
        }
    }

    // Search current session message content
    if !query_lower.is_empty() {
        for (i, msg) in app.messages.iter().enumerate() {
            if msg.text_lower.contains(&query_lower) {
                let snippet = excerpt(&msg.text, &query_lower, 60);
                let session_id = app.focused_session_id.clone().unwrap_or_else(|| "".into());
                let agent_name = app
                    .focused_agent
                    .as_ref()
                    .and_then(|id| app.agents.iter().find(|a| &a.id == id))
                    .map(|a| a.name.clone())
                    .unwrap_or_default();
                let agent_id = app.focused_agent.clone().unwrap_or_else(|| "".into());
                results.push(SearchResult {
                    agent_id,
                    agent_name,
                    session_id,
                    session_label: format!("msg #{}", i + 1),
                    snippet,
                    kind: SearchResultKind::MessageContent {
                        role: msg.role.clone(),
                    },
                });
            }
        }
    }

    results
}

/// Extract a short excerpt around the first occurrence of `needle` in `text`.
fn excerpt(text: &str, needle: &str, max_len: usize) -> String {
    let lower = text.to_lowercase();
    let pos = match lower.find(needle) {
        Some(p) => p,
        None => return text.chars().take(max_len).collect(),
    };

    // Find the start position (back up to context)
    let context = max_len / 2;
    let start = pos.saturating_sub(context);
    // Align to char boundary
    let start = text[..start]
        .char_indices()
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0);

    let result: String = text[start..].chars().take(max_len).collect();
    if start > 0 {
        format!("...{result}")
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;

    #[test]
    fn excerpt_finds_match() {
        let result = excerpt("hello world foo bar", "world", 20);
        assert!(result.contains("world"));
    }

    #[test]
    fn excerpt_no_match_truncates() {
        let result = excerpt("hello world", "xyz", 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn handle_open_creates_overlay() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(crate::api::types::Session {
            id: "s1".into(),
            nous_id: "syn".into(),
            key: "main".to_string(),
            status: None,
            message_count: 5,
            session_type: None,
            updated_at: None,
            display_name: None,
        });
        app.agents.push(agent);
        handle_open(&mut app);
        assert!(matches!(app.overlay, Some(Overlay::SessionSearch(_))));
    }

    #[test]
    fn handle_close_clears_overlay() {
        let mut app = test_app();
        app.overlay = Some(Overlay::SessionSearch(SessionSearchOverlay::new()));
        handle_close(&mut app);
        assert!(app.overlay.is_none());
    }

    #[test]
    fn handle_input_updates_query() {
        let mut app = test_app();
        app.overlay = Some(Overlay::SessionSearch(SessionSearchOverlay::new()));
        handle_input(&mut app, 't');
        handle_input(&mut app, 'e');
        if let Some(Overlay::SessionSearch(ref search)) = app.overlay {
            assert_eq!(search.query, "te");
        }
    }

    #[test]
    fn handle_backspace_on_empty_closes() {
        let mut app = test_app();
        app.overlay = Some(Overlay::SessionSearch(SessionSearchOverlay::new()));
        handle_backspace(&mut app);
        assert!(app.overlay.is_none());
    }

    #[test]
    fn build_results_matches_session_name() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(crate::api::types::Session {
            id: "s1".into(),
            nous_id: "syn".into(),
            key: "main".to_string(),
            status: None,
            message_count: 5,
            session_type: None,
            updated_at: None,
            display_name: Some("Debug Session".to_string()),
        });
        app.agents.push(agent);

        let results = build_results(&app, "debug");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_label, "Debug Session");
    }

    #[test]
    fn build_results_empty_query_returns_all() {
        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.sessions.push(crate::api::types::Session {
            id: "s1".into(),
            nous_id: "syn".into(),
            key: "main".to_string(),
            status: None,
            message_count: 5,
            session_type: None,
            updated_at: None,
            display_name: None,
        });
        app.agents.push(agent);

        let results = build_results(&app, "");
        assert!(!results.is_empty());
    }

    #[test]
    fn build_results_searches_message_content() {
        let mut app = test_app_with_messages(vec![
            ("user", "hello world"),
            ("assistant", "goodbye world"),
        ]);
        app.focused_agent = Some("syn".into());
        app.focused_session_id = Some("s1".into());

        let results = build_results(&app, "goodbye");
        assert!(
            results
                .iter()
                .any(|r| matches!(r.kind, SearchResultKind::MessageContent { .. }))
        );
    }
}
