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

pub struct App {
    pub config: Config,
    pub client: ApiClient,
    pub theme: Theme,
    pub highlighter: crate::highlight::Highlighter,
    pub should_quit: bool,

    // Dashboard state
    pub agents: Vec<AgentState>,
    pub focused_agent: Option<NousId>,
    /// PERF: ArcVec clone is O(1): tab switches share the Arc pointer, not the Vec.
    pub messages: ArcVec<ChatMessage>,
    pub focused_session_id: Option<SessionId>,
    pub daily_cost_cents: u32,

    // Input
    pub input: InputState,

    // Layout
    pub sidebar_visible: bool,
    pub thinking_expanded: bool,

    // Overlay
    pub overlay: Option<Overlay>,

    // Streaming state
    pub active_turn_id: Option<TurnId>,
    pub streaming_text: String,
    pub streaming_thinking: String,
    pub streaming_tool_calls: Vec<ToolCallInfo>,
    pub(crate) stream_rx: Option<mpsc::Receiver<StreamEvent>>,

    // SSE connection tracking
    sse: Option<SseConnection>,
    pub sse_connected: bool,
    /// When the SSE stream last disconnected. Cleared on successful reconnect.
    pub sse_disconnected_at: Option<std::time::Instant>,
    /// Timestamp of the most recent SSE event, for connection quality indicator.
    pub sse_last_event_at: Option<std::time::Instant>,
    /// Number of SSE reconnections since startup.
    pub sse_reconnect_count: u32,

    // Scroll
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub(crate) scroll_states: HashMap<NousId, SavedScrollState>,

    // Virtual scroll: O(viewport) rendering for large message lists
    pub(crate) virtual_scroll: VirtualScroll,

    // Markdown cache: avoid re-parsing on every frame
    pub markdown_cache: MarkdownCache,

    // Tick counter for spinner animation
    pub tick_count: u64,

    // Error toast (auto-dismiss after 5s)
    pub error_toast: Option<ErrorToast>,
    // Success toast (auto-dismiss after 5s)
    pub success_toast: Option<ErrorToast>,

    // @mention tab completion state
    pub tab_completion: Option<TabCompletion>,

    // Terminal size for responsive layout
    pub terminal_width: u16,
    pub terminal_height: u16,

    // Command palette (`:` mode)
    pub command_palette: CommandPaletteState,

    // Status bar enhanced fields
    pub session_cost_cents: u32,
    pub context_usage_pct: Option<u8>,
    pub selection: SelectionContext,

    // Message selection (None = auto-scroll mode, Some(index) = message selected)
    pub selected_message: Option<usize>,
    pub tool_expanded: HashSet<crate::id::ToolId>,

    // Live filter (`/` mode)
    pub filter: FilterState,

    // Stack-based navigation (Enter drills in, Esc pops out)
    pub view_stack: ViewStack,

    // Per-view scroll state preservation
    pub(crate) view_scroll_states: HashMap<usize, SavedScrollState>,

    // Operations pane (right-side panel)
    pub ops: OpsState,

    // Multi-session tab bar
    pub(crate) tab_bar: TabBar,

    // Vim `g` prefix pending (for gt/gT two-key sequences)
    pub(crate) pending_g: bool,

    // Memory inspector panel state
    pub memory: MemoryInspectorState,

    // Persistent command history (`:` commands)
    pub command_history: Vec<String>,
    pub command_history_index: Option<usize>,

    // Configurable keymap (built at init from defaults + TOML overrides).
    pub(crate) keymap: KeyMap,

    // Terminal bell for new messages on inactive agents.
    pub(crate) bell_enabled: bool,

    // Dirty-flag rendering: true when state changed since last frame.
    // Ticks only set this when animation is in progress (streaming or toasts).
    pub(crate) dirty: bool,
    // Cached buffer from the last full render, replayed on clean frames.
    pub(crate) frame_cache: Option<Buffer>,
}

impl App {
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
            agents: Vec::new(),
            focused_agent: None,
            messages: ArcVec::default(),
            focused_session_id: None,
            daily_cost_cents: 0,
            input: InputState::default(),
            sidebar_visible: true,
            thinking_expanded: false,
            overlay: None,
            active_turn_id: None,
            streaming_text: String::new(),
            streaming_thinking: String::new(),
            streaming_tool_calls: Vec::new(),
            stream_rx: None,
            sse: None,
            sse_connected: false,
            sse_disconnected_at: None,
            sse_last_event_at: None,
            sse_reconnect_count: 0,
            scroll_offset: 0,
            auto_scroll: true,
            scroll_states: HashMap::new(),
            virtual_scroll: VirtualScroll::new(),
            markdown_cache: MarkdownCache::default(),
            tick_count: 0,
            error_toast: None,
            success_toast: None,
            tab_completion: None,
            terminal_width: DEFAULT_TERMINAL_WIDTH,
            terminal_height: DEFAULT_TERMINAL_HEIGHT,
            command_palette: CommandPaletteState::default(),
            session_cost_cents: 0,
            context_usage_pct: None,
            selection: SelectionContext::default(),
            selected_message: None,
            tool_expanded: HashSet::new(),
            filter: FilterState::default(),
            view_stack: ViewStack::new(),
            view_scroll_states: HashMap::new(),
            ops: OpsState::default(),
            tab_bar: TabBar::new(),
            pending_g: false,
            memory: MemoryInspectorState::new(),
            command_history,
            command_history_index: None,
            keymap,
            bell_enabled,
            dirty: true,
            frame_cache: None,
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
                self.error_toast = Some(ErrorToast::new(format!(
                    "Failed to load agents: {e}. Retry with :reconnect"
                )));
                Vec::new()
            }
        };
        self.agents = agents
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

        self.focused_agent = self
            .config
            .default_agent
            .clone()
            .map(NousId::from)
            .or_else(|| self.agents.first().map(|a| a.id.clone()));

        if let Some(agent_id) = self.focused_agent.clone() {
            if let Ok(sessions) = self.client.sessions(&agent_id).await
                && let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id)
            {
                agent.sessions = sessions;
            }
            self.load_focused_session().await;

            if let Ok(tools) = self.client.tools(&agent_id).await
                && let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id)
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
                .agents
                .iter()
                .find(|a| a.id == agent_id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| agent_id.to_string());
            let title = self.tab_title_for_current(&agent_name);
            let idx = self.tab_bar.create_tab(agent_id, title);
            self.tab_bar.active = idx;
            self.save_to_active_tab();
        }

        if let Ok(cents) = self.client.today_cost_cents().await {
            self.daily_cost_cents = cents;
        }

        self.sse = Some(SseConnection::connect(
            self.client.raw_client().clone(),
            &self.config.url,
        ));

        Ok(())
    }

    #[tracing::instrument(skip(self), fields(agent = ?self.focused_agent))]
    pub(crate) async fn load_focused_session(&mut self) {
        let agent_id = match &self.focused_agent {
            Some(id) => id.clone(),
            None => return,
        };

        {
            let needs_load = self
                .agents
                .iter()
                .find(|a| a.id == agent_id)
                .map(|a| a.sessions.is_empty())
                .unwrap_or(false);

            if needs_load
                && let Ok(sessions) = self.client.sessions(&agent_id).await
                && let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id)
            {
                agent.sessions = sessions;
            }
        }

        let agent = match self.agents.iter().find(|a| a.id == agent_id) {
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
            self.focused_session_id = Some(session_id.clone());

            match self.client.history(&session_id).await {
                Ok(history) => {
                    // SAFETY: sanitized at ingestion: all message fields from API.
                    self.messages = history
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
                    self.markdown_cache.clear();
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
        self.sse.take()
    }

    #[tracing::instrument(skip_all)]
    pub fn restore_sse(&mut self, sse: Option<SseConnection>) {
        self.sse = sse;
    }

    #[tracing::instrument(skip_all)]
    pub fn take_stream(&mut self) -> Option<mpsc::Receiver<StreamEvent>> {
        self.stream_rx.take()
    }

    #[tracing::instrument(skip_all)]
    pub fn restore_stream(&mut self, rx: Option<mpsc::Receiver<StreamEvent>>) {
        self.stream_rx = rx;
    }

    #[tracing::instrument(skip_all)]
    pub async fn update(&mut self, msg: Msg) {
        let is_tick = matches!(msg, Msg::Tick);
        if !is_tick {
            self.dirty = true;
            crate::update::update(self, msg).await;
            return;
        }
        // WHY: Tick fires at 60 fps even when nothing changes. Only mark dirty when
        // tick-driven animation is actually visible: streaming spinner or toast dismissal.
        let had_animation = self.active_turn_id.is_some()
            || self.error_toast.is_some()
            || self.success_toast.is_some();
        crate::update::update(self, msg).await;
        let has_animation = self.active_turn_id.is_some()
            || self.error_toast.is_some()
            || self.success_toast.is_some();
        self.dirty = had_animation || has_animation;
    }

    /// Save current app state into the active tab.
    pub(crate) fn save_to_active_tab(&mut self) {
        if let Some(tab) = self.tab_bar.active_tab_mut() {
            tab.session_id = self.focused_session_id.clone();
            tab.state.messages = self.messages.clone();
            tab.state.focused_session_id = self.focused_session_id.clone();
            tab.state.input = self.input.clone();
            tab.state.scroll = SavedScrollState {
                scroll_offset: self.scroll_offset,
                auto_scroll: self.auto_scroll,
            };
            tab.state.selected_message = self.selected_message;
            tab.state.tool_expanded = self.tool_expanded.clone();
            tab.state.filter = self.filter.clone();
            tab.state.view_stack = self.view_stack.clone();
            tab.state.streaming_text = self.streaming_text.clone();
            tab.state.streaming_thinking = self.streaming_thinking.clone();
            tab.state.streaming_tool_calls = self.streaming_tool_calls.clone();
            tab.state.active_turn_id = self.active_turn_id.clone();
            tab.state.markdown_cache = self.markdown_cache.clone();
            tab.state.ops = self.ops.clone();
        }
    }

    /// Restore app state from the active tab.
    pub(crate) fn restore_from_active_tab(&mut self) {
        if let Some(tab) = self.tab_bar.active_tab() {
            self.focused_agent = Some(tab.agent_id.clone());
            self.focused_session_id = tab.state.focused_session_id.clone();
            self.messages = tab.state.messages.clone();
            self.input = tab.state.input.clone();
            self.scroll_offset = tab.state.scroll.scroll_offset;
            self.auto_scroll = tab.state.scroll.auto_scroll;
            self.selected_message = tab.state.selected_message;
            self.tool_expanded = tab.state.tool_expanded.clone();
            self.filter = tab.state.filter.clone();
            self.view_stack = tab.state.view_stack.clone();
            self.streaming_text = tab.state.streaming_text.clone();
            self.streaming_thinking = tab.state.streaming_thinking.clone();
            self.streaming_tool_calls = tab.state.streaming_tool_calls.clone();
            self.active_turn_id = tab.state.active_turn_id.clone();
            self.markdown_cache = tab.state.markdown_cache.clone();
            self.ops = tab.state.ops.clone();
        }
    }

    /// Build a display title for the current agent+session.
    pub(crate) fn tab_title_for_current(&self, agent_name: &str) -> String {
        let session_label = self
            .focused_session_id
            .as_ref()
            .and_then(|sid| {
                self.focused_agent.as_ref().and_then(|aid| {
                    self.agents.iter().find(|a| a.id == *aid).and_then(|a| {
                        a.sessions
                            .iter()
                            .find(|s| s.id == *sid)
                            .map(|s| s.display_name.as_deref().unwrap_or(&s.key).to_string())
                    })
                })
            })
            .unwrap_or_else(|| "main".to_string());
        format!("{agent_name}: {session_label}")
    }

    /// Switch to a different tab by index, saving current and restoring target.
    pub(crate) fn switch_to_tab(&mut self, index: usize) {
        if index == self.tab_bar.active {
            return;
        }
        self.save_to_active_tab();
        if !self.tab_bar.jump_to(index) {
            return;
        }
        self.tab_bar.clear_active_unread();
        self.restore_from_active_tab();
    }

    #[tracing::instrument(skip_all)]
    pub fn view(&mut self, frame: &mut Frame) -> Vec<OscLink> {
        if !self.dirty {
            // PERF: No state changed since last frame: replay the cached buffer.
            // ratatui diffs against the previous frame, so identical content produces
            // zero terminal output. This skips all layout and widget computation.
            if let Some(ref cached) = self.frame_cache
                && cached.area == frame.area()
            {
                *frame.buffer_mut() = cached.clone();
                return Vec::new();
            }
            // Cache miss (terminal resized or first frame): fall through to full render.
        }
        let links = view::render(self, frame);
        self.frame_cache = Some(frame.buffer_mut().clone());
        self.dirty = false;
        links
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
            agents: Vec::new(),
            focused_agent: None,
            messages: ArcVec::default(),
            focused_session_id: None,
            daily_cost_cents: 0,
            input: InputState::default(),
            sidebar_visible: true,
            thinking_expanded: false,
            overlay: None,
            active_turn_id: None,
            streaming_text: String::new(),
            streaming_thinking: String::new(),
            streaming_tool_calls: Vec::new(),
            stream_rx: None,
            sse: None,
            sse_connected: false,
            sse_disconnected_at: None,
            sse_last_event_at: None,
            sse_reconnect_count: 0,
            scroll_offset: 0,
            auto_scroll: true,
            scroll_states: HashMap::new(),
            virtual_scroll: VirtualScroll::new(),
            markdown_cache: MarkdownCache::default(),
            tick_count: 0,
            error_toast: None,
            success_toast: None,
            tab_completion: None,
            terminal_width: DEFAULT_TERMINAL_WIDTH,
            terminal_height: DEFAULT_TERMINAL_HEIGHT,
            command_palette: CommandPaletteState::default(),
            session_cost_cents: 0,
            context_usage_pct: None,
            selection: SelectionContext::default(),
            selected_message: None,
            tool_expanded: HashSet::new(),
            filter: FilterState::default(),
            view_stack: ViewStack::new(),
            view_scroll_states: HashMap::new(),
            ops: OpsState::default(),
            tab_bar: TabBar::new(),
            pending_g: false,
            memory: MemoryInspectorState::new(),
            command_history: Vec::new(),
            command_history_index: None,
            keymap: KeyMap::build(&HashMap::new()),
            bell_enabled: false,
            dirty: true,
            frame_cache: None,
        }
    }

    pub fn test_app_with_messages(msgs: Vec<(&str, &str)>) -> App {
        let mut app = test_app();
        for (role, text) in msgs {
            let text = text.to_string();
            let text_lower = text.to_lowercase();
            app.messages.push(ChatMessage {
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
        assert!(app.auto_scroll);
        assert!(app.sidebar_visible);
        assert!(!app.thinking_expanded);
        assert!(app.overlay.is_none());
        assert!(app.messages.is_empty());
        assert!(app.agents.is_empty());
        assert_eq!(app.scroll_offset, 0);
        assert_eq!(app.terminal_width, DEFAULT_TERMINAL_WIDTH);
        assert_eq!(app.terminal_height, DEFAULT_TERMINAL_HEIGHT);
        assert!(!app.sse_connected);
        assert!(app.sse_disconnected_at.is_none());
    }

    #[test]
    fn test_app_with_messages_populates() {
        let app = test_app_with_messages(vec![("user", "hello"), ("assistant", "hi there")]);
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[0].role, "user");
        assert_eq!(app.messages[1].text, "hi there");
    }

    #[test]
    fn markdown_cache_fields_exist_for_session_switch_clearing() {
        // Verifies that the fields cleared on session switch are present and
        // behave as expected when the caller clears them.
        let mut app = test_app();
        app.markdown_cache.text = "stale content from previous session".to_string();
        app.markdown_cache.lines = vec![ratatui::text::Line::raw("stale line")];

        // Simulate the clearing that load_focused_session performs on history load.
        app.markdown_cache.clear();

        assert!(
            app.markdown_cache.text.is_empty(),
            "markdown text cache must be cleared on session switch"
        );
        assert!(
            app.markdown_cache.lines.is_empty(),
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
        app.agents.push(agent);
        app.focused_agent = Some(agent_id.clone());

        // Create two tabs
        let idx0 = app.tab_bar.create_tab(agent_id.clone(), "tab0");
        app.tab_bar.active = idx0;

        // Set up state in tab0
        app.messages = vec![ChatMessage {
            role: "user".to_string(),
            text: "hello from tab0".to_string(),
            text_lower: "hello from tab0".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        }]
        .into();
        app.scroll_offset = 42;
        app.auto_scroll = false;
        app.input.text = "typing in tab0".to_string();
        app.ops.thinking.text = "thinking in tab0".to_string();
        app.ops.push_tool_start("read_file".to_string(), None);
        app.save_to_active_tab();

        // Create tab1 with different state
        let idx1 = app.tab_bar.create_tab(agent_id, "tab1");
        app.tab_bar.active = idx1;
        app.messages = vec![ChatMessage {
            role: "assistant".to_string(),
            text: "hello from tab1".to_string(),
            text_lower: "hello from tab1".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        }]
        .into();
        app.scroll_offset = 10;
        app.auto_scroll = true;
        app.input.text = "typing in tab1".to_string();
        app.ops = OpsState::default();
        app.save_to_active_tab();

        // Switch back to tab0 and verify state restored
        app.tab_bar.active = idx0;
        app.restore_from_active_tab();

        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].text, "hello from tab0");
        assert_eq!(app.scroll_offset, 42);
        assert!(!app.auto_scroll);
        assert_eq!(app.input.text, "typing in tab0");
        assert_eq!(app.ops.thinking.text, "thinking in tab0");
        assert_eq!(app.ops.tool_calls.len(), 1);
        assert_eq!(app.ops.tool_calls[0].name, "read_file");

        // Switch to tab1 and verify its state
        app.save_to_active_tab();
        app.tab_bar.active = idx1;
        app.restore_from_active_tab();

        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].text, "hello from tab1");
        assert_eq!(app.scroll_offset, 10);
        assert!(app.auto_scroll);
        assert_eq!(app.input.text, "typing in tab1");
        assert!(app.ops.thinking.text.is_empty());
        assert!(app.ops.tool_calls.is_empty());
    }

    #[test]
    fn tab_switch_messages_copy_on_write_isolated() {
        // After save_to_active_tab, the tab and the app share Arc storage.
        // A push to app.messages triggers COW: the tab's snapshot is unaffected.
        let mut app = test_app_with_messages(vec![("user", "hello"), ("assistant", "world")]);
        let agent = test_agent("syn", "Syn");
        let agent_id = agent.id.clone();
        app.agents.push(agent);
        app.focused_agent = Some(agent_id.clone());

        let idx0 = app.tab_bar.create_tab(agent_id, "tab0");
        app.tab_bar.active = idx0;
        app.save_to_active_tab();

        // Snapshot: 2 messages in both app and tab.
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.tab_bar.tabs[0].state.messages.len(), 2);

        // Mutation diverges app from the saved snapshot.
        app.messages.push(ChatMessage {
            role: "user".to_string(),
            text: "new".to_string(),
            text_lower: "new".to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        });

        // App grew; tab snapshot is unchanged (COW semantics).
        assert_eq!(app.messages.len(), 3);
        assert_eq!(
            app.tab_bar.tabs[0].state.messages.len(),
            2,
            "tab snapshot must not be affected by app mutation"
        );
    }

    #[test]
    fn dirty_starts_true_so_first_frame_renders() {
        let app = test_app();
        assert!(app.dirty, "new App must be dirty so first frame renders");
    }
}
