//! Chat view: session tabs, virtualized message list, streaming indicator,
//! command palette, distillation indicator, and input bar.
//!
//! Virtual scrolling uses the shared [`skeue::virtual_list`] utilities.
//! The streaming typing cursor blinks via the `cursor-blink` CSS animation defined
//! in `assets/styles/base.css`.

use std::time::{Duration, Instant};

use dioxus::prelude::*;
use skene::events::StreamEvent;
use themelion::ThemeMode;
use tokio_util::sync::CancellationToken;

use crate::api::client::authenticated_streaming_client;
use crate::app::Route;
use crate::components::chat::{
    ChatMessage as LegacyChatMessage, ChatState, ChatStateManager, MessageRole, TurnEndKind,
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
use crate::services::export::messages_to_markdown;
use crate::services::file_watcher::{self, FileChangeTracker};
use crate::state::agents::AgentStore;
use crate::state::app::TabBar;
use crate::state::chat::{ChatMessage, ChatSelection};
use crate::state::commands::{
    CommandAction, CommandDestination, CommandExecutionState, CommandResolution, CommandStore,
    CommandUiState,
};
use crate::state::composer_queue::{ComposerQueue, dequeue_after_turn_end, enqueue_if_streaming};
use crate::state::connection::{ConnectionConfig, ConnectionState};
use crate::state::events::StreamingState;
use crate::state::input::InputState;
use crate::state::pipeline::{PipelineStage, RoutingState};
use crate::state::platform::WindowState;
use crate::state::settings::{AppearanceSettings, KeybindingStore, ServerConfigStore};
use crate::state::toasts::{ToastSeverity, ToastStore};
use crate::state::view_preservation::{PreservedViewState, ViewKey, ViewPreservationStore};
use crate::views::chat_helpers::{format_tool_call, render_approval};
use crate::views::chat_selection::{
    activate_chat_selection, apply_active_turn_reattachment, canonical_agent_selection,
    history_messages_to_legacy, oldest_history_seq, resolve_chat_session_key,
};

/// Estimated message height in pixels for virtual scroll calculations.
const ESTIMATED_MSG_HEIGHT: f64 = 80.0;

/// Number of messages to load initially and per pagination chunk.
const PAGE_SIZE: usize = 100;

/// Server-side page size for chat history fetches.
const HISTORY_PAGE_SIZE_QUERY: u32 = 100;

/// Scroll threshold in pixels from the top to trigger loading older messages.
const LOAD_MORE_THRESHOLD: f64 = 200.0;

/// Outer UI guard for a turn stream that has not reached a terminal event.
const UI_STREAM_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChatHistoryStatus {
    Idle,
    LoadingInitial,
    LoadingOlder,
    Loaded,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatHistoryState {
    status: ChatHistoryStatus,
    total_count: Option<usize>,
    oldest_seq: Option<i64>,
    /// Count of raw server history rows fetched so far, across all pages.
    ///
    /// WHY(#7298): distinct from the *rendered* message count once history
    /// replay collapses a turn's repeated tool-call rows into one summary
    /// row per tool type -- comparing `total_count` (a raw server count)
    /// against the collapsed, smaller rendered count would report more
    /// history as always available, even once every page was loaded.
    raw_fetched: usize,
}

impl Default for ChatHistoryState {
    fn default() -> Self {
        Self {
            status: ChatHistoryStatus::Idle,
            total_count: None,
            oldest_seq: None,
            raw_fetched: 0,
        }
    }
}

impl ChatHistoryState {
    fn loading_initial(total_count: Option<usize>) -> Self {
        Self {
            status: ChatHistoryStatus::LoadingInitial,
            total_count,
            oldest_seq: None,
            raw_fetched: 0,
        }
    }

    fn loading_older(&self) -> Self {
        Self {
            status: ChatHistoryStatus::LoadingOlder,
            total_count: self.total_count,
            oldest_seq: self.oldest_seq,
            raw_fetched: self.raw_fetched,
        }
    }

    fn loaded(total_count: Option<usize>, oldest_seq: Option<i64>, raw_fetched: usize) -> Self {
        Self {
            status: ChatHistoryStatus::Loaded,
            total_count,
            oldest_seq,
            raw_fetched,
        }
    }

    fn failed(
        message: String,
        total_count: Option<usize>,
        oldest_seq: Option<i64>,
        raw_fetched: usize,
    ) -> Self {
        Self {
            status: ChatHistoryStatus::Error(message),
            total_count,
            oldest_seq,
            raw_fetched,
        }
    }

    fn is_loading(&self) -> bool {
        matches!(
            self.status,
            ChatHistoryStatus::LoadingInitial | ChatHistoryStatus::LoadingOlder
        )
    }

    fn is_initial_loading(&self) -> bool {
        self.status == ChatHistoryStatus::LoadingInitial
    }

    fn is_loading_older(&self) -> bool {
        self.status == ChatHistoryStatus::LoadingOlder
    }

    fn is_loaded(&self) -> bool {
        self.status == ChatHistoryStatus::Loaded
    }

    fn error(&self) -> Option<&str> {
        match &self.status {
            ChatHistoryStatus::Error(message) => Some(message),
            _ => None,
        }
    }

    fn has_older_server_history(&self) -> bool {
        self.oldest_seq.is_some() && self.total_count.is_some_and(|total| total > self.raw_fetched)
    }
}

fn history_total_count(selection: &ChatSelection) -> Option<usize> {
    selection
        .message_count
        .and_then(|count| usize::try_from(count).ok())
}

fn active_session_matches(state: &ChatState, selection: &ChatSelection) -> bool {
    state.agent_id.as_ref() == Some(&selection.agent_id)
        && state.session_key.as_deref() == Some(selection.session_key.as_str())
}

fn route_for_command_destination(destination: CommandDestination) -> Route {
    match destination {
        CommandDestination::Chat => Route::Chat {},
        CommandDestination::Files => Route::Files {},
        CommandDestination::Planning => Route::Planning {},
        CommandDestination::Memory => Route::Memory {},
        CommandDestination::Metrics => Route::Metrics {},
        CommandDestination::Ops => Route::Ops {},
        CommandDestination::Sessions => Route::Sessions {},
        CommandDestination::Settings => Route::Settings {},
    }
}

fn command_destination_label(destination: CommandDestination) -> &'static str {
    match destination {
        CommandDestination::Chat => "Chat",
        CommandDestination::Files => "Theke",
        CommandDestination::Planning => "Planning",
        CommandDestination::Memory => "Memory",
        CommandDestination::Metrics => "Metrics",
        CommandDestination::Ops => "Ops",
        CommandDestination::Sessions => "Sessions",
        CommandDestination::Settings => "Settings",
    }
}

fn command_state_toast(state: &CommandExecutionState) -> (ToastSeverity, String) {
    match state {
        CommandExecutionState::Succeeded { message, .. } => {
            (ToastSeverity::Success, message.clone())
        }
        CommandExecutionState::Failed { message, .. } => (ToastSeverity::Error, message.clone()),
        CommandExecutionState::Disabled { name, reason } => (
            ToastSeverity::Warning,
            format!("/{name} is disabled: {reason}"),
        ),
        CommandExecutionState::Unknown { name } => {
            (ToastSeverity::Warning, format!("Unknown command: /{name}"))
        }
    }
}

fn push_command_toast(severity: ToastSeverity, message: impl Into<String>) {
    if let Some(mut toast_store) = try_consume_context::<Signal<ToastStore>>() {
        toast_store.write().push(severity, message.into());
    }
}

#[derive(Clone, Copy)]
struct CommandRuntime {
    command_ui: Signal<CommandUiState>,
    cmd_store: Signal<CommandStore>,
    legacy_state: Signal<ChatState>,
    connection_state: Signal<ConnectionState>,
    theme_mode: Signal<ThemeMode>,
    appearance: Signal<AppearanceSettings>,
    server_store: Signal<ServerConfigStore>,
    keybindings: Signal<KeybindingStore>,
    nav: dioxus_router::Navigator,
}

fn execute_command_input(raw: String, runtime: CommandRuntime) {
    let mut command_ui = runtime.command_ui;
    let mut cmd_store = runtime.cmd_store;
    let mut legacy_state = runtime.legacy_state;
    let mut connection_state = runtime.connection_state;
    let mut theme_mode = runtime.theme_mode;
    let mut appearance = runtime.appearance;
    let server_store = runtime.server_store;
    let keybindings = runtime.keybindings;
    let nav = runtime.nav;

    command_ui.write().palette_open = false;

    let resolution = cmd_store.write().resolve_slash(&raw);
    let invocation = match resolution {
        CommandResolution::Ready(invocation) => invocation,
        CommandResolution::Rejected(state) => {
            let (severity, message) = command_state_toast(&state);
            push_command_toast(severity, message);
            return;
        }
    };

    let name = invocation.command.name.clone();
    match invocation.command.action.clone() {
        CommandAction::ShowHelp => {
            command_ui.write().help_visible = true;
            let message = "Opened keyboard shortcuts";
            cmd_store.write().record_success(&name, message);
            push_command_toast(ToastSeverity::Success, message);
        }
        CommandAction::ClearChat => {
            legacy_state.write().messages.clear();
            let message = "Cleared visible chat history";
            cmd_store.write().record_success(&name, message);
            push_command_toast(ToastSeverity::Success, message);
        }
        CommandAction::ToggleTheme => {
            let next = if *theme_mode.read() == ThemeMode::Dark {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            };
            theme_mode.set(next);
            appearance.write().theme = next.slug().to_string();
            crate::services::settings_config::save_state(
                &server_store.read(),
                &appearance.read(),
                &keybindings.read(),
            );
            let message = format!("Theme set to {}", next.label());
            cmd_store.write().record_success(&name, &message);
            push_command_toast(ToastSeverity::Success, message);
        }
        CommandAction::Disconnect => {
            let message = "Disconnected from server";
            cmd_store.write().record_success(&name, message);
            push_command_toast(ToastSeverity::Success, message);
            connection_state.set(ConnectionState::Disconnected);
        }
        CommandAction::ExportMarkdown => {
            let messages: Vec<ChatMessage> = legacy_state.read().project_messages(None);
            if messages.is_empty() {
                let message = "Nothing to export — start a conversation first";
                cmd_store.write().record_failure(&name, message);
                push_command_toast(ToastSeverity::Warning, message);
                return;
            }

            let md = messages_to_markdown(&messages);
            let Ok(escaped) = serde_json::to_string(&md) else {
                let message = "Could not prepare conversation export";
                cmd_store.write().record_failure(&name, message);
                push_command_toast(ToastSeverity::Error, message);
                return;
            };

            let js = format!("navigator.clipboard.writeText({escaped})");
            let mut command_store = cmd_store;
            spawn(async move {
                if let Err(error) = document::eval(&js).await {
                    tracing::warn!(%error, "failed to copy conversation markdown to clipboard");
                    let message = "Could not copy conversation to clipboard";
                    command_store.write().record_failure("export", message);
                    push_command_toast(ToastSeverity::Error, message);
                    return;
                }

                let message = "Conversation copied to clipboard";
                command_store.write().record_success("export", message);
                push_command_toast(ToastSeverity::Success, message);
            });
        }
        CommandAction::Navigate(destination) => {
            let label = command_destination_label(destination);
            nav.push(route_for_command_destination(destination));
            let message = format!("Opened {label}");
            cmd_store.write().record_success(&name, &message);
            push_command_toast(ToastSeverity::Success, message);
        }
        CommandAction::OpenToolDetails {
            agent_id,
            tool_name,
        } => {
            nav.push(Route::MetricsToolDetail {
                tool_name: tool_name.clone(),
            });
            let message = format!("Opened {tool_name} details for {agent_id}");
            cmd_store.write().record_success(&name, &message);
            push_command_toast(ToastSeverity::Success, message);
        }
    }
}

fn fetch_chat_history_page(
    cfg: ConnectionConfig,
    selection: ChatSelection,
    before: Option<i64>,
    replace: bool,
    mut legacy_state: Signal<ChatState>,
    mut history_state: Signal<ChatHistoryState>,
) {
    let Some(session_id) = selection.session_id.clone() else {
        history_state.set(ChatHistoryState::loaded(None, None, 0));
        return;
    };

    let total_count = history_total_count(&selection).or(history_state.read().total_count);
    if replace {
        history_state.set(ChatHistoryState::loading_initial(total_count));
    } else {
        let next_state = {
            let current = history_state.read();
            current.loading_older()
        };
        history_state.set(next_state);
    }

    spawn(async move {
        let client =
            match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
                Ok(client) => client,
                Err(err) => {
                    let raw_fetched = history_state.read().raw_fetched;
                    history_state.set(ChatHistoryState::failed(
                        err.to_string(),
                        total_count,
                        before,
                        raw_fetched,
                    ));
                    return;
                }
            };

        let result = client
            .history(session_id.as_ref(), Some(HISTORY_PAGE_SIZE_QUERY), before)
            .await
            .map_err(|err| err.to_string());

        if !active_session_matches(&legacy_state.read(), &selection) {
            return;
        }

        let previous = history_state.read().clone();
        match result {
            Ok(messages) => {
                let page_oldest_seq = oldest_history_seq(&messages);
                let oldest_seq =
                    page_oldest_seq.or(if replace { None } else { previous.oldest_seq });
                // WHY(#7298): raw page length, before tool-call collapsing,
                // so pagination compares against the server's uncollapsed
                // `total_count` on the same basis.
                let page_raw_count = messages.len();
                let raw_fetched = if replace {
                    page_raw_count
                } else {
                    previous.raw_fetched + page_raw_count
                };
                let mut loaded_messages = history_messages_to_legacy(&messages);

                {
                    let mut state = legacy_state.write();
                    if replace {
                        state.messages = loaded_messages;
                    } else {
                        let existing = std::mem::take(&mut state.messages);
                        loaded_messages.extend(existing);
                        state.messages = loaded_messages;
                    }
                }

                history_state.set(ChatHistoryState::loaded(total_count, oldest_seq, raw_fetched));
            }
            Err(message) => {
                history_state.set(ChatHistoryState::failed(
                    message,
                    total_count,
                    previous.oldest_seq,
                    previous.raw_fetched,
                ));
            }
        }
    });
}

/// Resolve the durable session for a keyless selection server-side, then load
/// its history.
///
/// WHY: entering a nous's chat must attach to that nous's one ongoing
/// conversation and always reload its history. The app knows only the stable
/// session key (`{nous}:default`); the key→session binding is owned by the
/// server (`POST /api/v1/sessions/resolve`), so the client resolves rather
/// than guessing — this is also what keeps a remounted Chat view pointed at
/// the same conversation instead of an empty pane.
fn resolve_and_fetch_history(
    cfg: ConnectionConfig,
    selection: ChatSelection,
    mut legacy_state: Signal<ChatState>,
    mut history_state: Signal<ChatHistoryState>,
    mut tab_bar: Signal<TabBar>,
    cancel_token: Signal<CancellationToken>,
    queued_messages: Signal<ComposerQueue>,
    pending_dispatch: Signal<Option<String>>,
) {
    debug_assert!(selection.session_id.is_none());
    history_state.set(ChatHistoryState::loading_initial(None));
    spawn(async move {
        let client =
            match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
                Ok(client) => client,
                Err(err) => {
                    history_state.set(ChatHistoryState::failed(err.to_string(), None, None, 0));
                    return;
                }
            };

        let resolved = client
            .resolve_session(selection.agent_id.as_ref(), &selection.session_key)
            .await;

        // WHY: the operator may have switched conversations while the resolve
        // was in flight; only apply the result to the session still on screen.
        if !active_session_matches(&legacy_state.read(), &selection) {
            return;
        }

        match resolved {
            Ok(session) => {
                tab_bar.write().stamp_session_identity(
                    &selection.agent_id,
                    &selection.session_key,
                    session.id.clone(),
                    Some(session.message_count),
                );
                // WHY(#7297): stamp reattachment state before fetching
                // history so the abort control renders immediately if a
                // turn is already in flight, then reattach to its event
                // stream to keep state live.
                let reattach_turn_id = apply_active_turn_reattachment(
                    &mut legacy_state.write(),
                    session.id.clone(),
                    session.active_turn_id.clone(),
                );
                let resolved_selection = ChatSelection {
                    session_id: Some(session.id.clone()),
                    message_count: Some(session.message_count),
                    ..selection
                };
                fetch_chat_history_page(
                    cfg.clone(),
                    resolved_selection,
                    None,
                    true,
                    legacy_state,
                    history_state,
                );
                if let Some(turn_id) = reattach_turn_id {
                    reattach_active_turn(
                        cfg,
                        session.id,
                        turn_id,
                        legacy_state,
                        cancel_token,
                        queued_messages,
                        pending_dispatch,
                    );
                }
            }
            Err(err) => {
                history_state.set(ChatHistoryState::failed(err.to_string(), None, None, 0));
            }
        }
    });
}

/// Stop watching a reattached turn without claiming the turn itself ended.
///
/// WHY: unlike cancelling a self-submitted stream, cancelling (or losing)
/// a reattached connection does not abort the turn server-side -- pylon
/// only treats the *original submitting* connection's disconnect as an
/// abort signal (see [`skene::api::streaming::reattach_turn_stream`]'s doc
/// comment). Resets local streaming state to idle so the operator regains
/// normal input controls, WITHOUT committing a fabricated `TurnAbort`
/// message into history: the turn may still be running, and a fabricated
/// abort would both misreport its outcome and collide with the real
/// terminal message once a future reattach (or a history refetch) catches
/// up to what actually happened. Tells the operator via toast instead, and
/// -- deliberately -- never dequeues: the turn has not ended, so nothing
/// queued behind it should dispatch yet.
fn stop_watching_reattached_turn(legacy_state: &mut Signal<ChatState>, reason: &str) {
    legacy_state.write().streaming = StreamingState::default();
    if let Some(mut toast_store) = try_consume_context::<Signal<ToastStore>>() {
        toast_store.write().push(
            ToastSeverity::Info,
            format!("Stopped watching \u{2014} the turn continues in the background ({reason})"),
        );
    }
}

/// Reattach to a session's already in-progress turn, replaying its
/// buffered events into local state so the operator regains live progress
/// and abort control after reloading mid-turn (#7297, PR #7267's
/// `active_turn_id`).
///
/// Shares `cancel_token` with `send_message`'s own turns: `on_abort` calls
/// `cancel_token.read().cancel()` unconditionally, and this task -- not
/// `on_abort` -- decides what that means for a reattached turn (see
/// [`stop_watching_reattached_turn`]).
///
/// Once a *genuine* terminal event replays from the server (the turn
/// really did complete, abort, or error), this dequeues and dispatches a
/// message queued behind it exactly as `send_message`'s own turn loop does
/// (`dequeue_after_turn_end`, shared between both), so a message queued
/// while watching a reattached turn is not stranded once that turn ends
/// (#7299 x #7297).
fn reattach_active_turn(
    cfg: ConnectionConfig,
    session_id: skene::id::ApiSessionId,
    turn_id: skene::id::TurnId,
    mut legacy_state: Signal<ChatState>,
    mut cancel_token: Signal<CancellationToken>,
    mut queued_messages: Signal<ComposerQueue>,
    mut pending_dispatch: Signal<Option<String>>,
) {
    cancel_token.read().cancel();
    let new_token = CancellationToken::new();
    cancel_token.set(new_token.clone());

    spawn(async move {
        let client = match authenticated_streaming_client(&cfg) {
            Ok(client) => client,
            Err(err) => {
                let mut state = legacy_state.write();
                let mut manager = ChatStateManager::new_reattached();
                if manager.apply(StreamEvent::Error(err.to_string()), &mut state) {
                    tracing::trace!("applied reattach client-build failure");
                }
                return;
            }
        };

        let mut rx = skene::api::streaming::reattach_turn_stream(
            client,
            &cfg.server_url,
            session_id.as_ref(),
            turn_id.as_ref(),
            new_token.clone(),
        );

        let mut manager = ChatStateManager::new_reattached();
        let timeout = tokio::time::sleep(UI_STREAM_TIMEOUT);
        tokio::pin!(timeout);

        loop {
            let event = tokio::select! {
                biased;
                _ = new_token.cancelled() => {
                    stop_watching_reattached_turn(&mut legacy_state, "cancelled by the operator");
                    break;
                }
                _ = &mut timeout => {
                    new_token.cancel();
                    let reason = format!(
                        "reattached connection idle past {} minutes",
                        UI_STREAM_TIMEOUT.as_secs() / 60
                    );
                    stop_watching_reattached_turn(&mut legacy_state, &reason);
                    break;
                }
                event = rx.recv() => event,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    let mut state = legacy_state.write();
                    if manager.tick(&mut state) {
                        tracing::trace!("flushed buffered reattached turn state");
                    }
                    continue;
                }
            };

            let Some(event) = event else { break };
            let end_kind = TurnEndKind::of(&event);
            {
                let mut state = legacy_state.write();
                if manager.apply(event, &mut state) {
                    tracing::trace!("applied reattached turn event");
                }
            }
            if let Some(kind) = end_kind
                && let Some(next) = dequeue_after_turn_end(kind, &mut queued_messages.write())
            {
                pending_dispatch.set(Some(next));
            }
        }
    });
}

/// Chat view with virtualized scrolling, markdown rendering, and agent switching.
#[component]
pub(crate) fn Chat() -> Element {
    let mut legacy_state = use_signal(ChatState::default);
    let history_state = use_signal(ChatHistoryState::default);
    let mut input_state = use_signal(InputState::default);
    let mut cancel_token = use_signal(CancellationToken::new);
    let config: Signal<ConnectionConfig> = use_context();
    let cmd_store = use_context::<Signal<CommandStore>>();
    let command_ui = use_context::<Signal<CommandUiState>>();
    let mut agent_store = use_context::<Signal<AgentStore>>();
    let mut tab_bar = use_context::<Signal<TabBar>>();
    let mut routing_signal = use_context::<Signal<Option<RoutingState>>>();
    let mut pending_chat_selection = use_context::<Signal<Option<ChatSelection>>>();
    let mut window_state = use_context::<Signal<WindowState>>();
    let connection_state = use_context::<Signal<ConnectionState>>();
    let theme_mode = use_context::<Signal<ThemeMode>>();
    let appearance = use_context::<Signal<AppearanceSettings>>();
    let server_store = use_context::<Signal<ServerConfigStore>>();
    let keybindings = use_context::<Signal<KeybindingStore>>();
    let nav = use_navigator();

    // WHY: Track last user message to enable retry on stream failure.
    let mut last_user_message = use_signal(String::new);
    // WHY(#4793): Retry must reuse the same client turn id so the server can
    // resolve it to the existing persisted/replayable turn instead of creating
    // a duplicate user action.
    let mut last_client_turn_id = use_signal(String::new);
    // WHY: Track stream start time for elapsed-time indicator and timeout
    // escalation messages (30s "taking longer", 5m "abort and retry").
    let mut stream_start_time = use_signal(|| None::<Instant>);
    // WHY: Ticking signal drives elapsed-time re-renders every second
    // during streaming without polling the DOM.
    let mut elapsed_tick = use_signal(|| 0u64);

    // WHY(#7299): messages submitted while a turn is streaming queue here
    // instead of being discarded; `send_message` dequeues and dispatches
    // the front entry once the in-flight turn's terminal event lands.
    let mut queued_messages = use_signal(ComposerQueue::default);
    // WHY: a spawned turn task cannot call the `send_message` closure
    // defined later in this same render (it does not exist yet when the
    // task is spawned in an *earlier* render). It stashes the next queued
    // message here instead; the `use_effect` below -- which captures
    // *this* render's `send_message` -- dispatches it.
    let mut pending_dispatch = use_signal(|| None::<String>);

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

        let activation = activate_chat_selection(
            &selection,
            &mut legacy_state.write(),
            &mut agent_store.write(),
            &mut tab_bar.write(),
            &mut window_state.write(),
        );
        if activation.session_changed {
            loaded_page_count.set(1);
            if selection.session_id.is_some() {
                fetch_chat_history_page(
                    config.read().clone(),
                    selection,
                    None,
                    true,
                    legacy_state,
                    history_state,
                );
            } else {
                resolve_and_fetch_history(
                    config.read().clone(),
                    selection,
                    legacy_state,
                    history_state,
                    tab_bar,
                    cancel_token,
                    queued_messages,
                    pending_dispatch,
                );
            }
        }
        pending_chat_selection.set(None);
    });

    // WHY: entering a nous's chat from the sidebar (or landing on Chat with an
    // active agent and no explicit session pick) must open that nous's one
    // ongoing conversation, not an empty pane. Drive the canonical selection
    // through the same pending-selection path the pickers use; the server
    // resolves the stable per-nous key to the durable session.
    let mut driven_canonical = use_signal(|| None::<(skene::id::ApiNousId, String)>);
    use_effect(move || {
        if pending_chat_selection.read().is_some() {
            return;
        }
        let Some(agent_id) = agent_store.read().active_id.clone() else {
            return;
        };
        let canonical_key = resolve_chat_session_key(&agent_id, None);
        {
            let state = legacy_state.read();
            let showing_canonical = state.agent_id.as_ref() == Some(&agent_id)
                && state.session_key.as_deref() == Some(canonical_key.as_str());
            if showing_canonical {
                return;
            }
        }
        if driven_canonical.read().as_ref() == Some(&(agent_id.clone(), canonical_key.clone())) {
            return;
        }
        driven_canonical.set(Some((agent_id.clone(), canonical_key)));
        let title = agent_store
            .read()
            .get(&agent_id)
            .map(|r| r.display_name().to_string())
            .unwrap_or_else(|| agent_id.to_string());
        pending_chat_selection.set(Some(canonical_agent_selection(&agent_id, title)));
    });

    let active_nous_id = agent_store.read().active_id.clone();

    // WHY(#7282): the live streaming placeholder's header must show the
    // responding nous's display name too, not the literal "Assistant" --
    // it renders before the turn's `ChatMessage` (and its own `agent_id`)
    // exists, so it falls back to the currently active nous instead.
    let streaming_label = active_nous_id
        .as_ref()
        .and_then(|id| {
            agent_store
                .read()
                .get(id)
                .map(|r| r.display_name().to_string())
        })
        .unwrap_or_else(|| "Assistant".to_string());

    let is_streaming = legacy_state.read().streaming.is_streaming;
    // WHY(#7297): a reattached turn's abort control reads "Stop watching"
    // instead of "Abort" -- cancelling it only stops local observation, it
    // does not abort the turn server-side (see `reattach_active_turn`'s
    // `stop_watching_reattached_turn`).
    let is_reattached_turn = legacy_state.read().streaming.reattached;

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
    let history_snapshot = history_state.read().clone();
    let server_has_more_history = history_snapshot.has_older_server_history();
    let has_more_history = total_message_count > loaded_limit || server_has_more_history;
    let messages: Vec<ChatMessage> = legacy_state.read().project_messages(Some(loaded_limit));
    let active_history_selection = {
        let bar = tab_bar.read();
        bar.active_tab().and_then(|tab| {
            let session_id = tab.session_id.clone()?;
            let session_key = tab.session_key.clone()?;
            Some(ChatSelection {
                agent_id: tab.agent_id.clone(),
                session_id: Some(session_id),
                session_key,
                title: tab.title.clone(),
                message_count: tab.message_count,
            })
        })
    };
    let scroll_history_selection = active_history_selection.clone();
    let click_history_selection = active_history_selection.clone();

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

    let visible_messages: Vec<(usize, ChatMessage, bool, Option<String>)> = messages
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
            // WHY(#7282): resolve the responding nous's display name
            // per-message from its own `agent_id` (not the currently
            // active nous) so history stays correct even after the
            // operator switches which nous is active mid-session.
            // Resolved here (not inside the rsx `for` body) because
            // dioxus-rsx's `TemplateBody` grammar for a `for` loop body
            // only accepts Element/Component/Text/RawExpr/ForLoop/IfChain
            // nodes -- a bare `let` statement fails to parse.
            let agent_name = msg.agent_id.as_ref().and_then(|id| {
                agent_store
                    .read()
                    .get(id)
                    .map(|r| r.display_name().to_string())
            });
            (i, msg.clone(), grouped, agent_name)
        })
        .collect();

    let mut send_message = move |text: String, is_retry: bool| {
        if text.is_empty() {
            return;
        }

        // WHY(#7299): a retry always fires after streaming has already
        // ended (from the error banner), never mid-turn, but guard it
        // defensively rather than queueing a retry behind itself.
        if is_streaming {
            if !is_retry {
                enqueue_if_streaming(&mut queued_messages.write(), is_streaming, text);
            }
            return;
        }

        // WHY: Guard against no agent selected -- don't silently send to "default".
        let Some(active_nous_id) = agent_store.read().active_id.clone() else {
            if let Some(mut toast_store) = try_consume_context::<Signal<ToastStore>>() {
                toast_store.write().push(
                    ToastSeverity::Warning,
                    "Select an agent first \u{2014} pick one in the sidebar",
                );
            }
            return;
        };

        let session_key =
            resolve_chat_session_key(&active_nous_id, legacy_state.read().session_key.as_deref());

        // WHY: Set streaming flag BEFORE spawning to prevent double-submit race.
        // Without this, rapid Ctrl+Enter could spawn two concurrent tasks.
        legacy_state.write().streaming.is_streaming = true;

        // WHY: Clear any previous error so the retry banner disappears
        // when the user sends a new message.
        legacy_state.write().streaming.error = None;

        let retry_same_bubble = is_retry && legacy_state.read().last_is_user_message(&text);
        let client_turn_id = if retry_same_bubble {
            let existing = last_client_turn_id.read().clone();
            if existing.is_empty() {
                koina::ulid::Ulid::new().to_string()
            } else {
                existing
            }
        } else {
            koina::ulid::Ulid::new().to_string()
        };

        last_user_message.set(text.clone());
        last_client_turn_id.set(client_turn_id.clone());
        stream_start_time.set(Some(Instant::now()));
        elapsed_tick.set(0);

        // WHY: A retry re-sends the failed turn in place. The original user
        // bubble is already the last history entry, so appending again would
        // render the same message twice (or more on repeated retries).
        let already_last = retry_same_bubble;
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
                turn_id: None,
                session_id: None,
                request_id: None,
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
            let client = match authenticated_streaming_client(&cfg) {
                Ok(client) => client,
                Err(err) => {
                    let mut state = legacy_state.write();
                    let mut manager = ChatStateManager::new();
                    if manager.apply(StreamEvent::Error(err.to_string()), &mut state) {
                        tracing::trace!("applied chat stream client-build failure");
                    }
                    return;
                }
            };

            // WHY: Capture the selected agent at submission time so a later
            // sidebar click cannot reroute this in-flight turn.
            let nous_id = active_nous_id.to_string();

            let mut rx = skene::api::streaming::stream_message(
                client,
                &cfg.server_url,
                &nous_id,
                &session_key,
                &text,
                &client_turn_id,
                new_token.clone(),
            );

            let mut manager = ChatStateManager::new();
            let mut file_tracker = FileChangeTracker::new();
            let timeout = tokio::time::sleep(UI_STREAM_TIMEOUT);
            tokio::pin!(timeout);

            // WHY: Derive agent display name for the routing indicator.
            // Resolve once at turn start to avoid repeated agent store reads.
            let routing_agent_name = {
                let store = agent_store.read();
                store
                    .get(&skene::id::ApiNousId::from(nous_id.as_str()))
                    .map(|r| r.display_name().to_string())
                    .unwrap_or_else(|| nous_id.clone())
            };
            let routing_agent_id = skene::id::ApiNousId::from(nous_id.as_str());

            update_routing_stage(
                &mut routing_signal,
                PipelineStage::Bootstrap,
                &routing_agent_name,
                &routing_agent_id,
            );

            // WHY(#7299): tracks how the turn ended so the post-loop dequeue
            // (shared with `reattach_active_turn` via `dequeue_after_turn_end`)
            // knows whether to dispatch -- `None` for an abnormal channel
            // close with no terminal event, which is treated like `Errored`
            // (do not auto-dispatch into unknown state).
            let mut last_terminal: Option<TurnEndKind> = None;

            loop {
                let event = tokio::select! {
                    biased;
                    _ = new_token.cancelled() => {
                        let mut state = legacy_state.write();
                        if manager.apply(
                            StreamEvent::TurnAbort {
                                reason: "cancelled by user".to_string(),
                            },
                            &mut state,
                        ) {
                            tracing::trace!("applied chat stream cancellation");
                        }
                        last_terminal = Some(TurnEndKind::Aborted);
                        break;
                    }
                    _ = &mut timeout => {
                        new_token.cancel();
                        let message = format!(
                            "stream timed out after {} minutes; stream task cancelled",
                            UI_STREAM_TIMEOUT.as_secs() / 60
                        );
                        let mut state = legacy_state.write();
                        if manager.apply(StreamEvent::Error(message), &mut state) {
                            tracing::trace!("applied chat stream timeout");
                        }
                        last_terminal = Some(TurnEndKind::Errored);
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
                let end_kind = TurnEndKind::of(&event);

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
                            action_id: crate::state::toasts::ToastActionId(action_id),
                        }),
                    );
                }

                // WHY: Update routing indicator stage from stream events.
                // This gives the operator real-time visibility into what
                // the pipeline is doing (#2411 transparent routing).
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
                drop(state);
                if let Some(kind) = end_kind {
                    last_terminal = Some(kind);
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

            // WHY(#7299): the turn just ended -- dispatch whatever queued up
            // behind it, UNLESS it ended `Errored`: `send_message` clears
            // `streaming.error` at its own start, so dispatching immediately
            // would wipe the retry banner before the operator ever sees it
            // (shared with `reattach_active_turn` via
            // `dequeue_after_turn_end`). Stashed in `pending_dispatch` rather
            // than called directly: this spawned task cannot call
            // `send_message` itself (see the signal's WHY comment above).
            if let Some(kind) = last_terminal
                && let Some(next) = dequeue_after_turn_end(kind, &mut queued_messages.write())
            {
                pending_dispatch.set(Some(next));
            }
        });
    };

    // WHY(#7299): drives the queue: reads (so it re-runs whenever the
    // spawned turn task above sets `pending_dispatch`), takes the pending
    // text, and dispatches it through *this* render's `send_message`.
    use_effect(move || {
        let next = pending_dispatch.read().clone();
        if let Some(text) = next {
            pending_dispatch.set(None);
            send_message(text, false);
        }
    });

    let command_runtime = CommandRuntime {
        command_ui,
        cmd_store,
        legacy_state,
        connection_state,
        theme_mode,
        appearance,
        server_store,
        keybindings,
        nav,
    };

    let on_submit = move |text: String| {
        if text.trim_start().starts_with('/') {
            execute_command_input(text, command_runtime);
        } else {
            send_message(text, false);
        }
    };

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
                        if history_snapshot.is_initial_loading() {
                            "Loading session history"
                        } else if history_snapshot.error().is_some() {
                            "Could not load session history"
                        } else if history_snapshot.is_loaded() && active_history_selection.is_some()
                        {
                            "No messages in this session"
                        } else {
                            "Start a conversation"
                        }
                    }
                    div {
                        style: "font-size: var(--text-sm);",
                        if let Some(err) = history_snapshot.error() {
                            "{err}"
                        } else if history_snapshot.is_initial_loading() {
                            "Fetching the saved transcript."
                        } else if history_snapshot.is_loaded() && active_history_selection.is_some()
                        {
                            "Type a message below to continue."
                        } else {
                            "Type a message below to begin."
                        }
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
                        let selection_for_scroll = scroll_history_selection.clone();
                        spawn(async move {
                            if let Ok(val) = document::eval(js).await {
                                let text = val.to_string();
                                let cleaned = text.trim_matches('"');
                                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(cleaned) {
                                    if let Some(top) = parsed.get("top").and_then(|v| v.as_f64()) {
                                        scroll_top.set(top);
                                        // WHY: Load older messages when the user scrolls
                                        // near the top of the viewport (#3321).
                                        if top < LOAD_MORE_THRESHOLD {
                                            if total_message_count > loaded_limit {
                                                loaded_page_count.set(loaded_page_count() + 1);
                                            } else if server_has_more_history
                                                && !history_state.read().is_loading()
                                                && let Some(selection) = selection_for_scroll.clone()
                                                && let Some(before_seq) = history_state.read().oldest_seq
                                            {
                                                fetch_chat_history_page(
                                                    config.read().clone(),
                                                    selection,
                                                    Some(before_seq),
                                                    false,
                                                    legacy_state,
                                                    history_state,
                                                );
                                            }
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
                                if total_message_count > loaded_limit {
                                    loaded_page_count.set(loaded_page_count() + 1);
                                } else if server_has_more_history
                                    && !history_state.read().is_loading()
                                    && let Some(selection) = click_history_selection.clone()
                                    && let Some(before_seq) = history_state.read().oldest_seq
                                {
                                    fetch_chat_history_page(
                                        config.read().clone(),
                                        selection,
                                        Some(before_seq),
                                        false,
                                        legacy_state,
                                        history_state,
                                    );
                                }
                            },
                            if history_snapshot.is_loading_older() {
                                "Loading older messages..."
                            } else {
                                "Scroll up or click to load older messages ({total_message_count} loaded)"
                            }
                        }
                    }

                    for (idx , msg , grouped , agent_name) in visible_messages {
                        MessageBubble {
                            key: "{idx}",
                            message: msg,
                            is_grouped: grouped,
                            agent_name,
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
                                    "{streaming_label}"
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
                is_open: command_ui.read().palette_open,
                on_execute: move |cmd: String| {
                    execute_command_input(cmd, command_runtime);
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

            // WHY(#7299): a message queued mid-turn must be visible, not
            // silently held -- otherwise a reload or a distracted operator
            // has no evidence it will still send once the turn ends.
            if !queued_messages.read().is_empty() {
                div {
                    style: "
                        display: flex;
                        flex-direction: column;
                        gap: var(--space-1);
                        background: var(--bg-surface-dim);
                        border: 1px solid var(--border-separator);
                        border-radius: var(--radius-md);
                        padding: var(--space-2) var(--space-3);
                        margin: 0 var(--space-4) var(--space-2) var(--space-4);
                        font-size: var(--text-sm);
                        color: var(--text-muted);
                    ",
                    span {
                        {
                            let count = queued_messages.read().len();
                            let noun = if count == 1 { "message" } else { "messages" };
                            format!("{count} {noun} queued \u{2014} sends when this turn ends")
                        }
                    }
                    for (idx , text) in queued_messages.read().iter().cloned().enumerate() {
                        div {
                            key: "{idx}",
                            style: "
                                display: flex;
                                align-items: center;
                                justify-content: space-between;
                                gap: var(--space-2);
                                overflow-wrap: anywhere;
                            ",
                            span { style: "color: var(--text-secondary);", "{text}" }
                            button {
                                style: "\
                                    all: unset; \
                                    cursor: pointer; \
                                    color: var(--text-muted); \
                                    font-size: var(--text-xs); \
                                    flex-shrink: 0;\
                                ",
                                onclick: move |_| {
                                    queued_messages.write().remove(&text);
                                },
                                "Remove"
                            }
                        }
                    }
                }
            }

            InputBar {
                input: input_state,
                is_streaming: is_streaming,
                is_reattached: is_reattached_turn,
                on_submit: on_submit,
                on_abort: on_abort,
            }
        }
    }
}

// NOTE: visible_range tests live in skeue.
pub(crate) use skeue::visible_range;
