//! Shared chat activation helpers for cross-view navigation.

use crate::components::chat::ChatState;
use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::chat::ChatSelection;
use crate::state::platform::WindowState;

pub(crate) fn activate_chat_selection(
    selection: &ChatSelection,
    legacy_state: &mut ChatState,
    agent_store: &mut AgentStore,
    tab_bar: &mut TabBar,
    window_state: &mut WindowState,
) {
    let session_changed = legacy_state.agent_id.as_ref() != Some(&selection.agent_id)
        || legacy_state.session_key.as_deref() != Some(selection.session_key.as_str());

    if session_changed {
        legacy_state.messages.clear();
        legacy_state.streaming = Default::default();
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
        tab_bar.active = idx;
    } else {
        let idx = tab_bar.create_for_session(
            selection.agent_id.clone(),
            selection.session_key.clone(),
            selection.title.clone(),
        );
        tab_bar.active = idx;
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use skene::api::types::Agent;
    use skene::id::NousId;

    use super::*;
    use crate::components::chat::{ChatMessage as LegacyChatMessage, MessageRole};

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

        activate_chat_selection(
            &selection(),
            &mut chat_state,
            &mut agent_store,
            &mut tab_bar,
            &mut window_state,
        );

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

        activate_chat_selection(
            &selection(),
            &mut chat_state,
            &mut agent_store,
            &mut tab_bar,
            &mut window_state,
        );

        assert_eq!(chat_state.messages.len(), 1);
        assert_eq!(tab_bar.len(), 1);
    }
}
