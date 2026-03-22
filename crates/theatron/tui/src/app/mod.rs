use std::collections::{HashMap, HashSet};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use tokio::sync::mpsc;

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
#[cfg(test)]
use crate::theme::THEME;
use crate::theme::Theme;
use crate::update::extract_text_content;
use crate::view;

use crate::state::ArcVec;
use crate::state::MarkdownCache;
use crate::state::SavedScrollState;
use crate::state::TabBar;
use crate::state::virtual_scroll::VirtualScroll;
#[expect(
    unused_imports,
    reason = "re-exported for downstream modules that import from crate::app"
)]
pub use crate::state::{
    ActiveTool, AgentState, AgentStatus, ChatMessage, CommandPaletteState, ContextAction,
    ContextActionsOverlay, FilterState, FocusedPane, InputState, MemoryInspectorState, OpsState,
    Overlay, PlanApprovalOverlay, PlanStepApproval, SelectionContext, SessionPickerOverlay,
    TabCompletion, ToolApprovalOverlay, ToolCallInfo, ToolSummary, View, ViewStack,
};

/// Default terminal width used before the first resize event arrives.
const DEFAULT_TERMINAL_WIDTH: u16 = 120;
/// Default terminal height used before the first resize event arrives.
const DEFAULT_TERMINAL_HEIGHT: u16 = 40;

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
    /// Timestamp of the last stream event received during the current turn.
    /// Used for stall detection.
    pub(crate) stream_last_event_at: Option<std::time::Instant>,
    /// Set once the 30s stall warning has been shown for the current turn.
    pub(crate) stall_warned: bool,
    /// Non-dismissing status message shown during stall conditions.
    pub(crate) stall_message: Option<String>,
}

/// Scroll position, virtual scroll index, and markdown render cache.
pub struct RenderState {
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub(crate) scroll_states: HashMap<NousId, SavedScrollState>,
    pub(crate) virtual_scroll: VirtualScroll,
    pub markdown_cache: MarkdownCache,
}

/// Terminal dimensions, tick counter, dirty flag, frame cache, and render state.
pub struct ViewportState {
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub tick_count: u64,
    pub error_toast: Option<ErrorToast>,
    pub success_toast: Option<ErrorToast>,
    pub(crate) dirty: bool,
    pub(crate) frame_cache: Option<Buffer>,
    pub render: RenderState,
}

/// Input, tab completion, command palette, selection, filter, and key state.
pub struct InteractionState {
    pub input: InputState,
    pub tab_completion: Option<TabCompletion>,
    pub command_palette: CommandPaletteState,
    pub command_history: Vec<String>,
    pub command_history_index: Option<usize>,
    pub selection: SelectionContext,
    pub selected_message: Option<usize>,
    pub tool_expanded: HashSet<crate::id::ToolId>,
    pub filter: FilterState,
    pub(crate) keymap: KeyMap,
    /// Tool names that bypass the approval dialog for the lifetime of this TUI session.
    pub(crate) always_allowed_tools: HashSet<String>,
}

/// Sidebar, overlay, view stack, ops, tabs, and memory inspector.
pub struct LayoutState {
    pub sidebar_visible: bool,
    pub thinking_expanded: bool,
    pub overlay: Option<Overlay>,
    pub view_stack: ViewStack,
    pub(crate) view_scroll_states: HashMap<usize, SavedScrollState>,
    pub ops: OpsState,
    pub(crate) tab_bar: TabBar,
    pub memory: MemoryInspectorState,
    pub(crate) pending_g: bool,
    pub(crate) bell_enabled: bool,
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
                stream_last_event_at: None,
                stall_warned: false,
                stall_message: None,
            },
            viewport: ViewportState {
                terminal_width: DEFAULT_TERMINAL_WIDTH,
                terminal_height: DEFAULT_TERMINAL_HEIGHT,
                tick_count: 0,
                error_toast: None,
                success_toast: None,
                dirty: true,
                frame_cache: None,
                render: RenderState {
                    scroll_offset: 0,
                    auto_scroll: true,
                    scroll_states: HashMap::new(),
                    virtual_scroll: VirtualScroll::new(),
                    markdown_cache: MarkdownCache::default(),
                },
            },
            interaction: InteractionState {
                input: InputState::default(),
                tab_completion: None,
                command_palette: CommandPaletteState::default(),
                command_history,
                command_history_index: None,
                selection: SelectionContext::default(),
                selected_message: None,
                tool_expanded: HashSet::new(),
                filter: FilterState::default(),
                keymap,
                always_allowed_tools: HashSet::new(),
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
                pending_g: false,
                bell_enabled,
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
                            })
                        })
                        .collect();
                    // Stale streaming markdown from the previous session must not
                    // bleed through when the user switches agents.
                    self.viewport.render.markdown_cache.clear();
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
            || self.connection.stall_message.is_some();
        crate::update::update(self, msg).await;
        let has_animation = self.connection.active_turn_id.is_some()
            || self.viewport.error_toast.is_some()
            || self.viewport.success_toast.is_some()
            || self.connection.stall_message.is_some();
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
        if !self.viewport.dirty {
            // PERF: No state changed since last frame: replay the cached buffer.
            // ratatui diffs against the previous frame, so identical content produces
            // zero terminal output. This skips all layout and widget computation.
            if let Some(ref cached) = self.viewport.frame_cache
                && cached.area == frame.area()
            {
                *frame.buffer_mut() = cached.clone();
                return Vec::new();
            }
            // Cache miss (terminal resized or first frame): fall through to full render.
        }
        // PERF: Refresh the streaming markdown cache once per frame instead of on
        // every text delta. Multiple deltas arriving between frames are batched
        // into a single markdown::render call, reducing CPU from O(tokens) to O(frames).
        self.refresh_streaming_markdown_cache();
        let links = view::render(self, frame);
        self.viewport.frame_cache = Some(frame.buffer_mut().clone());
        self.viewport.dirty = false;
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
}

pub(crate) const MAX_COMMAND_HISTORY: usize = 1000;

fn history_file_path(config: &Config) -> Option<std::path::PathBuf> {
    config
        .workspace_root
        .as_ref()
        .map(|root| root.join("state").join("tui_history"))
}

fn load_command_history(config: &Config) -> Vec<String> {
    let path = match history_file_path(config) {
        Some(p) => p,
        None => return Vec::new(),
    };
    match std::fs::read_to_string(&path) {
        Ok(contents) => contents
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub(crate) fn save_command_history(config: &Config, history: &[String]) {
    let path = match history_file_path(config) {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content: String = history.iter().map(|s| format!("{s}\n")).collect();
    #[expect(
        clippy::disallowed_methods,
        reason = "theatron TUI reads configuration and exports from disk in synchronous initialization paths"
    )]
    let _ = std::fs::write(&path, content);
}

fn session_state_file_path(config: &Config) -> Option<std::path::PathBuf> {
    config
        .workspace_root
        .as_ref()
        .map(|root| root.join("state").join("tui_sessions"))
}

/// Load the per-agent last-active session map from disk.
///
/// Format: one entry per line, `<agent_id>:<session_id>`.
/// Malformed or empty lines are silently skipped.
fn load_session_state(config: &Config) -> HashMap<NousId, SessionId> {
    let path = match session_state_file_path(config) {
        Some(p) => p,
        None => return HashMap::new(),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    contents
        .lines()
        .filter_map(|line| {
            let (agent, session) = line.split_once(':')?;
            if agent.is_empty() || session.is_empty() {
                return None;
            }
            Some((NousId::from(agent), SessionId::from(session)))
        })
        .collect()
}

/// Persist the per-agent last-active session map to disk.
///
/// Uses sync I/O because this runs in a synchronous TUI shutdown path
/// where spawning an async task would require a runtime handle that may
/// already be shutting down.
pub(crate) fn save_session_state(config: &Config, sessions: &HashMap<NousId, SessionId>) {
    let path = match session_state_file_path(config) {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content: String = sessions
        .iter()
        .map(|(agent, session)| format!("{agent}:{session}\n"))
        .collect();
    #[expect(
        clippy::disallowed_methods,
        reason = "synchronous write is intentional in TUI shutdown path"
    )]
    let _ = std::fs::write(&path, content);
}

/// Resolve the root directory for export files.
pub(crate) fn exports_dir(config: &Config) -> std::path::PathBuf {
    config
        .workspace_root
        .as_ref()
        .map(|root| root.join("exports"))
        .unwrap_or_else(|| std::path::PathBuf::from("exports"))
}

#[cfg(test)]
pub(crate) mod test_helpers;

#[cfg(test)]
mod app_tests;
