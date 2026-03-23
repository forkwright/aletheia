//! Chat view: message list, streaming indicator, thinking panels, and input bar.

use std::time::Duration;

use dioxus::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::api::client::authenticated_client;
use crate::components::chat::{ChatMessage, ChatState, ChatStateManager, MessageRole};
use crate::components::input_bar::InputBar;
use crate::components::thinking::ThinkingPanel;
use crate::state::connection::ConnectionConfig;
use crate::state::input::InputState;

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    background: #0f0f1a;\
";

const MESSAGES_STYLE: &str = "\
    flex: 1; \
    overflow-y: auto; \
    padding: 16px; \
    display: flex; \
    flex-direction: column; \
    gap: 12px;\
";

const USER_MSG_STYLE: &str = "\
    align-self: flex-end; \
    background: #2a2a4a; \
    color: #e0e0e0; \
    padding: 12px 16px; \
    border-radius: 12px 12px 4px 12px; \
    max-width: 70%; \
    white-space: pre-wrap; \
    word-wrap: break-word;\
";

const ASSISTANT_MSG_STYLE: &str = "\
    align-self: flex-start; \
    background: #1a1a2e; \
    color: #e0e0e0; \
    padding: 12px 16px; \
    border-radius: 12px 12px 12px 4px; \
    max-width: 80%; \
    white-space: pre-wrap; \
    word-wrap: break-word; \
    border: 1px solid #333;\
";

const STREAMING_STYLE: &str = "\
    align-self: flex-start; \
    background: #1a1a2e; \
    color: #aaa; \
    padding: 12px 16px; \
    border-radius: 12px 12px 12px 4px; \
    max-width: 80%; \
    white-space: pre-wrap; \
    word-wrap: break-word; \
    border: 1px solid #4a4aff;\
";

const TOOL_CALL_STYLE: &str = "\
    font-size: 12px; \
    color: #888; \
    padding: 4px 8px; \
    background: #1a1a30; \
    border-radius: 4px; \
    margin-top: 4px;\
";

const META_STYLE: &str = "\
    font-size: 11px; \
    color: #666; \
    margin-top: 4px;\
";

const EMPTY_STYLE: &str = "\
    flex: 1; \
    display: flex; \
    align-items: center; \
    justify-content: center; \
    color: #555; \
    font-size: 16px;\
";

#[component]
pub(crate) fn Chat() -> Element {
    let mut chat_state = use_signal(ChatState::default);
    let input_state = use_signal(InputState::default);
    let mut cancel_token = use_signal(CancellationToken::new);
    let config: Signal<ConnectionConfig> = use_context();

    let is_streaming = chat_state.read().streaming.is_streaming;

    let on_submit = move |text: String| {
        if text.is_empty() || is_streaming {
            return;
        }

        chat_state.write().messages.push(ChatMessage {
            role: MessageRole::User,
            content: text.clone(),
            model: None,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            thinking: None,
        });

        let cfg = config.read().clone();

        // WHY: Cancel the previous token before creating a new one so any
        // lingering stream from a prior turn is torn down.
        cancel_token.read().cancel();
        let new_token = CancellationToken::new();
        cancel_token.set(new_token.clone());

        spawn(async move {
            let client = authenticated_client(&cfg);

            let nous_id = chat_state
                .read()
                .agent_id
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_else(|| "default".to_string());
            let session_key = chat_state
                .read()
                .session_key
                .clone()
                .unwrap_or_else(|| "desktop".to_string());

            let mut rx = crate::api::streaming::stream_turn(
                client,
                &cfg.server_url,
                &nous_id,
                &session_key,
                &text,
                new_token.clone(),
            );

            let mut manager = ChatStateManager::new();
            let timeout = tokio::time::sleep(Duration::from_secs(600));
            tokio::pin!(timeout);

            loop {
                let event = tokio::select! {
                    biased;
                    _ = new_token.cancelled() => break,
                    _ = &mut timeout => {
                        let mut state = chat_state.write();
                        state.streaming.error =
                            Some("stream timed out after 10 minutes".to_string());
                        state.streaming.is_streaming = false;
                        break;
                    }
                    event = rx.recv() => event,
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        let mut state = chat_state.write();
                        let _ = manager.tick(&mut state);
                        continue;
                    }
                };

                let Some(event) = event else { break };
                let mut state = chat_state.write();
                let _ = manager.apply(event, &mut state);
            }
        });
    };

    let on_abort = move |()| {
        cancel_token.read().cancel();
    };

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

            if chat_state.read().messages.is_empty() && !is_streaming {
                div {
                    style: "{EMPTY_STYLE}",
                    "Start a conversation"
                }
            } else {
                div {
                    style: "{MESSAGES_STYLE}",
                    for (i , msg) in chat_state.read().messages.iter().enumerate() {
                        {render_message(msg, i)}
                    }
                    if is_streaming {
                        {render_streaming(&chat_state.read())}
                    }
                }
            }

            InputBar {
                input: input_state,
                is_streaming: is_streaming,
                on_submit: on_submit,
                on_abort: on_abort,
            }
        }
    }
}

fn render_message(msg: &ChatMessage, key: usize) -> Element {
    let style = match msg.role {
        MessageRole::User => USER_MSG_STYLE,
        MessageRole::Assistant => ASSISTANT_MSG_STYLE,
    };

    let meta = if msg.role == MessageRole::Assistant {
        msg.model.as_ref().map(|model| {
            let mut s = format!("{model} | {}in/{}out", msg.input_tokens, msg.output_tokens);
            if msg.tool_calls > 0 {
                s.push_str(&format!(" | {} tool calls", msg.tool_calls));
            }
            s
        })
    } else {
        None
    };

    let thinking_content = msg.thinking.clone().unwrap_or_default();

    rsx! {
        div {
            key: "{key}",
            style: "{style}",
            "{msg.content}"
            if !thinking_content.is_empty() {
                ThinkingPanel {
                    content: thinking_content,
                    is_streaming: false,
                }
            }
            if let Some(meta_text) = meta {
                div { style: "{META_STYLE}", "{meta_text}" }
            }
        }
    }
}

fn render_streaming(state: &ChatState) -> Element {
    let has_thinking = !state.streaming.thinking.is_empty();

    rsx! {
        div {
            style: "{STREAMING_STYLE}",
            if !state.streaming.text.is_empty() {
                "{state.streaming.text}"
            } else if !has_thinking {
                span { style: "color: #4a4aff;", "Thinking..." }
            }
            if has_thinking {
                ThinkingPanel {
                    content: state.streaming.thinking.clone(),
                    is_streaming: true,
                }
            }
            for tc in &state.streaming.tool_calls {
                div {
                    style: "{TOOL_CALL_STYLE}",
                    "{format_tool_call(tc)}"
                }
            }
            if let Some(err) = &state.streaming.error {
                div { style: "color: #ef4444; margin-top: 8px;", "Error: {err}" }
            }
        }
    }
}

fn format_tool_call(tc: &crate::state::events::ToolCallInfo) -> String {
    if tc.completed {
        let marker = if tc.is_error { "[x]" } else { "[v]" };
        match tc.duration_ms {
            Some(ms) => format!("{marker} {} ({ms}ms)", tc.tool_name),
            None => format!("{marker} {}", tc.tool_name),
        }
    } else {
        format!("[...] {}", tc.tool_name)
    }
}
