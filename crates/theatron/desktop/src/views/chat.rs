//! Chat view: message list, streaming indicator, and input box.

use std::time::Duration;

use dioxus::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::api::client::authenticated_client;
use crate::components::chat::{ChatMessage, ChatState, ChatStateManager, MessageRole};
use crate::state::connection::ConnectionConfig;

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

const INPUT_BAR_STYLE: &str = "\
    display: flex; \
    gap: 8px; \
    padding: 12px 16px; \
    background: #1a1a2e; \
    border-top: 1px solid #333;\
";

const INPUT_STYLE: &str = "\
    flex: 1; \
    background: #0f0f1a; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 10px 14px; \
    color: #e0e0e0; \
    font-size: 14px; \
    font-family: inherit;\
";

const SEND_BTN_STYLE: &str = "\
    background: #4a4aff; \
    color: white; \
    border: none; \
    border-radius: 8px; \
    padding: 10px 20px; \
    font-size: 14px; \
    cursor: pointer;\
";

const SEND_BTN_DISABLED: &str = "\
    background: #333; \
    color: #666; \
    border: none; \
    border-radius: 8px; \
    padding: 10px 20px; \
    font-size: 14px; \
    cursor: not-allowed;\
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
    let mut input_text = use_signal(String::new);
    let mut cancel_token = use_signal(CancellationToken::new);
    let config: Signal<ConnectionConfig> = use_context();

    let is_streaming = chat_state.read().streaming.is_streaming;

    let mut do_submit = move || {
        let text = input_text.read().trim().to_string();
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
        });
        input_text.set(String::new());

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

            loop {
                let event = tokio::select! {
                    biased;
                    _ = new_token.cancelled() => break,
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

            div {
                style: "{INPUT_BAR_STYLE}",
                input {
                    style: "{INPUT_STYLE}",
                    r#type: "text",
                    placeholder: "Type a message...",
                    value: "{input_text}",
                    disabled: is_streaming,
                    oninput: move |evt: Event<FormData>| {
                        input_text.set(evt.value().clone());
                    },
                    onkeypress: move |evt: Event<KeyboardData>| {
                        if evt.key() == Key::Enter {
                            do_submit();
                        }
                    },
                }
                button {
                    style: if is_streaming { "{SEND_BTN_DISABLED}" } else { "{SEND_BTN_STYLE}" },
                    disabled: is_streaming,
                    onclick: move |_| do_submit(),
                    if is_streaming { "..." } else { "Send" }
                }
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

    rsx! {
        div {
            key: "{key}",
            style: "{style}",
            "{msg.content}"
            if let Some(meta_text) = meta {
                div { style: "{META_STYLE}", "{meta_text}" }
            }
        }
    }
}

fn render_streaming(state: &ChatState) -> Element {
    rsx! {
        div {
            style: "{STREAMING_STYLE}",
            if !state.streaming.text.is_empty() {
                "{state.streaming.text}"
            } else {
                span { style: "color: #4a4aff;", "Thinking..." }
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
        let marker = if tc.is_error { "[x]" } else { "[v]" }; // kanon:ignore RUST/indexing-slicing
        match tc.duration_ms {
            Some(ms) => format!("{marker} {} ({ms}ms)", tc.tool_name),
            None => format!("{marker} {}", tc.tool_name),
        }
    } else {
        format!("[...] {}", tc.tool_name) // kanon:ignore RUST/string-slice
    }
}
