//! Chat view: session tabs, virtualized message list, streaming indicator,
//! command palette, distillation indicator, and input bar.
//!
//! Virtual scrolling uses the shared [`crate::components::virtual_list`] utilities.
//! The streaming typing cursor blinks via the `cursor-blink` CSS animation defined
//! in `assets/styles/base.css`.

use std::time::Duration;

use dioxus::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::api::client::authenticated_client;
use crate::components::chat::{
    ChatMessage as LegacyChatMessage, ChatState, ChatStateManager, MessageRole,
};
use crate::components::command_palette::CommandPaletteView;
use crate::components::distillation::DistillationIndicatorView;
use crate::components::input_bar::InputBar;
use crate::components::markdown::Markdown;
use crate::components::message::{MessageBubble, should_group};
use crate::components::planning_card::PlanningCard;
use crate::components::session_tabs::SessionTabsView;
use crate::components::tool_approval::ToolApproval;
use crate::components::tool_panel::ToolPanel;
use crate::services::file_watcher::{self, FileChangeTracker};
use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::chat::{ChatMessage, ChatStore, Role};
use crate::state::commands::CommandStore;
use crate::state::connection::ConnectionConfig;
use crate::state::input::InputState;
use crate::state::toasts::{Severity, ToastStore};

/// Estimated message height in pixels for virtual scroll calculations.
const ESTIMATED_MSG_HEIGHT: f64 = 80.0;

/// Chat view with virtualized scrolling, markdown rendering, and agent switching.
#[component]
pub(crate) fn Chat() -> Element {
    let mut legacy_state = use_signal(ChatState::default);
    let _store = use_signal(ChatStore::default);
    let input_state = use_signal(InputState::default);
    let mut cancel_token = use_signal(CancellationToken::new);
    let mut palette_open = use_signal(|| false);
    let config: Signal<ConnectionConfig> = use_context();
    let mut cmd_store = use_context::<Signal<CommandStore>>();
    let agent_store = use_context::<Signal<AgentStore>>();
    let mut tab_bar = use_context::<Signal<TabBar>>();

    // Virtual scroll state
    let mut scroll_top = use_signal(|| 0.0_f64);
    let mut container_height = use_signal(|| 600.0_f64);

    // Derive the active agent ID from the agent store.
    let active_nous_id = agent_store.read().active_id.clone();

    let is_streaming = legacy_state.read().streaming.is_streaming;

    // Bridge: sync legacy ChatState messages into the new ChatStore model.
    // WHY: The existing ChatStateManager + streaming pipeline writes to
    // ChatState.messages (Vec<LegacyChatMessage>). Rather than rewriting
    // the entire streaming pipeline, we project legacy messages into
    // ChatMessage for rendering.
    let messages: Vec<ChatMessage> = {
        let state = legacy_state.read();
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        state
            .messages
            .iter()
            .enumerate()
            .map(|(i, m)| ChatMessage {
                id: i as u64 + 1,
                role: match m.role {
                    MessageRole::User => Role::User,
                    MessageRole::Assistant => Role::Assistant,
                },
                content: m.content.clone(),
                timestamp: now_ts - ((state.messages.len() - 1 - i) as i64 * 30),
                agent_id: state.agent_id.clone(),
                tool_calls: m.tool_calls,
                thinking_content: None,
                is_streaming: false,
                model: m.model.clone(),
                input_tokens: m.input_tokens,
                output_tokens: m.output_tokens,
            })
            .collect()
    };

    // Virtual scroll: compute visible range using shared utility.
    let total_messages = messages.len();
    let (range_start, range_end) = visible_range(
        scroll_top(),
        container_height(),
        total_messages,
        ESTIMATED_MSG_HEIGHT,
        crate::components::virtual_list::DEFAULT_OVERSCAN,
    );
    let (pad_top, pad_bottom) = crate::components::virtual_list::spacer_heights(
        range_start,
        range_end,
        total_messages,
        ESTIMATED_MSG_HEIGHT,
    );

    let visible_messages: Vec<(usize, ChatMessage, bool)> = messages
        .iter()
        .enumerate()
        .skip(range_start)
        .take(range_end - range_start)
        .map(|(i, msg)| {
            let grouped = if i > 0 {
                should_group(&messages[i - 1], msg)
            } else {
                false
            };
            (i, msg.clone(), grouped)
        })
        .collect();

    let on_submit = move |text: String| {
        if text.is_empty() || is_streaming {
            return;
        }

        // WHY: Slash commands beginning with `/` are intercepted here so the
        // palette can handle them. Unrecognised commands fall through to chat.
        if text.starts_with('/') {
            palette_open.set(false);
            // NOTE: Command execution wired at the application level.
            // The palette already handles known commands via on_execute.
            return;
        }

        legacy_state.write().messages.push(LegacyChatMessage {
            role: MessageRole::User,
            content: text.clone(),
            model: None,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            thinking: None,
            tool_call_details: Vec::new(),
            plans: Vec::new(),
        });

        // Register a tab for this agent if not already open.
        if let Some(ref agent_id) = agent_store.read().active_id {
            let bar = tab_bar.read();
            let already_open = bar.tabs.iter().any(|t| &t.agent_id == agent_id);
            drop(bar);
            if !already_open {
                let display = agent_store
                    .read()
                    .get(agent_id)
                    .map(|r| r.display_name().to_string())
                    .unwrap_or_else(|| agent_id.to_string());
                let idx = tab_bar.write().create(agent_id.clone(), display);
                tab_bar.write().active = idx;
            }
        }

        let cfg = config.read().clone();

        cancel_token.read().cancel();
        let new_token = CancellationToken::new();
        cancel_token.set(new_token.clone());

        spawn(async move {
            let client = authenticated_client(&cfg);

            let nous_id = legacy_state
                .read()
                .agent_id
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_else(|| "default".to_string());
            let session_key = legacy_state
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
            let mut file_tracker = FileChangeTracker::new();
            let timeout = tokio::time::sleep(Duration::from_secs(600));
            tokio::pin!(timeout);

            loop {
                let event = tokio::select! {
                    biased;
                    _ = new_token.cancelled() => break,
                    _ = &mut timeout => {
                        let mut state = legacy_state.write();
                        state.streaming.error =
                            Some("stream timed out after 10 minutes".to_string());
                        state.streaming.is_streaming = false;
                        break;
                    }
                    event = rx.recv() => event,
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        let mut state = legacy_state.write();
                        let _ = manager.tick(&mut state);
                        continue;
                    }
                };

                let Some(event) = event else { break };

                // NOTE: Check for file change events and emit toast notifications.
                if let Some(change) = file_tracker.process(&event) {
                    if let Some(mut store) = try_consume_context::<Signal<ToastStore>>() {
                        let title = file_watcher::toast_title(&change.kind);
                        let body = file_watcher::truncate_path(&change.path, 60);
                        let action_id = format!("open_diff:{}", change.path);
                        store.write().push_full(
                            Severity::Info,
                            title.to_string(),
                            Some(body),
                            Some(crate::state::toasts::ToastAction {
                                label: "Open".to_string(),
                                action_id,
                            }),
                        );
                    }
                }

                let mut state = legacy_state.write();
                let _ = manager.apply(event, &mut state);
            }
        });
    };

    let on_abort = move |()| {
        cancel_token.read().cancel();
    };

    rsx! {
        div {
            style: "
                display: flex;
                flex-direction: column;
                height: 100%;
                background: var(--bg);
                font-family: var(--font-body);
                position: relative;
            ",

            SessionTabsView {}

            if messages.is_empty() && !is_streaming {
                // Empty state
                div {
                    style: "
                        flex: 1;
                        display: flex;
                        flex-direction: column;
                        align-items: center;
                        justify-content: center;
                        gap: var(--space-4);
                        color: var(--text-muted);
                    ",
                    div {
                        style: "
                            font-family: var(--font-display);
                            font-size: var(--text-xl);
                            color: var(--text-secondary);
                        ",
                        "Start a conversation"
                    }
                    div {
                        style: "font-size: var(--text-sm);",
                        "Type a message below to begin."
                    }
                }
            } else {
                // Message list with virtual scrolling
                div {
                    style: "
                        flex: 1;
                        overflow-y: auto;
                        position: relative;
                    ",
                    onscroll: move |_evt: Event<ScrollData>| {
                        // NOTE: Dioxus desktop scroll data provides
                        // scroll_offset via the ScrollData type.
                        // We read the raw pixel values for virtual scroll.
                        // For now, track via eval for precise values.
                        let js = r#"
                            (function() {
                                var el = document.querySelector('[data-chat-scroll]');
                                if (el) return JSON.stringify({top: el.scrollTop, height: el.clientHeight});
                                return '{}';
                            })()
                        "#;
                        spawn(async move {
                            if let Ok(val) = document::eval(js).await {
                                let text = val.to_string();
                                let cleaned = text.trim_matches('"');
                                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(cleaned) {
                                    if let Some(top) = parsed.get("top").and_then(|v| v.as_f64()) {
                                        scroll_top.set(top);
                                    }
                                    if let Some(h) = parsed.get("height").and_then(|v| v.as_f64()) {
                                        if h > 0.0 {
                                            container_height.set(h);
                                        }
                                    }
                                }
                            }
                        });
                    },
                    "data-chat-scroll": "true",

                    // Virtual scroll spacer (top)
                    div {
                        style: "height: {pad_top}px;",
                    }

                    // Visible messages
                    for (idx , msg , grouped) in visible_messages {
                        MessageBubble {
                            key: "{idx}",
                            message: msg,
                            is_grouped: grouped,
                            agent_name: None,
                        }
                    }

                    // Streaming indicator
                    if is_streaming {
                        div {
                            style: "
                                padding: 0 var(--space-4);
                                margin-top: var(--space-3);
                            ",
                            div {
                                style: "
                                    display: flex;
                                    flex-direction: column;
                                    align-items: flex-start;
                                ",
                                div {
                                    style: "
                                        font-size: var(--text-xs);
                                        color: var(--role-assistant);
                                        font-weight: var(--weight-semibold);
                                        margin-bottom: var(--space-1);
                                    ",
                                    "Assistant"
                                }
                                div {
                                    style: "
                                        background: var(--bg-surface-bright);
                                        border: 1px solid var(--accent);
                                        border-radius: var(--radius-xl) var(--radius-xl) var(--radius-xl) var(--radius-sm);
                                        padding: var(--space-3) var(--space-4);
                                        max-width: 85%;
                                        color: var(--text-primary);
                                    ",
                                    if !legacy_state.read().streaming.text.is_empty() {
                                        Markdown {
                                            content: legacy_state.read().streaming.text.clone(),
                                        }
                                        // Typing cursor — blinks via CSS animation while streaming.
                                        span {
                                            class: "streaming-cursor",
                                            "aria-hidden": "true",
                                            style: "
                                                display: inline-block;
                                                width: 2px;
                                                height: 1.1em;
                                                background: var(--accent);
                                                vertical-align: text-bottom;
                                                animation: cursor-blink 1s step-end infinite;
                                                margin-left: 1px;
                                            ",
                                        }
                                    } else {
                                        div {
                                            style: "
                                                color: var(--accent);
                                                font-style: italic;
                                            ",
                                            "Thinking..."
                                        }
                                    }
                                    // Rich tool call panels (expandable)
                                    for detail in legacy_state.read().streaming.tool_call_details.iter() {
                                        ToolPanel { tool: detail.clone() }
                                    }
                                    // Tool approval dialogs
                                    for approval in legacy_state.read().streaming.approvals.iter() {
                                        if !approval.resolved {
                                            {render_approval(approval.clone(), legacy_state)}
                                        }
                                    }
                                    // Planning cards
                                    for plan in legacy_state.read().streaming.plans.iter() {
                                        PlanningCard { plan: plan.clone() }
                                    }
                                    // Active tool calls (compact)
                                    for tc in legacy_state.read().streaming.tool_calls.iter() {
                                        div {
                                            style: "
                                                font-size: var(--text-xs);
                                                color: var(--text-muted);
                                                padding: var(--space-1) var(--space-2);
                                                background: var(--bg-surface-dim);
                                                border-radius: var(--radius-md);
                                                margin-top: var(--space-1);
                                                font-family: var(--font-mono);
                                            ",
                                            "{format_tool_call(tc)}"
                                        }
                                    }
                                    if let Some(err) = &legacy_state.read().streaming.error {
                                        div {
                                            style: "
                                                color: var(--status-error);
                                                margin-top: var(--space-2);
                                                font-size: var(--text-sm);
                                            ",
                                            "Error: {err}"
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Virtual scroll spacer (bottom)
                    div {
                        style: "height: {pad_bottom}px;",
                    }
                }
            }

            if let Some(ref nous_id) = active_nous_id {
                DistillationIndicatorView { nous_id: nous_id.clone() }
            }

            CommandPaletteView {
                is_open: *palette_open.read(),
                on_execute: move |cmd: String| {
                    palette_open.set(false);
                    // NOTE: Command execution feeds back into the input bar.
                },
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

fn render_approval(
    approval: crate::state::tools::ToolApprovalState,
    _chat_signal: Signal<ChatState>,
) -> Element {
    let turn_id = approval.turn_id.to_string();
    let tool_id = approval.tool_id.to_string();
    let turn_id_deny = turn_id.clone();
    let tool_id_deny = tool_id.clone();

    // WHY: capture IDs by value for the async approval/deny calls.
    let config: Signal<ConnectionConfig> = use_context();

    rsx! {
        ToolApproval {
            approval: approval,
            on_approve: move |_| {
                let turn_id = turn_id.clone();
                let tool_id = tool_id.clone();
                let cfg = config.read().clone();
                spawn(async move {
                    let client = theatron_core::api::client::ApiClient::new(
                        &cfg.server_url,
                        cfg.auth_token.clone(),
                    );
                    if let Ok(client) = client {
                        let _ = client.approve_tool(&turn_id, &tool_id).await;
                    }
                });
            },
            on_deny: move |_| {
                let turn_id = turn_id_deny.clone();
                let tool_id = tool_id_deny.clone();
                let cfg = config.read().clone();
                spawn(async move {
                    let client = theatron_core::api::client::ApiClient::new(
                        &cfg.server_url,
                        cfg.auth_token.clone(),
                    );
                    if let Ok(client) = client {
                        let _ = client.deny_tool(&turn_id, &tool_id).await;
                    }
                });
            },
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

// NOTE: visible_range tests have moved to components::virtual_list.
pub(crate) use crate::components::virtual_list::visible_range;
