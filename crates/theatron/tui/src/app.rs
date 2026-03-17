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
        let client = ApiClient::new(&config.url, config.token.clone())?;

        let theme = Theme::for_mode(config.theme);
        tracing::info!(depth = ?theme.depth, mode = ?theme.mode, "theme initialized");

        let command_history = load_command_history(&config);
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

        let session = if let Some(ref key) = self.config.default_session {
            agent.sessions.iter().find(|s| s.key == *key)
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
                                is_streaming: false,
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
            || self.viewport.success_toast.is_some();
        crate::update::update(self, msg).await;
        let has_animation = self.connection.active_turn_id.is_some()
            || self.viewport.error_toast.is_some()
            || self.viewport.success_toast.is_some();
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
            tab.state.streaming_text = self.connection.streaming_text.clone();
            tab.state.streaming_thinking = self.connection.streaming_thinking.clone();
            tab.state.streaming_tool_calls = self.connection.streaming_tool_calls.clone();
            tab.state.active_turn_id = self.connection.active_turn_id.clone();
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
            self.connection.streaming_text = tab.state.streaming_text.clone();
            self.connection.streaming_thinking = tab.state.streaming_thinking.clone();
            self.connection.streaming_tool_calls = tab.state.streaming_tool_calls.clone();
            self.connection.active_turn_id = tab.state.active_turn_id.clone();
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
        let width = self.viewport.terminal_width.saturating_sub(4).max(1) as usize;
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
#[expect(
    clippy::unwrap_used,
    reason = "test helper; ApiClient construction failure indicates a bug in test setup"
)]
pub(crate) mod test_helpers {
    use super::*;
    use std::collections::{HashMap, HashSet};

    pub fn test_app() -> App {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = Config {
            url: "http://localhost:18789".to_string(),
            token: None,
            default_agent: None,
            default_session: None,
            workspace_root: None,
            bell: false,
            keybindings: HashMap::new(),
            theme: None,
        };
        let client = ApiClient::new(&config.url, config.token.clone()).unwrap();
        let theme = THEME.clone();

        App {
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
                command_history: Vec::new(),
                command_history_index: None,
                selection: SelectionContext::default(),
                selected_message: None,
                tool_expanded: HashSet::new(),
                filter: FilterState::default(),
                keymap: KeyMap::build(&HashMap::new()),
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
                bell_enabled: false,
            },
        }
    }

    pub fn test_app_with_messages(msgs: Vec<(&str, &str)>) -> App {
        let mut app = test_app();
        for (role, text) in msgs {
            let text = text.to_string();
            let text_lower = text.to_lowercase();
            app.dashboard.messages.push(ChatMessage {
                role: role.to_string(),
                text,
                text_lower,
                timestamp: None,
                model: None,
                is_streaming: false,
                tool_calls: Vec::new(),
            });
        }
        app
    }

    pub fn test_agent(id: &str, name: &str) -> AgentState {
        let name = name.to_string();
        let name_lower = name.to_lowercase();
        AgentState {
            id: crate::id::NousId::from(id),
            name,
            name_lower,
            emoji: None,
            status: AgentStatus::Idle,
            active_tool: None,
            sessions: Vec::new(),
            model: Some("test-model".to_string()),
            compaction_stage: None,
            unread_count: 0,
            tools: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::{DEFAULT_TERMINAL_HEIGHT, DEFAULT_TERMINAL_WIDTH};
    use crate::state::{ChatMessage, OpsState};

    #[test]
    fn test_app_constructs_with_defaults() {
        let app = test_app();
        assert!(!app.should_quit);
        assert!(app.viewport.render.auto_scroll);
        assert!(app.layout.sidebar_visible);
        assert!(!app.layout.thinking_expanded);
        assert!(app.layout.overlay.is_none());
        assert!(app.dashboard.messages.is_empty());
        assert!(app.dashboard.agents.is_empty());
        assert_eq!(app.viewport.render.scroll_offset, 0);
        assert_eq!(app.viewport.terminal_width, DEFAULT_TERMINAL_WIDTH);
        assert_eq!(app.viewport.terminal_height, DEFAULT_TERMINAL_HEIGHT);
        assert!(!app.connection.sse_connected);
        assert!(app.connection.sse_disconnected_at.is_none());
    }

    #[test]
    fn test_app_with_messages_populates() {
        let app = test_app_with_messages(vec![("user", "hello"), ("assistant", "hi there")]);
        assert_eq!(app.dashboard.messages.len(), 2);
        assert_eq!(app.dashboard.messages[0].role, "user");
        assert_eq!(app.dashboard.messages[1].text, "hi there");
    }

    #[test]
    fn markdown_cache_fields_exist_for_session_switch_clearing() {
        // Verifies that the fields cleared on session switch are present and
        // behave as expected when the caller clears them.
        let mut app = test_app();
        app.viewport.render.markdown_cache.text = "stale content from previous session".to_string();
        app.viewport.render.markdown_cache.lines = vec![ratatui::text::Line::raw("stale line")];

        // Simulate the clearing that load_focused_session performs on history load.
        app.viewport.render.markdown_cache.clear();

        assert!(
            app.viewport.render.markdown_cache.text.is_empty(),
            "markdown text cache must be cleared on session switch"
        );
        assert!(
            app.viewport.render.markdown_cache.lines.is_empty(),
            "markdown line cache must be cleared on session switch"
        );
    }

    #[test]
    fn take_restore_sse_roundtrip() {
        let mut app = test_app();
        assert!(app.take_sse().is_none());
        app.restore_sse(None);
    }

    #[test]
    fn take_restore_stream_roundtrip() {
        let mut app = test_app();
        assert!(app.take_stream().is_none());
        app.restore_stream(None);
    }

    #[test]
    fn tab_state_save_restore_roundtrip() {
        let mut app = test_app();
        let agent = test_agent("syn", "Syn");
        let agent_id = agent.id.clone();
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some(agent_id.clone());

        // Create two tabs
        let idx0 = app.layout.tab_bar.create_tab(agent_id.clone(), "tab0");
        app.layout.tab_bar.active = idx0;

        // Set up state in tab0
        app.dashboard.messages = vec![ChatMessage {
            role: "user".to_string(),
            text: "hello from tab0".to_string(),
            text_lower: "hello from tab0".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        }]
        .into();
        app.viewport.render.scroll_offset = 42;
        app.viewport.render.auto_scroll = false;
        app.interaction.input.text = "typing in tab0".to_string();
        app.layout.ops.thinking.text = "thinking in tab0".to_string();
        app.layout
            .ops
            .push_tool_start("read_file".to_string(), None);
        app.save_to_active_tab();

        // Create tab1 with different state
        let idx1 = app.layout.tab_bar.create_tab(agent_id, "tab1");
        app.layout.tab_bar.active = idx1;
        app.dashboard.messages = vec![ChatMessage {
            role: "assistant".to_string(),
            text: "hello from tab1".to_string(),
            text_lower: "hello from tab1".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        }]
        .into();
        app.viewport.render.scroll_offset = 10;
        app.viewport.render.auto_scroll = true;
        app.interaction.input.text = "typing in tab1".to_string();
        app.layout.ops = OpsState::default();
        app.save_to_active_tab();

        // Switch back to tab0 and verify state restored
        app.layout.tab_bar.active = idx0;
        app.restore_from_active_tab();

        assert_eq!(app.dashboard.messages.len(), 1);
        assert_eq!(app.dashboard.messages[0].text, "hello from tab0");
        assert_eq!(app.viewport.render.scroll_offset, 42);
        assert!(!app.viewport.render.auto_scroll);
        assert_eq!(app.interaction.input.text, "typing in tab0");
        assert_eq!(app.layout.ops.thinking.text, "thinking in tab0");
        assert_eq!(app.layout.ops.tool_calls.len(), 1);
        assert_eq!(app.layout.ops.tool_calls[0].name, "read_file");

        // Switch to tab1 and verify its state
        app.save_to_active_tab();
        app.layout.tab_bar.active = idx1;
        app.restore_from_active_tab();

        assert_eq!(app.dashboard.messages.len(), 1);
        assert_eq!(app.dashboard.messages[0].text, "hello from tab1");
        assert_eq!(app.viewport.render.scroll_offset, 10);
        assert!(app.viewport.render.auto_scroll);
        assert_eq!(app.interaction.input.text, "typing in tab1");
        assert!(app.layout.ops.thinking.text.is_empty());
        assert!(app.layout.ops.tool_calls.is_empty());
    }

    #[test]
    fn tab_switch_messages_copy_on_write_isolated() {
        // After save_to_active_tab, the tab and the app share Arc storage.
        // A push to app.dashboard.messages triggers COW: the tab's snapshot is unaffected.
        let mut app = test_app_with_messages(vec![("user", "hello"), ("assistant", "world")]);
        let agent = test_agent("syn", "Syn");
        let agent_id = agent.id.clone();
        app.dashboard.agents.push(agent);
        app.dashboard.focused_agent = Some(agent_id.clone());

        let idx0 = app.layout.tab_bar.create_tab(agent_id, "tab0");
        app.layout.tab_bar.active = idx0;
        app.save_to_active_tab();

        // Snapshot: 2 messages in both app and tab.
        assert_eq!(app.dashboard.messages.len(), 2);
        assert_eq!(app.layout.tab_bar.tabs[0].state.messages.len(), 2);

        // Mutation diverges app from the saved snapshot.
        app.dashboard.messages.push(ChatMessage {
            role: "user".to_string(),
            text: "new".to_string(),
            text_lower: "new".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        });

        // App grew; tab snapshot is unchanged (COW semantics).
        assert_eq!(app.dashboard.messages.len(), 3);
        assert_eq!(
            app.layout.tab_bar.tabs[0].state.messages.len(),
            2,
            "tab snapshot must not be affected by app mutation"
        );
    }

    #[test]
    fn dirty_starts_true_so_first_frame_renders() {
        let app = test_app();
        assert!(
            app.viewport.dirty,
            "new App must be dirty so first frame renders"
        );
    }
}
