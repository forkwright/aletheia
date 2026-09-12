//! Shared chat activation helpers for cross-view navigation.

use skene::api::types::HistoryMessage;
use skene::id::{ApiNousId, ApiSessionId, TurnId};

use crate::components::chat::ChatState;
use crate::components::chat::{ChatMessage as LegacyChatMessage, MessageRole};
use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::chat::ChatSelection;
use crate::state::platform::WindowState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChatActivation {
    /// Whether the active agent/session changed.
    pub session_changed: bool,
}

/// Resolve the session key used to stream a chat turn.
///
/// Session-picker selections keep their explicit key. Sidebar-only agent
/// selections have no session key, so they use a stable key scoped to the
/// active agent instead of sharing one process-wide fallback.
pub(crate) fn resolve_chat_session_key(nous_id: &ApiNousId, session_key: Option<&str>) -> String {
    session_key.map_or_else(|| format!("{nous_id}:default"), str::to_owned)
}

/// Build the selection for a nous's canonical ongoing conversation — the
/// single continuous chat the desktop app attaches to when the operator
/// enters a nous without picking a specific session.
///
/// The returned selection has no server session id yet; the caller resolves
/// it through `POST /api/v1/sessions/resolve` before fetching history.
pub(crate) fn canonical_agent_selection(agent_id: &ApiNousId, title: String) -> ChatSelection {
    ChatSelection::new(
        agent_id.clone(),
        resolve_chat_session_key(agent_id, None),
        title,
    )
}

/// Stamp local streaming state to reattach to a session's already
/// in-progress turn, so the abort control renders immediately on entry
/// instead of waiting for the first reattached event to arrive.
///
/// Returns the turn to reattach to, or `None` when the session is idle
/// (nothing to reattach to, and `chat_state` is left untouched).
///
/// WHY(#7297): mid-turn re-entry -- reloading the app while a turn is
/// running -- must regain abort control. Since PR #7267, `POST
/// /api/v1/sessions/resolve` reports `active_turn_id` on the session; the
/// desktop chat treats that as "streaming" up front so `InputBar`'s abort
/// control (driven purely by `streaming.is_streaming`) shows up without
/// waiting on the network. The caller reattaches to the turn's event
/// stream (`skene::api::streaming::reattach_turn_stream`) to keep it live.
pub(crate) fn apply_active_turn_reattachment(
    chat_state: &mut ChatState,
    session_id: ApiSessionId,
    active_turn_id: Option<TurnId>,
) -> Option<TurnId> {
    let turn_id = active_turn_id?;
    chat_state.streaming.is_streaming = true;
    chat_state.streaming.turn_id = Some(turn_id.clone());
    chat_state.streaming.session_id = Some(session_id);
    Some(turn_id)
}

pub(crate) fn activate_chat_selection(
    selection: &ChatSelection,
    legacy_state: &mut ChatState,
    agent_store: &mut AgentStore,
    tab_bar: &mut TabBar,
    window_state: &mut WindowState,
) -> ChatActivation {
    let session_changed = legacy_state.agent_id.as_ref() != Some(&selection.agent_id)
        || legacy_state.session_key.as_deref() != Some(selection.session_key.as_str());

    if session_changed {
        legacy_state.messages.clear();
    }
    legacy_state.agent_id = Some(selection.agent_id.clone());
    legacy_state.session_key = Some(selection.session_key.clone());

    let agent_known = agent_store.set_active(&selection.agent_id);
    debug_assert!(
        agent_known || agent_store.is_empty(),
        "chat selection referenced an agent absent from AgentStore"
    );
    window_state.active_sessions.insert(
        selection.agent_id.to_string(),
        selection.session_key.clone(),
    );

    if let Some(idx) = tab_bar.tabs.iter().position(|tab| {
        tab.agent_id == selection.agent_id
            && tab.session_key.as_deref() == Some(selection.session_key.as_str())
    }) {
        if let Some(tab) = tab_bar.tabs.get_mut(idx) {
            if tab.session_id.is_none() {
                tab.session_id = selection.session_id.clone();
            }
            if tab.message_count.is_none() {
                tab.message_count = selection.message_count;
            }
        }
        tab_bar.active = idx;
    } else {
        let idx = match selection.session_id.clone() {
            Some(session_id) => tab_bar.create_for_existing_session(
                selection.agent_id.clone(),
                session_id,
                selection.session_key.clone(),
                selection.message_count,
                selection.title.clone(),
            ),
            _ => tab_bar.create_for_session(
                selection.agent_id.clone(),
                selection.session_key.clone(),
                selection.title.clone(),
            ),
        };
        tab_bar.active = idx;
    }

    ChatActivation { session_changed }
}

/// Convert one page of history into render-ready messages, collapsing each
/// turn's raw per-call tool-result messages into one summary row per tool
/// type.
///
/// WHY(#7298): history replay previously rendered one bubble per raw
/// `role: "tool"` message -- a turn with a dozen `Read` calls filled the
/// pane with a dozen near-identical rows carrying the raw tool payload.
/// Turns are delimited by user messages (history carries no per-message
/// turn id, #4911); within a turn, every tool-result message sharing a
/// tool name collapses into a single row at the position of that tool's
/// first call, carrying the total call count. A turn that spans a page
/// boundary (>100 tool calls) is summarized per page, not across pages --
/// a rare case given `HISTORY_PAGE_SIZE_QUERY`.
pub(crate) fn history_messages_to_legacy(messages: &[HistoryMessage]) -> Vec<LegacyChatMessage> {
    let mut out = Vec::with_capacity(messages.len());
    for turn in split_into_turns(messages) {
        collapse_turn_tool_calls(turn, &mut out);
    }
    out
}

pub(crate) fn oldest_history_seq(messages: &[HistoryMessage]) -> Option<i64> {
    messages.iter().filter_map(|msg| msg.seq).min()
}

/// Split a chronological page of history into turns, each starting at a
/// `user`-role message (the only turn boundary history carries).
/// Messages preceding the first user message, if any, form their own
/// leading turn.
fn split_into_turns(messages: &[HistoryMessage]) -> Vec<&[HistoryMessage]> {
    let mut turns = Vec::new();
    let mut start = 0;
    for (i, message) in messages.iter().enumerate() {
        if i > start && message.role == "user" {
            turns.push(&messages[start..i]);
            start = i;
        }
    }
    if start < messages.len() {
        turns.push(&messages[start..]);
    }
    turns
}

/// Append one turn's messages to `out`, collapsing tool-result messages
/// that share a tool name into a single summary row per name.
fn collapse_turn_tool_calls(turn: &[HistoryMessage], out: &mut Vec<LegacyChatMessage>) {
    let mut tool_counts: Vec<(String, u32)> = Vec::new();
    for message in turn {
        if message.role != "tool" {
            continue;
        }
        let name = tool_display_name(message);
        match tool_counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => tool_counts.push((name, 1)),
        }
    }

    let mut summarized: std::collections::HashSet<String> = std::collections::HashSet::new();
    for message in turn {
        if message.role == "tool" {
            let name = tool_display_name(message);
            if summarized.insert(name.clone()) {
                let count = tool_counts
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map_or(1, |(_, count)| *count);
                out.push(tool_summary_message(&name, count));
            }
            continue;
        }
        if let Some(legacy) = history_message_to_legacy(message) {
            out.push(legacy);
        }
    }
}

fn tool_display_name(message: &HistoryMessage) -> String {
    message
        .tool_name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "tool".to_string())
}

/// Build the single summary row standing in for `count` raw calls to the
/// same tool within one turn.
fn tool_summary_message(tool_name: &str, count: u32) -> LegacyChatMessage {
    let content = if count == 1 {
        tool_name.to_string()
    } else {
        format!("{tool_name} \u{d7}{count}")
    };
    LegacyChatMessage {
        role: MessageRole::Assistant,
        content,
        model: None,
        tool_calls: count,
        input_tokens: 0,
        output_tokens: 0,
        thinking: None,
        tool_call_details: Vec::new(),
        plans: Vec::new(),
        turn_id: None,
        session_id: None,
        request_id: None,
    }
}

fn history_message_to_legacy(message: &HistoryMessage) -> Option<LegacyChatMessage> {
    let role = match message.role.as_str() {
        "user" => MessageRole::User,
        "assistant" | "system" => MessageRole::Assistant,
        other => {
            tracing::debug!(role = other, "skipping unsupported history message role");
            return None;
        }
    };

    Some(LegacyChatMessage {
        role,
        content: history_content_to_string(message.content.as_ref()),
        // WHY(#4911): pylon's history wire format carries no per-message model
        // and never did, so the previous `message.model.clone()` read a field
        // that was always None against real data. The model is a session-level
        // fact; `views/sessions/mod.rs` takes it from `session.model`. This
        // conversion has no session in scope, so it leaves the field empty
        // rather than reintroducing a phantom per-message source.
        model: None,
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        thinking: None,
        tool_call_details: Vec::new(),
        plans: Vec::new(),
        turn_id: None,
        session_id: None,
        request_id: None,
    })
}

fn history_content_to_string(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(value) => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        None => String::new(),
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use skene::api::types::{Agent, HistoryResponse};
    use skene::id::{ApiNousId, ApiSessionId};

    use super::*;

    fn agent(id: &str) -> Agent {
        Agent {
            id: ApiNousId::from(id),
            name: Some(id.to_string()),
            model: None,
            emoji: None,
            status: None,
            tools: Vec::new(),
            enabled: None,
        }
    }

    fn selection() -> ChatSelection {
        ChatSelection::new(
            ApiNousId::from("syn"),
            "incident-review".to_string(),
            "Incident Review".to_string(),
        )
    }

    #[test]
    fn sidebar_selected_agents_route_to_distinct_default_session_keys() {
        let mut agent_store = AgentStore::new();
        agent_store.load_from_api(vec![agent("syn"), agent("arc")]);

        assert!(agent_store.set_active(&ApiNousId::from("syn")));
        let first_agent = agent_store.active_id.as_ref().unwrap();
        let first_key = resolve_chat_session_key(first_agent, None);

        assert!(agent_store.set_active(&ApiNousId::from("arc")));
        let second_agent = agent_store.active_id.as_ref().unwrap();
        let second_key = resolve_chat_session_key(second_agent, None);

        assert_eq!(first_key, "syn:default");
        assert_eq!(second_key, "arc:default");
        assert_ne!(first_key, second_key);
    }

    #[test]
    fn resolve_chat_session_key_preserves_explicit_selection() {
        let key = resolve_chat_session_key(&ApiNousId::from("syn"), Some("incident-review"));

        assert_eq!(key, "incident-review");
    }

    #[test]
    fn canonical_agent_selection_uses_stable_default_key_without_session_id() {
        let selection = canonical_agent_selection(&ApiNousId::from("syn"), "Syn".to_string());

        assert_eq!(selection.agent_id.as_ref(), "syn");
        assert_eq!(selection.session_key, "syn:default");
        assert!(
            selection.session_id.is_none(),
            "canonical selection carries no server id until resolve answers"
        );
        assert_eq!(selection.title, "Syn");
    }

    #[test]
    fn activate_chat_selection_sets_agent_session_tab_and_window_state() {
        let mut chat_state = ChatState::default();
        chat_state.messages.push(LegacyChatMessage {
            role: MessageRole::User,
            content: "old draft".to_string(),
            model: None,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            thinking: None,
            tool_call_details: Vec::new(),
            plans: Vec::new(),
            turn_id: None,
            session_id: None,
            request_id: None,
        });
        let mut agent_store = AgentStore::new();
        agent_store.load_from_api(vec![agent("syn")]);
        let mut tab_bar = TabBar::new();
        let mut window_state = WindowState::default();

        let activation = activate_chat_selection(
            &selection(),
            &mut chat_state,
            &mut agent_store,
            &mut tab_bar,
            &mut window_state,
        );

        assert!(activation.session_changed);
        assert_eq!(agent_store.active_id.as_deref(), Some("syn"));
        assert_eq!(chat_state.agent_id.as_deref(), Some("syn"));
        assert_eq!(chat_state.session_key.as_deref(), Some("incident-review"));
        assert!(chat_state.messages.is_empty());
        let active_tab = tab_bar.active_tab().unwrap();
        assert_eq!(active_tab.title, "Incident Review");
        assert_eq!(active_tab.session_key.as_deref(), Some("incident-review"));
        assert_eq!(
            window_state.active_sessions.get("syn").map(String::as_str),
            Some("incident-review")
        );
    }

    #[test]
    fn activate_chat_selection_preserves_messages_when_selection_is_unchanged() {
        let mut chat_state = ChatState {
            agent_id: Some(ApiNousId::from("syn")),
            session_key: Some("incident-review".to_string()),
            ..ChatState::default()
        };
        chat_state.messages.push(LegacyChatMessage {
            role: MessageRole::User,
            content: "keep me".to_string(),
            model: None,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            thinking: None,
            tool_call_details: Vec::new(),
            plans: Vec::new(),
            turn_id: None,
            session_id: None,
            request_id: None,
        });
        let mut agent_store = AgentStore::new();
        agent_store.load_from_api(vec![agent("syn")]);
        let mut tab_bar = TabBar::new();
        tab_bar.create_for_session(
            ApiNousId::from("syn"),
            "incident-review".to_string(),
            "Incident Review",
        );
        let mut window_state = WindowState::default();

        let activation = activate_chat_selection(
            &selection(),
            &mut chat_state,
            &mut agent_store,
            &mut tab_bar,
            &mut window_state,
        );

        assert!(!activation.session_changed);
        assert_eq!(chat_state.messages.len(), 1);
        assert_eq!(tab_bar.len(), 1);
    }

    #[test]
    fn activate_chat_selection_keeps_existing_session_metadata_on_tab() {
        let mut chat_state = ChatState::default();
        let mut agent_store = AgentStore::new();
        agent_store.load_from_api(vec![agent("syn")]);
        let mut tab_bar = TabBar::new();
        let mut window_state = WindowState::default();
        let selection = ChatSelection::for_existing_session(
            ApiNousId::from("syn"),
            ApiSessionId::from("session-id"),
            "incident-review".to_string(),
            "Incident Review".to_string(),
            4,
        );

        activate_chat_selection(
            &selection,
            &mut chat_state,
            &mut agent_store,
            &mut tab_bar,
            &mut window_state,
        );

        let active_tab = tab_bar.active_tab().unwrap();
        assert_eq!(active_tab.session_id.as_deref(), Some("session-id"));
        assert_eq!(active_tab.message_count, Some(4));
    }

    #[test]
    fn activating_existing_session_loads_history_messages() {
        let mut chat_state = ChatState::default();
        let mut agent_store = AgentStore::new();
        agent_store.load_from_api(vec![agent("syn")]);
        let mut tab_bar = TabBar::new();
        let mut window_state = WindowState::default();
        let selection = ChatSelection::for_existing_session(
            ApiNousId::from("syn"),
            ApiSessionId::from("session-id"),
            "incident-review".to_string(),
            "Incident Review".to_string(),
            2,
        );
        let json = r#"{
            "messages": [
                {"id": 1, "seq": 1, "role": "user", "content": "What happened?"},
                {"id": 2, "seq": 2, "role": "assistant", "content": "Recovered the transcript.", "model": "claude-opus-4-6"}
            ]
        }"#;

        activate_chat_selection(
            &selection,
            &mut chat_state,
            &mut agent_store,
            &mut tab_bar,
            &mut window_state,
        );
        let messages = serde_json::from_str::<HistoryResponse>(json)
            .unwrap()
            .messages;
        chat_state.messages = history_messages_to_legacy(&messages);

        assert_eq!(oldest_history_seq(&messages), Some(1));
        assert_eq!(chat_state.messages.len(), 2);
        assert_eq!(chat_state.messages[0].role, MessageRole::User);
        assert_eq!(chat_state.messages[0].content, "What happened?");
        assert_eq!(chat_state.messages[1].role, MessageRole::Assistant);
        assert_eq!(chat_state.messages[1].content, "Recovered the transcript.");
        // WHY(#4911): the fixture above deliberately still carries a "model"
        // key, because a real pylon history payload never has one -- this
        // pins that an unexpected per-message key cannot leak into the
        // rendered model. The previous assertion required it to flow through,
        // which is what kept the phantom field alive: the only place a
        // per-message model ever existed was this fixture. The model is a
        // session-level fact and views/sessions takes it from session.model.
        assert!(
            chat_state.messages[1].model.is_none(),
            "a per-message model must not be sourced from history; got {:?}",
            chat_state.messages[1].model
        );
    }

    // WHY(#7298): history replay must collapse raw per-call tool messages
    // into one summary row per tool type per turn instead of rendering one
    // bubble per call.
    #[test]
    fn history_messages_collapse_repeated_tool_calls_into_one_row_per_tool_type() {
        let json = r#"{
            "messages": [
                {"seq": 1, "role": "user", "content": "grep the logs"},
                {"seq": 2, "role": "tool", "tool_name": "Read", "content": "log line 1"},
                {"seq": 3, "role": "tool", "tool_name": "Read", "content": "log line 2"},
                {"seq": 4, "role": "tool", "tool_name": "Read", "content": "log line 3"},
                {"seq": 5, "role": "tool", "tool_name": "Bash", "content": "exit 0"},
                {"seq": 6, "role": "assistant", "content": "Found it."}
            ]
        }"#;
        let messages = serde_json::from_str::<HistoryResponse>(json)
            .unwrap()
            .messages;

        let legacy = history_messages_to_legacy(&messages);

        // 4 rows, not 6: user, one Read summary, one Bash summary, assistant.
        assert_eq!(
            legacy.len(),
            4,
            "expected raw per-call tool rows collapsed to one per tool type, got {legacy:?}"
        );
        assert_eq!(legacy[0].role, MessageRole::User);
        assert_eq!(legacy[1].content, "Read \u{d7}3");
        assert_eq!(legacy[1].tool_calls, 3);
        assert_eq!(legacy[2].content, "Bash");
        assert_eq!(legacy[2].tool_calls, 1);
        assert_eq!(legacy[3].role, MessageRole::Assistant);
        assert_eq!(legacy[3].content, "Found it.");
    }

    #[test]
    fn history_messages_keep_tool_summaries_scoped_to_their_own_turn() {
        let json = r#"{
            "messages": [
                {"seq": 1, "role": "user", "content": "first"},
                {"seq": 2, "role": "tool", "tool_name": "Read", "content": "a"},
                {"seq": 3, "role": "tool", "tool_name": "Read", "content": "b"},
                {"seq": 4, "role": "assistant", "content": "done one"},
                {"seq": 5, "role": "user", "content": "second"},
                {"seq": 6, "role": "tool", "tool_name": "Read", "content": "c"},
                {"seq": 7, "role": "assistant", "content": "done two"}
            ]
        }"#;
        let messages = serde_json::from_str::<HistoryResponse>(json)
            .unwrap()
            .messages;

        let legacy = history_messages_to_legacy(&messages);

        let read_summaries: Vec<&str> = legacy
            .iter()
            .filter(|m| m.content.starts_with("Read"))
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(
            read_summaries,
            vec!["Read \u{d7}2", "Read"],
            "each turn's tool calls must summarize independently, not merge across turns"
        );
    }

    #[test]
    fn history_messages_fall_back_to_generic_tool_label_when_name_missing() {
        let json = r#"{
            "messages": [
                {"seq": 1, "role": "user", "content": "run something"},
                {"seq": 2, "role": "tool", "content": "raw result"}
            ]
        }"#;
        let messages = serde_json::from_str::<HistoryResponse>(json)
            .unwrap()
            .messages;

        let legacy = history_messages_to_legacy(&messages);

        assert_eq!(legacy.len(), 2);
        assert_eq!(legacy[1].content, "tool");
    }

    #[test]
    fn activating_existing_session_preserves_live_stream_state() {
        let mut chat_state = ChatState::default();
        chat_state.messages.push(LegacyChatMessage {
            role: MessageRole::User,
            content: "old draft".to_string(),
            model: None,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            thinking: None,
            tool_call_details: Vec::new(),
            plans: Vec::new(),
            turn_id: None,
            session_id: None,
            request_id: None,
        });
        chat_state.streaming.is_streaming = true;
        chat_state.streaming.text = "partial answer".to_string();
        let mut agent_store = AgentStore::new();
        agent_store.load_from_api(vec![agent("syn")]);
        let mut tab_bar = TabBar::new();
        let mut window_state = WindowState::default();
        let selection = ChatSelection::for_existing_session(
            ApiNousId::from("syn"),
            ApiSessionId::from("session-id"),
            "incident-review".to_string(),
            "Incident Review".to_string(),
            4,
        );

        let activation = activate_chat_selection(
            &selection,
            &mut chat_state,
            &mut agent_store,
            &mut tab_bar,
            &mut window_state,
        );

        assert!(activation.session_changed);
        assert!(chat_state.messages.is_empty());
        assert!(chat_state.streaming.is_streaming);
        assert_eq!(chat_state.streaming.text, "partial answer");
    }

    // WHY(#7297): mid-turn re-entry -- a session with `active_turn_id` set
    // must render the abort control immediately (driven by
    // `streaming.is_streaming`) rather than only after a reattached event
    // arrives over the network.
    #[test]
    fn session_with_active_turn_id_renders_abort_control() {
        let mut chat_state = ChatState::default();
        assert!(
            !chat_state.streaming.is_streaming,
            "a fresh chat state must start idle"
        );

        let reattach_turn_id = apply_active_turn_reattachment(
            &mut chat_state,
            ApiSessionId::from("session-id"),
            Some(TurnId::from("turn-1")),
        );

        assert_eq!(reattach_turn_id, Some(TurnId::from("turn-1")));
        assert!(
            chat_state.streaming.is_streaming,
            "InputBar's abort control renders only when streaming.is_streaming is true"
        );
        assert_eq!(chat_state.streaming.turn_id, Some(TurnId::from("turn-1")));
        assert_eq!(
            chat_state.streaming.session_id,
            Some(ApiSessionId::from("session-id"))
        );
    }

    #[test]
    fn idle_session_does_not_stamp_streaming_state() {
        let mut chat_state = ChatState::default();

        let reattach_turn_id =
            apply_active_turn_reattachment(&mut chat_state, ApiSessionId::from("session-id"), None);

        assert_eq!(reattach_turn_id, None);
        assert!(!chat_state.streaming.is_streaming);
        assert_eq!(chat_state.streaming.turn_id, None);
    }

    #[test]
    fn abort_after_reattachment_ends_the_turn() {
        use crate::components::chat::ChatStateManager;
        use skene::events::StreamEvent;

        let mut chat_state = ChatState::default();
        apply_active_turn_reattachment(
            &mut chat_state,
            ApiSessionId::from("session-id"),
            Some(TurnId::from("turn-1")),
        );
        assert!(chat_state.streaming.is_streaming, "precondition: reattached");

        // WHY: this mirrors exactly what `on_abort` triggers in production --
        // cancelling the shared `cancel_token` makes the stream task apply a
        // `TurnAbort` before it returns, whether the task is the original
        // sender or a reattached listener.
        let mut manager = ChatStateManager::new();
        let applied = manager.apply(
            StreamEvent::TurnAbort {
                reason: "cancelled by user".to_string(),
            },
            &mut chat_state,
        );

        assert!(applied);
        assert!(
            !chat_state.streaming.is_streaming,
            "abort must end the turn: the abort control must disappear"
        );
    }
}
