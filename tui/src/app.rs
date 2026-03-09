use std::collections::{HashMap, HashSet};

use anyhow::Result;
use ratatui::Frame;
use tokio::sync::mpsc;

use crate::api::client::ApiClient;
use crate::api::sse::SseConnection;
use crate::config::Config;
use crate::events::StreamEvent;
use crate::id::{NousId, SessionId, TurnId};
use crate::msg::{ErrorToast, Msg};
use crate::theme::ThemePalette;
use crate::update::extract_text_content;
use crate::view;

use crate::state::SavedScrollState;
#[expect(
    unused_imports,
    reason = "re-exported for downstream modules that import from crate::app"
)]
pub use crate::state::{
    AgentState, AgentStatus, ChatMessage, CommandPaletteState, FilterState, InputState, Overlay,
    PlanApprovalOverlay, PlanStepApproval, SelectionContext, TabCompletion, ToolApprovalOverlay,
    ToolCallInfo,
};

// --- App ---

pub struct App {
    pub config: Config,
    pub client: ApiClient,
    pub theme: ThemePalette,
    pub highlighter: crate::highlight::Highlighter,
    pub should_quit: bool,

    // Dashboard state
    pub agents: Vec<AgentState>,
    pub focused_agent: Option<NousId>,
    pub messages: Vec<ChatMessage>,
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

    // SSE
    sse: Option<SseConnection>,
    pub sse_connected: bool,

    // Scroll
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub(crate) scroll_states: HashMap<NousId, SavedScrollState>,

    // Markdown cache — avoid re-parsing on every frame
    pub cached_markdown_text: String,
    pub cached_markdown_lines: Vec<ratatui::text::Line<'static>>,

    // Tick counter for spinner animation
    pub tick_count: u64,

    // Error toast (auto-dismiss after 5s)
    pub error_toast: Option<ErrorToast>,

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
}

impl App {
    #[tracing::instrument(skip_all, fields(url = %config.url))]
    pub async fn init(config: Config) -> Result<Self> {
        let client = ApiClient::new(&config.url, config.token.clone())?;

        let theme = ThemePalette::detect();
        tracing::info!("detected color depth: {:?}", theme.depth);

        let mut app = Self {
            config,
            client,
            theme,
            highlighter: crate::highlight::Highlighter::new(),
            should_quit: false,
            agents: Vec::new(),
            focused_agent: None,
            messages: Vec::new(),
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
            scroll_offset: 0,
            auto_scroll: true,
            scroll_states: HashMap::new(),
            cached_markdown_text: String::new(),
            cached_markdown_lines: Vec::new(),
            tick_count: 0,
            error_toast: None,
            tab_completion: None,
            terminal_width: 120,
            terminal_height: 40,
            command_palette: CommandPaletteState::default(),
            session_cost_cents: 0,
            context_usage_pct: None,
            selection: SelectionContext::default(),
            selected_message: None,
            tool_expanded: HashSet::new(),
            filter: FilterState::default(),
        };

        app.connect().await?;

        Ok(app)
    }

    #[tracing::instrument(skip(self), fields(url = %self.config.url))]
    async fn connect(&mut self) -> Result<()> {
        if !self.client.health().await.unwrap_or(false) {
            anyhow::bail!(
                "cannot reach gateway at {}. Is it running?",
                self.config.url
            );
        }

        match self.client.auth_mode().await {
            Ok(mode) => match mode.mode.as_str() {
                "none" => {
                    tracing::info!("no auth required");
                }
                "token" => {
                    if self.client.token().is_none() {
                        anyhow::bail!(
                            "gateway requires token auth. Pass --token or set ALETHEIA_TOKEN"
                        );
                    }
                }
                _ => {
                    if self.client.token().is_none() {
                        anyhow::bail!(
                            "gateway requires authentication. Pass --token or set ALETHEIA_TOKEN"
                        );
                    }
                }
            },
            Err(e) => {
                tracing::warn!("could not detect auth mode: {e}, proceeding without auth");
            }
        }

        let agents = self.client.agents().await?;
        self.agents = agents
            .into_iter()
            .map(|a| AgentState {
                id: a.id.clone(),
                name: a.display_name().to_owned(),
                emoji: a.emoji,
                status: AgentStatus::Idle,
                active_tool: None,
                tool_started_at: None,
                sessions: Vec::new(),
                model: a.model,
                compaction_stage: None,
                has_notification: false,
            })
            .collect();

        self.focused_agent = self
            .config
            .default_agent
            .clone()
            .map(NousId::from)
            .or_else(|| self.agents.first().map(|a| a.id.clone()));

        if let Some(ref agent_id) = self.focused_agent {
            if let Ok(sessions) = self.client.sessions(agent_id).await {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.sessions = sessions;
                }
            }
            self.load_focused_session().await;
        }

        if let Ok(cents) = self.client.today_cost_cents().await {
            self.daily_cost_cents = cents;
        }

        self.sse = Some(SseConnection::connect(
            &self.config.url,
            self.client.token(),
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

            if needs_load {
                if let Ok(sessions) = self.client.sessions(&agent_id).await {
                    if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
                        agent.sessions = sessions;
                    }
                }
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
                    self.messages = history
                        .into_iter()
                        .filter_map(|m| {
                            if m.role != "user" && m.role != "assistant" {
                                return None;
                            }
                            let text = extract_text_content(&m.content)?;
                            Some(ChatMessage {
                                role: m.role,
                                text,
                                timestamp: m.created_at,
                                model: m.model,
                                is_streaming: false,
                                tool_calls: Vec::new(),
                            })
                        })
                        .collect();
                    self.scroll_to_bottom();
                }
                Err(e) => {
                    tracing::error!("failed to load history: {e}");
                }
            }
        }
    }

    // --- Event source management (take/restore pattern for borrow checker) ---

    pub fn take_sse(&mut self) -> Option<SseConnection> {
        self.sse.take()
    }

    pub fn restore_sse(&mut self, sse: Option<SseConnection>) {
        self.sse = sse;
    }

    pub fn take_stream(&mut self) -> Option<mpsc::Receiver<StreamEvent>> {
        self.stream_rx.take()
    }

    pub fn restore_stream(&mut self, rx: Option<mpsc::Receiver<StreamEvent>>) {
        self.stream_rx = rx;
    }

    // --- State update ---

    pub async fn update(&mut self, msg: Msg) {
        crate::update::update(self, msg).await;
    }

    // --- View ---

    pub fn view(&self, frame: &mut Frame) {
        view::render(self, frame);
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::*;
    use std::collections::{HashMap, HashSet};

    pub fn test_app() -> App {
        let config = Config {
            url: "http://localhost:18789".to_string(),
            token: None,
            default_agent: None,
            default_session: None,
        };
        let client = ApiClient::new(&config.url, config.token.clone()).unwrap();
        let theme = ThemePalette::detect();

        App {
            config,
            client,
            theme,
            highlighter: crate::highlight::Highlighter::new(),
            should_quit: false,
            agents: Vec::new(),
            focused_agent: None,
            messages: Vec::new(),
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
            scroll_offset: 0,
            auto_scroll: true,
            scroll_states: HashMap::new(),
            cached_markdown_text: String::new(),
            cached_markdown_lines: Vec::new(),
            tick_count: 0,
            error_toast: None,
            tab_completion: None,
            terminal_width: 120,
            terminal_height: 40,
            command_palette: CommandPaletteState::default(),
            session_cost_cents: 0,
            context_usage_pct: None,
            selection: SelectionContext::default(),
            selected_message: None,
            tool_expanded: HashSet::new(),
            filter: FilterState::default(),
        }
    }

    pub fn test_app_with_messages(msgs: Vec<(&str, &str)>) -> App {
        let mut app = test_app();
        for (role, text) in msgs {
            app.messages.push(ChatMessage {
                role: role.to_string(),
                text: text.to_string(),
                timestamp: None,
                model: None,
                is_streaming: false,
                tool_calls: Vec::new(),
            });
        }
        app
    }

    pub fn test_agent(id: &str, name: &str) -> AgentState {
        AgentState {
            id: crate::id::NousId::from(id),
            name: name.to_string(),
            emoji: None,
            status: AgentStatus::Idle,
            active_tool: None,
            tool_started_at: None,
            sessions: Vec::new(),
            model: Some("test-model".to_string()),
            compaction_stage: None,
            has_notification: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;

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
        assert_eq!(app.terminal_width, 120);
        assert_eq!(app.terminal_height, 40);
    }

    #[test]
    fn test_app_with_messages_populates() {
        let app = test_app_with_messages(vec![("user", "hello"), ("assistant", "hi there")]);
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[0].role, "user");
        assert_eq!(app.messages[1].text, "hi there");
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
}
