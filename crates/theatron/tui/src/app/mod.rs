mod persistence;
pub(crate) use persistence::{
    MAX_COMMAND_HISTORY, exports_dir, save_command_history, save_session_state,
};

use std::collections::{HashMap, HashSet};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use tokio::sync::mpsc;

use persistence::{load_command_history, load_session_state};

use crate::api::client::ApiClient;
use crate::api::sse::SseConnection;
use crate::config::Config;
use crate::error::{GatewayUnreachableSnafu, Result, TokenRequiredSnafu};
use crate::events::StreamEvent;
use crate::hyperlink::OscLink;
use crate::id::{NousId, SessionId, TurnId};
use crate::keybindings::KeyMap;
use crate::msg::{ErrorToast, Msg};
use crate::sanitize::sanitize_for_display;
use crate::state::ArcVec;
use crate::state::MarkdownCache;
use crate::state::MetricsState;
use crate::state::SavedScrollState;
use crate::state::TabBar;
use crate::state::virtual_scroll::VirtualScroll;
#[expect(
    unused_imports,
    reason = "re-exported for downstream modules that import from crate::app"
)]
pub use crate::state::{
    ActiveTool, AgentState, AgentStatus, ChatMessage, CommandPaletteState, ContextAction,
    ContextActionsOverlay, DecisionCardOverlay, DecisionField, DecisionOption, ErrorBanner,
    FilterState, FocusedPane, InputState, MemoryInspectorState, MessageKind, NotificationStore,
    OpsState, Overlay, PlanApprovalOverlay, PlanStepApproval, SelectionContext,
    SessionPickerOverlay, SlashCompleteState, StreamPhase, SubmittedDecision, TabCompletion, Toast,
    ToolApprovalOverlay, ToolCallInfo, ToolSummary, View, ViewStack,
};
#[cfg(test)]
use crate::theme::THEME;
use crate::theme::Theme;
use crate::update::extract_text_content;
use crate::view;

/// Default terminal width used before the first resize event arrives.
const DEFAULT_TERMINAL_WIDTH: u16 = 120;
/// Default terminal height used before the first resize event arrives.
const DEFAULT_TERMINAL_HEIGHT: u16 = 40;
/// Minimum interval between renders in milliseconds (30fps cap).
const MIN_RENDER_INTERVAL_MS: u64 = 33;

// ---------------------------------------------------------------------------
// Sub-structs: group related fields so App stays under ~10 top-level fields.
// ---------------------------------------------------------------------------

/// Agent roster, sessions, messages, and cost tracking.
pub struct DashboardState {
    pub agents: Vec<AgentState>,
    pub focused_agent: Option<NousId>,
    /// PERF: ArcVec clone is O(1): tab switches share the Arc pointer, not the Vec.
    pub messages: ArcVec<ChatMessage>,
    pub focused_session_id: Option<SessionId>,
    pub daily_cost_cents: u32,
    pub session_cost_cents: u32,
    pub context_usage_pct: Option<u8>,
    /// Raw token count currently in the context window (input + cache-read).
    pub context_tokens_used: Option<u32>,
    /// Total context window capacity for the current model.
    pub context_tokens_total: Option<u32>,
    /// Last-active session per agent, loaded from disk on startup and saved on exit.
    pub(crate) saved_sessions: HashMap<NousId, SessionId>,
    pub submitted_decisions: Vec<crate::state::SubmittedDecision>,
}

/// SSE link, stream receiver, and reconnect bookkeeping.
pub struct ConnectionState {
    sse: Option<SseConnection>,
    pub sse_connected: bool,
    /// When the SSE stream last disconnected. Cleared on successful reconnect.
    pub sse_disconnected_at: Option<std::time::Instant>,
    /// Timestamp of the most recent SSE event, for connection quality indicator.
    pub sse_last_event_at: Option<std::time::Instant>,
    /// Number of SSE reconnections since startup.
    pub sse_reconnect_count: u32,
    pub(crate) stream_rx: Option<mpsc::Receiver<StreamEvent>>,
    pub active_turn_id: Option<TurnId>,
    pub streaming_text: String,
    pub streaming_thinking: String,
    pub streaming_tool_calls: Vec<ToolCallInfo>,
    /// Current stream lifecycle phase. Drives status indicator rendering.
    pub stream_phase: crate::state::StreamPhase,
    /// Partial line buffer for line-by-line streaming.
    /// Complete lines (ending in `\n`) are flushed to `streaming_text`;
    /// the incomplete tail stays here until the next delta.
    pub(crate) streaming_line_buffer: String,
    /// Timestamp of the last stream event received during the current turn.
    /// Used for stall detection.
    pub(crate) stream_last_event_at: Option<std::time::Instant>,
    /// Set once the 30s stall warning has been shown for the current turn.
    pub(crate) stall_warned: bool,
    /// Non-dismissing status message shown during stall conditions.
    pub(crate) stall_message: Option<String>,
    /// Monotonic counter incremented on every stream lifecycle boundary
    /// (TurnStart, TurnComplete, TurnAbort, Error, CancelTurn).
    /// SSE handlers snapshot this before an async history reload and compare
    /// afterward: if it changed, a concurrent stream event mutated state and
    /// the reload result must be discarded to prevent stale UI.
    pub(crate) state_epoch: u64,
}

impl ConnectionState {
    /// Returns true when a stream is active, pending, or just completed and
    /// state should not be clobbered by an SSE-triggered history reload.
    pub(crate) fn is_stream_busy(&self) -> bool {
        self.stream_rx.is_some()
            || self.active_turn_id.is_some()
            || !matches!(
                self.stream_phase,
                crate::state::StreamPhase::Idle | crate::state::StreamPhase::Error
            )
    }
}

/// Scroll position, virtual scroll index, and markdown render cache.
pub struct RenderState {
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub(crate) scroll_states: HashMap<NousId, SavedScrollState>,
    pub(crate) virtual_scroll: VirtualScroll,
    pub markdown_cache: MarkdownCache,
    /// Pre-rendered lines for finalized (committed) messages.
    /// PERF: Never re-rendered; only appended when a new message is committed.
    pub(crate) static_lines: Vec<ratatui::text::Line<'static>>,
    /// Number of committed messages covered by `static_lines`.
    pub(crate) static_message_count: usize,
    /// Width used to render `static_lines`. Cache invalidated on width change.
    pub(crate) static_width: usize,
}

/// Terminal dimensions, tick counter, dirty flag, frame cache, and render state.
pub struct ViewportState {
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub tick_count: u64,
    pub error_toast: Option<ErrorToast>,
    pub success_toast: Option<ErrorToast>,
    /// Multi-type toast queue; each entry auto-dismisses after `duration_secs`.
    pub toasts: Vec<Toast>,
    /// Persistent top-of-viewport error banner, dismissed explicitly.
    pub error_banner: Option<ErrorBanner>,
    pub(crate) dirty: bool,
    pub(crate) frame_cache: Option<Buffer>,
    /// Timestamp of the last completed render. Used for 30fps throttling.
    pub(crate) last_render_at: Option<std::time::Instant>,
    pub render: RenderState,
}

/// Input, tab completion, command palette, slash complete, selection, filter, and key state.
pub struct InteractionState {
    pub input: InputState,
    pub tab_completion: Option<TabCompletion>,
    pub command_palette: CommandPaletteState,
    pub slash_complete: SlashCompleteState,
    pub command_history: Vec<String>,
    pub command_history_index: Option<usize>,
    pub selection: SelectionContext,
    pub selected_message: Option<usize>,
    pub tool_expanded: HashSet<crate::id::ToolId>,
    pub filter: FilterState,
    pub(crate) keymap: KeyMap,
    /// Tool names that bypass the approval dialog for the lifetime of this TUI session.
    pub(crate) always_allowed_tools: HashSet<String>,
    /// Messages queued while the agent is streaming; auto-sent when the turn completes.
    pub queued_messages: Vec<crate::state::QueuedMessage>,
}

/// Sidebar, overlay, view stack, ops, tabs, memory inspector, and notification log.
pub struct LayoutState {
    pub sidebar_visible: bool,
    pub thinking_expanded: bool,
    pub overlay: Option<Overlay>,
    pub view_stack: ViewStack,
    pub(crate) view_scroll_states: HashMap<usize, SavedScrollState>,
    pub ops: OpsState,
    pub(crate) tab_bar: TabBar,
    pub memory: MemoryInspectorState,
    pub(crate) metrics: MetricsState,
    pub(crate) editor: crate::state::editor::EditorState,
    pub(crate) pending_g: bool,
    pub(crate) bell_enabled: bool,
    /// Cross-agent notification log with read/unread tracking.
    pub notifications: NotificationStore,
}

pub struct App {
    pub config: Config,
    pub client: ApiClient,
    pub theme: Theme,
    pub highlighter: crate::highlight::Highlighter,
    pub should_quit: bool,

    pub dashboard: DashboardState,
    pub connection: ConnectionState,
    pub viewport: ViewportState,
    pub interaction: InteractionState,
    pub layout: LayoutState,
}

impl App {
    /// Connect to the gateway and initialize the TUI application.
    ///
    /// # Errors
    ///
    /// Returns an error if the API client cannot be constructed, the gateway is
    /// unreachable, or the server requires authentication and no token is configured.
    #[tracing::instrument(skip_all, fields(url = %config.url))]
    pub async fn init(config: Config) -> Result<Self> {
        let client = ApiClient::new(
            &config.url,
            config.token.as_ref().map(|t| t.expose_secret().to_owned()),
        )?;

        let theme = Theme::for_mode(config.theme);
        tracing::info!(depth = ?theme.depth, mode = ?theme.mode, "theme initialized");

        let command_history = load_command_history(&config);
        let saved_sessions = load_session_state(&config);
        let keymap = KeyMap::build(&config.keybindings);
        let bell_enabled = config.bell;

        let mut app = Self {
            config,
            client,
            theme: theme.clone(),
            highlighter: crate::highlight::Highlighter::new(theme.mode),
            should_quit: false,
            dashboard: DashboardState {
                agents: Vec::new(),
                focused_agent: None,
                messages: ArcVec::default(),
                focused_session_id: None,
                daily_cost_cents: 0,
                session_cost_cents: 0,
                context_usage_pct: None,
                context_tokens_used: None,
                context_tokens_total: None,
                saved_sessions,
                submitted_decisions: Vec::new(),
            },
            connection: ConnectionState {
                sse: None,
                sse_connected: false,
                sse_disconnected_at: None,
                sse_last_event_at: None,
                sse_reconnect_count: 0,
                stream_rx: None,
                active_turn_id: None,
                streaming_text: String::new(),
                streaming_thinking: String::new(),
                streaming_tool_calls: Vec::new(),
                stream_phase: crate::state::StreamPhase::default(),
                streaming_line_buffer: String::new(),
                stream_last_event_at: None,
                stall_warned: false,
                stall_message: None,
                state_epoch: 0,
            },
            viewport: ViewportState {
                terminal_width: DEFAULT_TERMINAL_WIDTH,
                terminal_height: DEFAULT_TERMINAL_HEIGHT,
                tick_count: 0,
                error_toast: None,
                success_toast: None,
                toasts: Vec::new(),
                error_banner: None,
                dirty: true,
                frame_cache: None,
                last_render_at: None,
                render: RenderState {
                    scroll_offset: 0,
                    auto_scroll: true,
                    scroll_states: HashMap::new(),
                    virtual_scroll: VirtualScroll::new(),
                    markdown_cache: MarkdownCache::default(),
                    static_lines: Vec::new(),
                    static_message_count: 0,
                    static_width: 0,
                },
            },
            interaction: InteractionState {
                input: InputState::default(),
                tab_completion: None,
                command_palette: CommandPaletteState::default(),
                slash_complete: SlashCompleteState::default(),
                command_history,
                command_history_index: None,
                selection: SelectionContext::default(),
                selected_message: None,
                tool_expanded: HashSet::new(),
                filter: FilterState::default(),
                keymap,
                always_allowed_tools: HashSet::new(),
                queued_messages: Vec::new(),
            },
            layout: LayoutState {
                sidebar_visible: true,
                thinking_expanded: false,
                overlay: None,
                view_stack: ViewStack::new(),
                view_scroll_states: HashMap::new(),
                ops: OpsState::default(),
                tab_bar: TabBar::new(),
                memory: MemoryInspectorState::new(),
                metrics: MetricsState::new(),
                editor: crate::state::editor::EditorState::default(),
                pending_g: false,
                bell_enabled,
                notifications: NotificationStore::default(),
            },
        };

        app.connect().await?;

        Ok(app)
    }

    #[tracing::instrument(skip(self), fields(url = %self.config.url))]
    async fn connect(&mut self) -> Result<()> {
        if !self.client.health().await.unwrap_or(false) {
            return GatewayUnreachableSnafu {
                url: self.config.url.clone(),
            }
            .fail();
        }

        match self.client.auth_mode().await {
            Ok(mode) => match mode.mode.as_str() {
                "none" => {
                    tracing::info!("no auth required");
                }
                "token" => {
                    if self.client.token().is_none() {
                        return TokenRequiredSnafu {
                            message: "gateway requires token auth. Pass --token or set ALETHEIA_TOKEN",
                        }
                        .fail();
                    }
                }
                _ => {
                    if self.client.token().is_none() {
                        return TokenRequiredSnafu {
                            message: "gateway requires authentication. Pass --token or set ALETHEIA_TOKEN",
                        }
                        .fail();
                    }
                }
            },
            Err(e) => {
                tracing::warn!("could not detect auth mode: {e}, proceeding without auth");
            }
        }

        // SAFETY: sanitized at ingestion: all agent fields from API are sanitized here.
        // Best-effort: if agent fetch fails, start with empty list and show error toast.
        let agents = match self.client.agents().await {
            Ok(a) => a,
            Err(e) => {
                tracing::error!("failed to load agents: {e}");
                self.viewport.error_toast = Some(ErrorToast::new(format!(
                    "Failed to load agents: {e}. Retry with :reconnect"
                )));
                Vec::new()
            }
        };
        self.dashboard.agents = agents
            .into_iter()
            .map(|a| {
                let name = sanitize_for_display(a.display_name()).into_owned();
                let name_lower = name.to_lowercase();
                AgentState {
                    id: a.id.clone(),
                    name,
                    name_lower,
                    emoji: a.emoji.map(|e| sanitize_for_display(&e).into_owned()),
                    status: AgentStatus::Idle,
                    active_tool: None,
                    sessions: Vec::new(),
                    model: a.model.map(|m| sanitize_for_display(&m).into_owned()),
                    compaction_stage: None,
                    distill_completed_at: None,
                    unread_count: 0,
                    tools: Vec::new(),
                }
            })
            .collect();

        self.dashboard.focused_agent = self
            .config
            .default_agent
            .clone()
            .map(NousId::from)
            .or_else(|| self.dashboard.agents.first().map(|a| a.id.clone()));

        if let Some(agent_id) = self.dashboard.focused_agent.clone() {
            if let Ok(sessions) = self.client.sessions(&agent_id).await
                && let Some(agent) = self.dashboard.agents.iter_mut().find(|a| a.id == agent_id)
            {
                agent.sessions = sessions;
            }
            self.load_focused_session().await;

            if let Ok(tools) = self.client.tools(&agent_id).await
                && let Some(agent) = self.dashboard.agents.iter_mut().find(|a| a.id == agent_id)
            {
                agent.tools = tools
                    .into_iter()
                    .map(|t| ToolSummary {
                        name: sanitize_for_display(&t.name).into_owned(),
                        enabled: t.enabled,
                    })
                    .collect();
            }

            // Create initial tab for the default agent/session
            let agent_name = self
                .dashboard
                .agents
                .iter()
                .find(|a| a.id == agent_id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| agent_id.to_string());
            let title = self.tab_title_for_current(&agent_name);
            let idx = self.layout.tab_bar.create_tab(agent_id, title);
            self.layout.tab_bar.active = idx;
            self.save_to_active_tab();
        }

        if let Ok(cents) = self.client.today_cost_cents().await {
            self.dashboard.daily_cost_cents = cents;
        }

        self.connection.sse = Some(SseConnection::connect(
            self.client.raw_client().clone(),
            &self.config.url,
        ));

        Ok(())
    }

    #[tracing::instrument(skip(self), fields(agent = ?self.dashboard.focused_agent))]
    pub(crate) async fn load_focused_session(&mut self) {
        let agent_id = match &self.dashboard.focused_agent {
            Some(id) => id.clone(),
            None => return,
        };

        // Snapshot the epoch before any async work. If a stream event mutates
        // state while we await the HTTP response, the epoch will differ and we
        // discard the now-stale history to prevent clobbering live state.
        let epoch_before = self.connection.state_epoch;

        {
            let needs_load = self
                .dashboard
                .agents
                .iter()
                .find(|a| a.id == agent_id)
                .map(|a| a.sessions.is_empty())
                .unwrap_or(false);

            if needs_load
                && let Ok(sessions) = self.client.sessions(&agent_id).await
                && let Some(agent) = self.dashboard.agents.iter_mut().find(|a| a.id == agent_id)
            {
                agent.sessions = sessions;
            }
        }

        // Epoch check: a stream event arrived while we awaited the sessions fetch.
        if self.connection.state_epoch != epoch_before {
            tracing::info!("epoch changed during session load, discarding stale result");
            return;
        }

        let agent = match self.dashboard.agents.iter().find(|a| a.id == agent_id) {
            Some(a) => a,
            None => return,
        };

        // WHY: Prefer the session the user last had open for this agent so that the TUI
        // resumes exactly where they left off after a restart.  Fall back to the most
        // recently updated session when no saved state exists, or when the saved session
        // is no longer in the agent's session list (e.g. it was deleted server-side).
        let saved_id = self.dashboard.saved_sessions.get(&agent_id).cloned();
        let session = if let Some(ref key) = self.config.default_session {
            agent.sessions.iter().find(|s| s.key == *key)
        } else if let Some(ref sid) = saved_id {
            agent.sessions.iter().find(|s| s.id == *sid)
        } else {
            agent
                .sessions
                .iter()
                .filter(|s| {
                    s.session_type.as_deref() != Some("background")
                        && s.status.as_deref() != Some("archived")
                        && !s.key.contains(":archived:")
                        && !s.key.starts_with("cron:")
                        && !s.key.starts_with("daemon:")
                        && !s.key.starts_with("prosoche")
                        && !s.key.starts_with("agent:")
                })
                .max_by(|a, b| {
                    let a_ts = a.updated_at.as_deref().unwrap_or("");
                    let b_ts = b.updated_at.as_deref().unwrap_or("");
                    a_ts.cmp(b_ts)
                })
        }
        .or_else(|| agent.sessions.iter().find(|s| s.key == "main"))
        .or_else(|| agent.sessions.first());

        if let Some(session) = session {
            let session_id = session.id.clone();
            self.dashboard.focused_session_id = Some(session_id.clone());
            // Track the last-used session for this agent so we can restore it on relaunch.
            self.dashboard
                .saved_sessions
                .insert(agent_id.clone(), session_id.clone());

            match self.client.history(&session_id).await {
                Ok(history) => {
                    // Epoch check: a stream event arrived while we awaited the history fetch.
                    if self.connection.state_epoch != epoch_before {
                        tracing::info!(
                            "epoch changed during history fetch, discarding stale result"
                        );
                        return;
                    }
                    // SAFETY: sanitized at ingestion: all message fields from API.
                    self.dashboard.messages = history
                        .into_iter()
                        .filter_map(|m| {
                            if m.role != "user" && m.role != "assistant" {
                                return None;
                            }
                            let text = extract_text_content(&m.content)?;
                            let text = sanitize_for_display(&text).into_owned();
                            let text_lower = text.to_lowercase();
                            Some(ChatMessage {
                                role: sanitize_for_display(&m.role).into_owned(),
                                text,
                                text_lower,
                                timestamp: m
                                    .created_at
                                    .map(|t| sanitize_for_display(&t).into_owned()),
                                model: m.model.map(|m| sanitize_for_display(&m).into_owned()),
                                tool_calls: Vec::new(),
                                kind: MessageKind::default(),
                            })
                        })
                        .collect();
                    // Stale streaming markdown from the previous session must not
                    // bleed through when the user switches agents.
                    self.viewport.render.markdown_cache.clear();
                    self.viewport.render.static_lines.clear();
                    self.viewport.render.static_message_count = 0;
                    self.rebuild_virtual_scroll();
                    self.scroll_to_bottom();
                }
                Err(e) => {
                    tracing::error!("failed to load history: {e}");
                }
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn take_sse(&mut self) -> Option<SseConnection> {
        self.connection.sse.take()
    }

    #[tracing::instrument(skip_all)]
    pub fn restore_sse(&mut self, sse: Option<SseConnection>) {
        self.connection.sse = sse;
    }

    #[tracing::instrument(skip_all)]
    pub fn take_stream(&mut self) -> Option<mpsc::Receiver<StreamEvent>> {
        self.connection.stream_rx.take()
    }

    #[tracing::instrument(skip_all)]
    pub fn restore_stream(&mut self, rx: Option<mpsc::Receiver<StreamEvent>>) {
        self.connection.stream_rx = rx;
    }

    #[tracing::instrument(skip_all)]
    pub async fn update(&mut self, msg: Msg) {
        let is_tick = matches!(msg, Msg::Tick);
        if !is_tick {
            self.viewport.dirty = true;
            crate::update::update(self, msg).await;
            return;
        }
        // WHY: Tick fires at 60 fps even when nothing changes. Only mark dirty when
        // tick-driven animation is actually visible: streaming spinner or toast dismissal.
        let had_animation = self.connection.active_turn_id.is_some()
            || self.viewport.error_toast.is_some()
            || self.viewport.success_toast.is_some()
            || self.connection.stall_message.is_some()
            || !self.viewport.toasts.is_empty();
        crate::update::update(self, msg).await;
        let has_animation = self.connection.active_turn_id.is_some()
            || self.viewport.error_toast.is_some()
            || self.viewport.success_toast.is_some()
            || self.connection.stall_message.is_some()
            || !self.viewport.toasts.is_empty();
        self.viewport.dirty = had_animation || has_animation;
    }

    /// Save current app state into the active tab.
    pub(crate) fn save_to_active_tab(&mut self) {
        if let Some(tab) = self.layout.tab_bar.active_tab_mut() {
            tab.session_id = self.dashboard.focused_session_id.clone();
            tab.state.messages = self.dashboard.messages.clone();
            tab.state.focused_session_id = self.dashboard.focused_session_id.clone();
            tab.state.input = self.interaction.input.clone();
            tab.state.scroll = SavedScrollState {
                scroll_offset: self.viewport.render.scroll_offset,
                auto_scroll: self.viewport.render.auto_scroll,
            };
            tab.state.selected_message = self.interaction.selected_message;
            tab.state.tool_expanded = self.interaction.tool_expanded.clone();
            tab.state.filter = self.interaction.filter.clone();
            tab.state.view_stack = self.layout.view_stack.clone();
            tab.state.streaming.streaming_text = self.connection.streaming_text.clone();
            tab.state.streaming.streaming_thinking = self.connection.streaming_thinking.clone();
            tab.state.streaming.streaming_tool_calls = self.connection.streaming_tool_calls.clone();
            tab.state.streaming.active_turn_id = self.connection.active_turn_id.clone();
            tab.state.streaming.stream_phase = self.connection.stream_phase;
            tab.state.markdown_cache = self.viewport.render.markdown_cache.clone();
            tab.state.ops = self.layout.ops.clone();
        }
    }

    /// Restore app state from the active tab.
    pub(crate) fn restore_from_active_tab(&mut self) {
        if let Some(tab) = self.layout.tab_bar.active_tab() {
            self.dashboard.focused_agent = Some(tab.agent_id.clone());
            self.dashboard.focused_session_id = tab.state.focused_session_id.clone();
            self.dashboard.messages = tab.state.messages.clone();
            self.interaction.input = tab.state.input.clone();
            self.viewport.render.scroll_offset = tab.state.scroll.scroll_offset;
            self.viewport.render.auto_scroll = tab.state.scroll.auto_scroll;
            self.interaction.selected_message = tab.state.selected_message;
            self.interaction.tool_expanded = tab.state.tool_expanded.clone();
            self.interaction.filter = tab.state.filter.clone();
            self.layout.view_stack = tab.state.view_stack.clone();
            self.connection.streaming_text = tab.state.streaming.streaming_text.clone();
            self.connection.streaming_thinking = tab.state.streaming.streaming_thinking.clone();
            self.connection.streaming_tool_calls = tab.state.streaming.streaming_tool_calls.clone();
            self.connection.active_turn_id = tab.state.streaming.active_turn_id.clone();
            self.connection.stream_phase = tab.state.streaming.stream_phase;
            self.viewport.render.markdown_cache = tab.state.markdown_cache.clone();
            self.layout.ops = tab.state.ops.clone();
        }
    }

    /// Build a display title for the current agent+session.
    pub(crate) fn tab_title_for_current(&self, agent_name: &str) -> String {
        let session_label =
            self.dashboard
                .focused_session_id
                .as_ref()
                .and_then(|sid| {
                    self.dashboard.focused_agent.as_ref().and_then(|aid| {
                        self.dashboard
                            .agents
                            .iter()
                            .find(|a| a.id == *aid)
                            .and_then(|a| {
                                a.sessions.iter().find(|s| s.id == *sid).map(|s| {
                                    s.display_name.as_deref().unwrap_or(&s.key).to_string()
                                })
                            })
                    })
                })
                .unwrap_or_else(|| "main".to_string());
        format!("{agent_name}: {session_label}")
    }

    /// Switch to a different tab by index, saving current and restoring target.
    pub(crate) fn switch_to_tab(&mut self, index: usize) {
        if index == self.layout.tab_bar.active {
            return;
        }
        self.save_to_active_tab();
        if !self.layout.tab_bar.jump_to(index) {
            return;
        }
        self.layout.tab_bar.clear_active_unread();
        self.restore_from_active_tab();
    }

    #[tracing::instrument(skip_all)]
    pub fn view(&mut self, frame: &mut Frame) -> Vec<OscLink> {
        // PERF: 30fps render throttle. Skip the frame if less than 33ms elapsed
        // since the last render AND the state hasn't changed.
        if !self.viewport.dirty
            && let Some(ref cached) = self.viewport.frame_cache
            && cached.area == frame.area()
        {
            *frame.buffer_mut() = cached.clone();
            return Vec::new();
        }
        if let Some(last) = self.viewport.last_render_at
            && last.elapsed() < std::time::Duration::from_millis(MIN_RENDER_INTERVAL_MS)
            && !self.viewport.dirty
            && let Some(ref cached) = self.viewport.frame_cache
            && cached.area == frame.area()
        {
            *frame.buffer_mut() = cached.clone();
            return Vec::new();
        }
        // PERF: Refresh the streaming markdown cache once per frame instead of on
        // every text delta. Multiple deltas arriving between frames are batched
        // into a single markdown::render call, reducing CPU from O(tokens) to O(frames).
        self.refresh_streaming_markdown_cache();
        self.refresh_static_lines_cache();
        let links = view::render(self, frame);
        self.viewport.frame_cache = Some(frame.buffer_mut().clone());
        self.viewport.dirty = false;
        self.viewport.last_render_at = Some(std::time::Instant::now());
        links
    }

    /// Rebuild the streaming markdown cache if the text has changed since the
    /// last render. Called once per frame, not per token delta.
    pub(crate) fn refresh_streaming_markdown_cache(&mut self) {
        if self.connection.streaming_text.is_empty() {
            return;
        }
        let width = usize::from(self.viewport.terminal_width.saturating_sub(4).max(1));
        if self.viewport.render.markdown_cache.text == self.connection.streaming_text
            && self.viewport.render.markdown_cache.width == width
        {
            return;
        }
        self.viewport.render.markdown_cache.lines = crate::markdown::render(
            &self.connection.streaming_text,
            width,
            &self.theme,
            &self.highlighter,
        )
        .0;
        self.viewport.render.markdown_cache.text = self.connection.streaming_text.clone();
        self.viewport.render.markdown_cache.width = width;
    }

    /// Rebuild the static lines cache for finalized (committed) messages.
    ///
    /// PERF: This cache prevents re-parsing markdown for messages that haven't
    /// changed. During streaming, only the streaming section re-renders.
    /// The cache is invalidated when: message count changes, terminal width changes,
    /// or a session switch clears it.
    pub(crate) fn refresh_static_lines_cache(&mut self) {
        let inner_width = usize::from(self.viewport.terminal_width.saturating_sub(2));
        let msg_count = self.dashboard.messages.len();

        if self.viewport.render.static_message_count == msg_count
            && self.viewport.render.static_width == inner_width
        {
            return;
        }

        let agent_name: &str = self
            .dashboard
            .focused_agent
            .as_ref()
            .and_then(|id| self.dashboard.agents.iter().find(|a| a.id == *id))
            .map(|a| a.name_lower.as_str())
            .unwrap_or("assistant");

        // PERF: Only render new messages appended since the last cache build,
        // unless width changed (which requires full re-render).
        let start = if self.viewport.render.static_width == inner_width {
            self.viewport.render.static_message_count
        } else {
            self.viewport.render.static_lines.clear();
            0
        };

        let render_width = inner_width.saturating_sub(2);
        for idx in start..msg_count {
            let msg = &self.dashboard.messages[idx];
            let (role_label, role_style) = match msg.role.as_str() {
                "user" => ("you", self.theme.style_user()),
                "assistant" => (agent_name, self.theme.style_assistant()),
                _ => ("system", self.theme.style_muted()),
            };
            self.viewport
                .render
                .static_lines
                .push(ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(format!(" {role_label}"), role_style),
                ]));
            let (md_lines, _) =
                crate::markdown::render(&msg.text, render_width, &self.theme, &self.highlighter);
            for line in md_lines {
                let mut padded = vec![ratatui::text::Span::raw(" ")];
                padded.extend(line.spans);
                self.viewport
                    .render
                    .static_lines
                    .push(ratatui::text::Line::from(padded));
            }
            self.viewport
                .render
                .static_lines
                .push(ratatui::text::Line::raw(""));
        }

        self.viewport.render.static_message_count = msg_count;
        self.viewport.render.static_width = inner_width;
    }
}

#[cfg(test)]
pub(crate) mod test_helpers;

#[cfg(test)]
mod app_tests;
