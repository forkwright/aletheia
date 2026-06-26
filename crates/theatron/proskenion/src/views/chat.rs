//! Chat view: session tabs, virtualized message list, streaming indicator,
//! command palette, distillation indicator, and input bar.
//!
//! Virtual scrolling uses the shared [`skeue::virtual_list`] utilities.
//! The streaming typing cursor blinks via the `cursor-blink` CSS animation defined
//! in `assets/styles/base.css`.

use std::time::{Duration, Instant};

use dioxus::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::api::client::authenticated_streaming_client;
use crate::components::chat::{
    ChatMessage as LegacyChatMessage, ChatState, ChatStateManager, MessageRole,
};
use crate::components::command_palette::CommandPaletteView;
use crate::components::distillation::DistillationIndicatorView;
use crate::components::input_bar::InputBar;
use crate::components::markdown::Markdown;
use crate::components::message::{MessageBubble, should_group};
use crate::components::planning_card::PlanningCard;
use crate::components::routing_indicator::{RoutingIndicator, update_routing_stage};
use crate::components::session_tabs::SessionTabsView;
use crate::components::tool_panel::ToolPanel;
use crate::services::file_watcher::{self, FileChangeTracker};
use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::chat::{ChatMessage, ChatSelection};
use crate::state::commands::CommandStore;
use crate::state::connection::ConnectionConfig;
use crate::state::input::InputState;
use crate::state::pipeline::{PipelineStage, RoutingState};
use crate::state::platform::WindowState;
use crate::state::toasts::{ToastSeverity, ToastStore};
use crate::state::view_preservation::{PreservedViewState, ViewKey, ViewPreservationStore};
use crate::views::chat_helpers::{format_tool_call, render_approval};
use crate::views::chat_selection::activate_chat_selection;

/// Estimated message height in pixels for virtual scroll calculations.
const ESTIMATED_MSG_HEIGHT: f64 = 80.0;

/// Number of messages to load initially and per pagination chunk.
const PAGE_SIZE: usize = 100;

/// Scroll threshold in pixels from the top to trigger loading older messages.
const LOAD_MORE_THRESHOLD: f64 = 200.0;

/// Chat view with virtualized scrolling, markdown rendering, and agent switching.
#[component]
pub(crate) fn Chat() -> Element {
    let mut legacy_state = use_signal(ChatState::default);
    let mut input_state = use_signal(InputState::default);
    let mut cancel_token = use_signal(CancellationToken::new);
    let mut palette_open = use_signal(|| false);
    let config: Signal<ConnectionConfig> = use_context();
    let cmd_store = use_context::<Signal<CommandStore>>();
    let mut agent_store = use_context::<Signal<AgentStore>>();
    let mut tab_bar = use_context::<Signal<TabBar>>();
    let mut routing_signal = use_context::<Signal<Option<RoutingState>>>();
    let mut pending_chat_selection = use_context::<Signal<Option<ChatSelection>>>();
    let mut window_state = use_context::<Signal<WindowState>>();

    // WHY: Track last user message to enable retry on stream failure.
    let mut last_user_message = use_signal(String::new);
    // WHY: Track stream start time for elapsed-time indicator and timeout
    // escalation messages (30s "taking longer", 5m "abort and retry").
    let mut stream_start_time = use_signal(|| None::<Instant>);
    // WHY: Ticking signal drives elapsed-time re-renders every second
    // during streaming without polling the DOM.
    let mut elapsed_tick = use_signal(|| 0u64);

    // WHY: Paginate message history so only the most recent PAGE_SIZE
    // messages are projected into ChatMessage structs. Scrolling up past
    // the LOAD_MORE_THRESHOLD loads the next page (#3321).
    let mut loaded_page_count = use_signal(|| 1_usize);

    let mut scroll_top = use_signal(|| 0.0_f64);
    let mut container_height = use_signal(|| 600.0_f64);

    // WHY: Restore preserved view state on mount. Context switches cost
    // ~23 minutes to recover from (#2411). Preserving scroll position and
    // input drafts eliminates the UI-imposed context tax.
    let mut preservation = use_context::<Signal<ViewPreservationStore>>();
    use_hook(|| {
        if let Some(saved) = preservation.write().restore(&ViewKey::Chat) {
            scroll_top.set(saved.scroll_top);
            input_state.write().text = saved.input_text;
        }
    });

    // WHY: Save view state on unmount so it survives route changes.
    use_drop(move || {
        preservation.write().save(
            ViewKey::Chat,
            PreservedViewState {
                scroll_top: scroll_top(),
                input_text: input_state.read().text.clone(),
                secondary_scroll: 0.0,
            },
        );
    });

    use_effect(move || {
        let selection = pending_chat_selection.read().clone();
        let Some(selection) = selection else {
            return;
        };

        activate_chat_selection(
            &selection,
            &mut legacy_state.write(),
            &mut agent_store.write(),
            &mut tab_bar.write(),
            &mut window_state.write(),
        );
        pending_chat_selection.set(None);
    });

    let active_nous_id = agent_store.read().active_id.clone();

    let is_streaming = legacy_state.read().streaming.is_streaming;

    // WHY: Drive elapsed-time re-renders every second during streaming.
    // The tick signal forces the streaming indicator to re-render with
    // updated elapsed time without polling the DOM.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if stream_start_time.read().is_some() {
                elapsed_tick.set(elapsed_tick() + 1);
            }
        }
    });

    // WHY: Use the centralized projection method to convert legacy ChatState
    // messages into render-ready ChatMessage structs. Only the most recent
    // loaded_page_count * PAGE_SIZE messages are projected (#3321, #3323).
    let total_message_count = legacy_state.read().messages.len();
    let loaded_limit = loaded_page_count() * PAGE_SIZE;
    let has_more_history = total_message_count > loaded_limit;
    let messages: Vec<ChatMessage> = legacy_state.read().project_messages(Some(loaded_limit));

    let total_messages = messages.len();
    let (range_start, range_end) = visible_range(
        scroll_top(),
        container_height(),
        total_messages,
        ESTIMATED_MSG_HEIGHT,
        skeue::DEFAULT_OVERSCAN,
    );
    let (pad_top, pad_bottom) =
        skeue::spacer_heights(range_start, range_end, total_messages, ESTIMATED_MSG_HEIGHT);

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

    let mut send_message = move |text: String, is_retry: bool| {
        if text.is_empty() || is_streaming {
            return;
        }

        // WHY: Guard against no agent selected -- don't silently send to "default".
        if agent_store.read().active_id.is_none() {
            if let Some(mut toast_store) = try_consume_context::<Signal<ToastStore>>() {
                toast_store.write().push(
                    ToastSeverity::Warning,
                    "Select an agent first \u{2014} pick one in the sidebar",
                );
            }
            return;
        }

        // WHY: Set streaming flag BEFORE spawning to prevent double-submit race.
        // Without this, rapid Ctrl+Enter could spawn two concurrent tasks.
        legacy_state.write().streaming.is_streaming = true;

        // WHY: Slash commands beginning with `/` are intercepted here so the
        // palette can handle them. Unrecognised commands get a toast warning
        // so the operator knows the input was not silently eaten.
        if let Some(command_text) = text.strip_prefix('/') {
            let cmd_name = command_text.split_whitespace().next().unwrap_or("");
            let known = cmd_store.read().filtered.iter().any(|c| c.name == cmd_name);
            if !known && let Some(mut toast_store) = try_consume_context::<Signal<ToastStore>>() {
                toast_store.write().push(
                    ToastSeverity::Warning,
                    format!("Unknown command: /{cmd_name}"),
                );
            }
            palette_open.set(false);
            legacy_state.write().streaming.is_streaming = false;
            return;
        }

        // WHY: Clear any previous error so the retry banner disappears
        // when the user sends a new message.
        legacy_state.write().streaming.error = None;

        last_user_message.set(text.clone());
        stream_start_time.set(Some(Instant::now()));
        elapsed_tick.set(0);

        // WHY: A retry re-sends the failed turn in place. The original user
        // bubble is already the last history entry, so appending again would
        // render the same message twice (or more on repeated retries).
        let already_last = is_retry && legacy_state.read().last_is_user_message(&text);
        if !already_last {
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
        }

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
            let client = authenticated_streaming_client(&cfg);

            // WHY: Use agent_store.active_id (set by topbar pill clicks) instead
            // of legacy_state.agent_id (which is always None). Without this,
            // the server returns 404 because there's no agent named "default".
            let nous_id = agent_store
                .read()
                .active_id
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_else(|| "default".to_string());
            let Some(session_key) = legacy_state.read().session_key.clone() else {
                tracing::warn!("no session key available — aborting send");
                return;
            };

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

            // WHY: Derive agent display name for the routing indicator.
            // Resolve once at turn start to avoid repeated agent store reads.
            let routing_agent_name = {
                let store = agent_store.read();
                store
                    .get(&skene::id::NousId::from(nous_id.as_str()))
                    .map(|r| r.display_name().to_string())
                    .unwrap_or_else(|| nous_id.clone())
            };
            let routing_agent_id = skene::id::NousId::from(nous_id.as_str());

            update_routing_stage(
                &mut routing_signal,
                PipelineStage::Bootstrap,
                &routing_agent_name,
                &routing_agent_id,
            );

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
                        if manager.tick(&mut state) {
                            tracing::trace!("flushed buffered chat stream state");
                        }
                        continue;
                    }
                };

                let Some(event) = event else { break };

                // NOTE: Check for file change events and emit toast notifications.
                if let Some(change) = file_tracker.process(&event)
                    && let Some(mut store) = try_consume_context::<Signal<ToastStore>>()
                {
                    let title = file_watcher::toast_title(&change.kind);
                    let body = file_watcher::truncate_path(&change.path, 60);
                    let action_id = format!("open_diff:{}", change.path);
                    store.write().push_full(
                        ToastSeverity::Info,
                        title.to_string(),
                        Some(body),
                        Some(crate::state::toasts::ToastAction {
                            label: "Open".to_string(),
                            action_id,
                        }),
                    );
                }

                // WHY: Update routing indicator stage from stream events.
                // This gives the operator real-time visibility into what
                // the pipeline is doing (#2411 transparent routing).
                use skene::events::StreamEvent;
                let new_stage = match &event {
                    StreamEvent::TurnStart { .. } => Some(PipelineStage::Recalling),
                    StreamEvent::TextDelta(_) => Some(PipelineStage::Thinking),
                    StreamEvent::ThinkingDelta(_) => Some(PipelineStage::Thinking),
                    StreamEvent::ToolStart { tool_name, .. } => Some(PipelineStage::Executing {
                        tool_name: tool_name.clone(),
                    }),
                    StreamEvent::ToolResult { .. } => Some(PipelineStage::Thinking),
                    StreamEvent::TurnComplete { .. } => Some(PipelineStage::Complete),
                    StreamEvent::TurnAbort { .. } => Some(PipelineStage::Idle),
                    StreamEvent::Error(_) => Some(PipelineStage::Idle),
                    _ => None,
                };
                if let Some(stage) = new_stage {
                    update_routing_stage(
                        &mut routing_signal,
                        stage,
                        &routing_agent_name,
                        &routing_agent_id,
                    );
                }

                let mut state = legacy_state.write();
                if manager.apply(event, &mut state) {
                    tracing::trace!("applied chat stream event");
                }
            }

            // WHY: Clear stream start so the elapsed timer stops.
            stream_start_time.set(None);

            // WHY: After streaming completes, transition to Idle after a
            // brief delay so the operator sees "done" before it disappears.
            // 2-second visibility matches the toast auto-dismiss timing.
            tokio::time::sleep(Duration::from_secs(2)).await;
            update_routing_stage(
                &mut routing_signal,
                PipelineStage::Idle,
                &routing_agent_name,
                &routing_agent_id,
            );
        });
    };

    let on_submit = move |text: String| send_message(text, false);

    let on_abort = move |()| {
        cancel_token.read().cancel();
    };

    // WHY: Retry re-sends the last user message in place after clearing the
    // error -- `is_retry` suppresses the duplicate user bubble. This is a
    // separate closure so it can be used in the error banner without
    // interfering with the InputBar's on_submit prop.
    let on_retry = move |_| {
        let msg = last_user_message.read().clone();
        if !msg.is_empty() {
            legacy_state.write().streaming.error = None;
            send_message(msg, true);
        }
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
                div {
                    style: "
                        flex: 1;
                        overflow-y: auto;
                        overflow-x: hidden;
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
                                        // WHY: Load older messages when the user scrolls
                                        // near the top of the viewport (#3321).
                                        if top < LOAD_MORE_THRESHOLD && has_more_history {
                                            loaded_page_count.set(loaded_page_count() + 1);
                                        }
                                    }
                                    if let Some(h) = parsed.get("height").and_then(|v| v.as_f64())
                                        && h > 0.0
                                    {
                                        container_height.set(h);
                                    }
                                }
                            }
                        });
                    },
                    "data-chat-scroll": "true",

                    div {
                        style: "height: {pad_top}px;",
                    }

                    if has_more_history {
                        div {
                            style: "\
                                text-align: center; \
                                padding: var(--space-2); \
                                color: var(--text-muted); \
                                font-size: var(--text-xs); \
                                cursor: pointer;\
                            ",
                            onclick: move |_| {
                                loaded_page_count.set(loaded_page_count() + 1);
                            },
                            "Scroll up or click to load older messages ({total_message_count} total)"
                        }
                    }

                    for (idx , msg , grouped) in visible_messages {
                        MessageBubble {
                            key: "{idx}",
                            message: msg,
                            is_grouped: grouped,
                            agent_name: None,
                        }
                    }

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
                                        // Typing cursor -- blinks via CSS animation while streaming.
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
                                            {
                                                // WHY: Read elapsed_tick to subscribe to
                                                // re-renders, then compute actual elapsed
                                                // from the Instant for accuracy.
                                                std::hint::black_box(elapsed_tick());
                                                // WHY: Surface the live pipeline phase instead
                                                // of a bare "thinking" so the operator can see
                                                // what the turn is actually doing.
                                                let phase = match routing_signal.read().as_ref().map(|r| r.stage.clone()) {
                                                    Some(PipelineStage::Bootstrap) => "assembling context\u{2026}".to_string(),
                                                    Some(PipelineStage::Recalling) => "recalling memories\u{2026}".to_string(),
                                                    Some(PipelineStage::Executing { tool_name }) => {
                                                        format!("using {tool_name}\u{2026}")
                                                    }
                                                    Some(PipelineStage::Thinking) => {
                                                        if legacy_state.read().streaming.thinking.is_empty() {
                                                            "writing\u{2026}".to_string()
                                                        } else {
                                                            "thinking\u{2026}".to_string()
                                                        }
                                                    }
                                                    _ => "thinking\u{2026}".to_string(),
                                                };
                                                match stream_start_time.read().as_ref() {
                                                    Some(start) => {
                                                        let secs = start.elapsed().as_secs();
                                                        format!("{phase} ({secs}s)")
                                                    }
                                                    None => phase,
                                                }
                                            }
                                        }
                                    }
                                    // WHY: Escalating timeout messages give the operator
                                    // actionable feedback when streaming takes unexpectedly long.
                                    {
                                        std::hint::black_box(elapsed_tick());
                                        let elapsed_secs = stream_start_time
                                            .read()
                                            .as_ref()
                                            .map(|s| s.elapsed().as_secs())
                                            .unwrap_or(0);
                                        if elapsed_secs >= 300 {
                                            rsx! {
                                                div {
                                                    style: "
                                                        color: var(--status-warning);
                                                        font-size: var(--text-xs);
                                                        margin-top: var(--space-2);
                                                        display: flex;
                                                        align-items: center;
                                                        gap: var(--space-2);
                                                    ",
                                                    span { "This is taking a while. You can abort and retry." }
                                                    button {
                                                        style: "
                                                            background: var(--status-warning);
                                                            color: var(--text-inverse);
                                                            border: none;
                                                            border-radius: var(--radius-md);
                                                            padding: var(--space-1) var(--space-3);
                                                            cursor: pointer;
                                                            font-size: var(--text-xs);
                                                            font-weight: var(--weight-semibold);
                                                            transition: background-color var(--transition-quick);
                                                        ",
                                                        onclick: move |_| {
                                                            cancel_token.read().cancel();
                                                        },
                                                        "Abort"
                                                    }
                                                }
                                            }
                                        } else if elapsed_secs >= 30 {
                                            rsx! {
                                                div {
                                                    style: "
                                                        color: var(--text-muted);
                                                        font-size: var(--text-xs);
                                                        font-style: italic;
                                                        margin-top: var(--space-2);
                                                    ",
                                                    "Taking longer than usual..."
                                                }
                                            }
                                        } else {
                                            rsx! {}
                                        }
                                    }
                                    for detail in legacy_state.read().streaming.tool_call_details.iter() {
                                        ToolPanel { tool: detail.clone() }
                                    }
                                    for approval in legacy_state.read().streaming.approvals.iter() {
                                        if !approval.resolved {
                                            {render_approval(approval.clone(), legacy_state)}
                                        }
                                    }
                                    for plan in legacy_state.read().streaming.plans.iter() {
                                        PlanningCard { plan: plan.clone() }
                                    }
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

                    div {
                        style: "height: {pad_bottom}px;",
                    }
                }
            }

            if let Some(ref nous_id) = active_nous_id {
                DistillationIndicatorView { nous_id: nous_id.clone() }
            }

            // WHY: Transparent routing indicator shows pipeline stage
            // so the operator always knows what the system is doing (#2411).
            RoutingIndicator {}

            CommandPaletteView {
                is_open: *palette_open.read(),
                on_execute: move |_cmd: String| {
                    palette_open.set(false);
                    // NOTE: Command execution feeds back into the input bar.
                },
            }

            // WHY: Error banner above input bar gives the operator immediate
            // visibility into stream failures with a one-click retry path.
            if let Some(err) = legacy_state.read().streaming.error.clone() {
                div {
                    style: "
                        background: var(--status-error-bg);
                        color: var(--status-error);
                        border: 1px solid var(--status-error);
                        border-radius: var(--radius-md);
                        padding: var(--space-2) var(--space-3);
                        margin: 0 var(--space-4) var(--space-2) var(--space-4);
                        display: flex;
                        align-items: center;
                        justify-content: space-between;
                        gap: var(--space-3);
                        font-size: var(--text-sm);
                    ",
                    span {
                        style: "min-width: 0; overflow-wrap: anywhere; white-space: normal;",
                        "{err}"
                    }
                    button {
                        style: "
                            background: var(--status-error);
                            color: var(--text-inverse);
                            border: none;
                            border-radius: var(--radius-md);
                            padding: var(--space-1) var(--space-3);
                            cursor: pointer;
                            transition: background-color var(--transition-quick);
                            flex-shrink: 0;
                            font-size: var(--text-sm);
                        ",
                        onclick: on_retry,
                        "Retry"
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

// NOTE: visible_range tests live in skeue.
pub(crate) use skeue::visible_range;
