//! Shared chat activation helpers for cross-view navigation.

use skene::api::types::{HistoryMessage, HistoryResponse};

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

pub(crate) fn parse_history_messages(text: &str) -> Result<Vec<HistoryMessage>, String> {
    match serde_json::from_str::<HistoryResponse>(text) {
        Ok(wrapper) => Ok(wrapper.messages),
        Err(wrapper_err) => match serde_json::from_str::<Vec<HistoryMessage>>(text) {
            Ok(messages) => Ok(messages),
            Err(list_err) => Err(format!(
                "parse history response: wrapper error: {wrapper_err}; list error: {list_err}"
            )),
        },
    }
}

pub(crate) fn history_messages_to_legacy(messages: &[HistoryMessage]) -> Vec<LegacyChatMessage> {
    messages
        .iter()
        .filter_map(history_message_to_legacy)
        .collect()
}

pub(crate) fn oldest_history_seq(messages: &[HistoryMessage]) -> Option<i64> {
    messages.iter().map(|msg| msg.seq).min()
}

fn history_message_to_legacy(message: &HistoryMessage) -> Option<LegacyChatMessage> {
    let role = match message.role.as_str() {
        "user" => MessageRole::User,
        "assistant" | "system" | "tool" => MessageRole::Assistant,
        other => {
            tracing::debug!(role = other, "skipping unsupported history message role");
            return None;
        }
    };

    let tool_calls = if message.role == "tool" || message.tool_name.is_some() {
        1
    } else {
        0
    };

    Some(LegacyChatMessage {
        role,
        content: history_content_to_string(&message.content),
        model: None,
        tool_calls,
        input_tokens: 0,
        output_tokens: 0,
        thinking: None,
        tool_call_details: Vec::new(),
        plans: Vec::new(),
    })
}

fn history_content_to_string(content: &str) -> String {
    content.to_string()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use skene::api::types::Agent;
    use skene::id::{NousId, SessionId};

    use super::*;

    fn agent(id: &str) -> Agent {
        Agent {
            id: NousId::from(id),
            name: Some(id.to_string()),
            model: None,
            emoji: None,
        }
    }

    fn selection() -> ChatSelection {
        ChatSelection::new(
            NousId::from("syn"),
            "incident-review".to_string(),
            "Incident Review".to_string(),
        )
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
            agent_id: Some(NousId::from("syn")),
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
        });
        let mut agent_store = AgentStore::new();
        agent_store.load_from_api(vec![agent("syn")]);
        let mut tab_bar = TabBar::new();
        tab_bar.create_for_session(
            NousId::from("syn"),
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
            NousId::from("syn"),
            SessionId::from("session-id"),
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
            NousId::from("syn"),
            SessionId::from("session-id"),
            "incident-review".to_string(),
            "Incident Review".to_string(),
            2,
        );
        let json = r#"{
            "messages": [
                {
                    "id": 1,
                    "seq": 1,
                    "role": "user",
                    "content": "What happened?",
                    "tool_call_id": null,
                    "tool_name": null,
                    "created_at": "2025-01-01T00:00:00Z"
                },
                {
                    "id": 2,
                    "seq": 2,
                    "role": "assistant",
                    "content": "Recovered the transcript.",
                    "tool_call_id": null,
                    "tool_name": null,
                    "created_at": "2025-01-01T00:00:01Z"
                }
            ]
        }"#;

        activate_chat_selection(
            &selection,
            &mut chat_state,
            &mut agent_store,
            &mut tab_bar,
            &mut window_state,
        );
        let messages = parse_history_messages(json).unwrap();
        chat_state.messages = history_messages_to_legacy(&messages);

        assert_eq!(oldest_history_seq(&messages), Some(1));
        assert_eq!(chat_state.messages.len(), 2);
        assert_eq!(chat_state.messages[0].role, MessageRole::User);
        assert_eq!(chat_state.messages[0].content, "What happened?");
        assert_eq!(chat_state.messages[1].role, MessageRole::Assistant);
        assert_eq!(chat_state.messages[1].content, "Recovered the transcript.");
        assert!(chat_state.messages[1].model.is_none());
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
        });
        chat_state.streaming.is_streaming = true;
        chat_state.streaming.text = "partial answer".to_string();
        let mut agent_store = AgentStore::new();
        agent_store.load_from_api(vec![agent("syn")]);
        let mut tab_bar = TabBar::new();
        let mut window_state = WindowState::default();
        let selection = ChatSelection::for_existing_session(
            NousId::from("syn"),
            SessionId::from("session-id"),
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
}
