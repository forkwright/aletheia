use std::collections::HashMap;

use anyhow::Result;
use crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::Frame;
use tokio::sync::mpsc;

use crate::api::client::ApiClient;
use crate::api::sse::SseConnection;
use crate::api::streaming;
use crate::api::types::*;
use crate::config::Config;
use crate::events::{Event, StreamEvent};
use crate::msg::{ErrorToast, Msg, OverlayKind};
use crate::theme::ThemePalette;
use crate::update::extract_text_content;
use crate::view;

#[allow(unused_imports)]
pub use crate::state::{
    AgentState, AgentStatus, ChatMessage, CommandPaletteState, InputState, Overlay,
    PlanApprovalOverlay, PlanStepApproval, SelectionContext, TabCompletion,
    ToolApprovalOverlay, ToolCallInfo,
};
use crate::state::SavedScrollState;

// --- App ---

pub struct App {
    pub config: Config,
    pub client: ApiClient,
    pub theme: ThemePalette,
    pub highlighter: crate::highlight::Highlighter,
    pub should_quit: bool,

    // Dashboard state
    pub agents: Vec<AgentState>,
    pub focused_agent: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub focused_session_id: Option<String>,
    pub daily_cost_cents: u32,

    // Input
    pub input: InputState,

    // Layout
    pub sidebar_visible: bool,
    pub thinking_expanded: bool,

    // Overlay
    pub overlay: Option<Overlay>,

    // Streaming state
    pub active_turn_id: Option<String>,
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
    scroll_states: HashMap<String, SavedScrollState>,

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
    pub active_filter: Option<String>,
    pub context_usage_pct: Option<u8>,
    pub selection: SelectionContext,
}

impl App {
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
            active_filter: Option::None,
            context_usage_pct: Option::None,
            selection: SelectionContext::default(),
        };

        app.connect().await?;

        Ok(app)
    }

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
                name: a.name,
                emoji: a.emoji,
                status: AgentStatus::Idle,
                active_tool: None,
                tool_started_at: None,
                sessions: Vec::new(),
                compaction_stage: None,
                has_notification: false,
            })
            .collect();

        self.focused_agent = self
            .config
            .default_agent
            .clone()
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

    // --- Event mapping ---

    pub fn map_event(&self, event: Event) -> Option<Msg> {
        match event {
            Event::Terminal(term_event) => self.map_terminal(term_event),
            Event::Sse(sse_event) => Some(self.map_sse(sse_event)),
            Event::Stream(stream_event) => Some(self.map_stream(stream_event)),
            Event::Tick => Some(Msg::Tick),
        }
    }

    fn map_terminal(&self, event: TermEvent) -> Option<Msg> {
        match event {
            TermEvent::Key(key) => self.map_key(key),
            TermEvent::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => Some(Msg::ScrollUp),
                MouseEventKind::ScrollDown => Some(Msg::ScrollDown),
                MouseEventKind::Down(MouseButton::Left) => {
                    let sidebar = crate::view::SIDEBAR_RECT.load_rect();
                    if sidebar.width > 0
                        && mouse.column < sidebar.x + sidebar.width
                        && mouse.row >= sidebar.y
                    {
                        let mut y = sidebar.y + 1;
                        for agent in &self.agents {
                            let row_count = if agent.active_tool.is_some()
                                || agent.compaction_stage.is_some()
                            {
                                2u16
                            } else {
                                1
                            };
                            if mouse.row >= y && mouse.row < y + row_count {
                                return Some(Msg::FocusAgent(agent.id.clone()));
                            }
                            y += row_count;
                        }
                    }
                    None
                }
                _ => None,
            },
            TermEvent::Resize(w, h) => Some(Msg::Resize(w, h)),
            _ => None,
        }
    }

    fn map_key(&self, key: KeyEvent) -> Option<Msg> {
        if self.overlay.is_some() {
            return self.map_overlay_key(key);
        }

        // Command palette intercepts all keys when active
        if self.command_palette.active {
            return self.map_palette_key(key);
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('q')) => Some(Msg::Quit),

            (KeyModifiers::CONTROL, KeyCode::Char('f')) => Some(Msg::ToggleSidebar),
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => Some(Msg::ToggleThinking),

            (_, KeyCode::F(1)) => Some(Msg::OpenOverlay(OverlayKind::Help)),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                Some(Msg::OpenOverlay(OverlayKind::AgentPicker))
            }
            (KeyModifiers::CONTROL, KeyCode::Char('i')) => {
                Some(Msg::OpenOverlay(OverlayKind::SystemStatus))
            }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => Some(Msg::NewSession),

            (_, KeyCode::Tab) => {
                if self.input.text.contains('@') {
                    Some(Msg::CharInput('\t'))
                } else {
                    None
                }
            }

            (_, KeyCode::PageUp) => Some(Msg::ScrollPageUp),
            (_, KeyCode::PageDown) => Some(Msg::ScrollPageDown),
            (KeyModifiers::SHIFT, KeyCode::Up) => Some(Msg::ScrollUp),
            (KeyModifiers::SHIFT, KeyCode::Down) => Some(Msg::ScrollDown),

            (_, KeyCode::Enter) => Some(Msg::Submit),
            (_, KeyCode::Backspace) => Some(Msg::Backspace),
            (_, KeyCode::Delete) => Some(Msg::Delete),
            (_, KeyCode::Left) => Some(Msg::CursorLeft),
            (_, KeyCode::Right) => Some(Msg::CursorRight),
            (_, KeyCode::Home) => Some(Msg::CursorHome),
            (_, KeyCode::End) => Some(Msg::CursorEnd),
            (_, KeyCode::Up) => Some(Msg::HistoryUp),
            (_, KeyCode::Down) => Some(Msg::HistoryDown),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => Some(Msg::DeleteWord),
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => Some(Msg::ClearLine),
            (KeyModifiers::CONTROL, KeyCode::Char('y')) => Some(Msg::CopyLastResponse),
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => Some(Msg::ComposeInEditor),

            // Context-aware help (only when input is empty)
            (KeyModifiers::NONE, KeyCode::Char('?')) if self.input.text.is_empty() => {
                Some(Msg::OpenOverlay(OverlayKind::Help))
            }

            // Command palette:  when input is empty and no overlay
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(':'))
                if self.input.text.is_empty() =>
            {
                Some(Msg::CommandPaletteOpen)
            }

            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::CharInput(c))
            }

            _ => None,
        }
    }

    fn map_palette_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => Some(Msg::CommandPaletteClose),
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => Some(Msg::CommandPaletteClose),
            (_, KeyCode::Enter) => Some(Msg::CommandPaletteSelect),
            (_, KeyCode::Tab) => Some(Msg::CommandPaletteTab),
            (_, KeyCode::Up) => Some(Msg::CommandPaletteUp),
            (_, KeyCode::Down) => Some(Msg::CommandPaletteDown),
            (_, KeyCode::Backspace) => Some(Msg::CommandPaletteBackspace),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => Some(Msg::CommandPaletteDeleteWord),
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => Some(Msg::CommandPaletteClose),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::CommandPaletteInput(c))
            }
            _ => None,
        }
    }

    fn map_overlay_key(&self, key: KeyEvent) -> Option<Msg> {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => Some(Msg::CloseOverlay),
            (_, KeyCode::Up) => Some(Msg::OverlayUp),
            (_, KeyCode::Down) => Some(Msg::OverlayDown),
            (_, KeyCode::Enter) => Some(Msg::OverlaySelect),

            (_, KeyCode::Char('a' | 'A')) if self.is_tool_approval_overlay() => {
                Some(Msg::OverlaySelect)
            }
            (_, KeyCode::Char('d' | 'D')) if self.is_tool_approval_overlay() => {
                Some(Msg::CloseOverlay)
            }

            (_, KeyCode::Char(' ')) if self.is_plan_approval_overlay() => {
                Some(Msg::OverlaySelect)
            }
            (_, KeyCode::Char('a' | 'A')) if self.is_plan_approval_overlay() => {
                Some(Msg::OverlaySelect)
            }
            (_, KeyCode::Char('c' | 'C')) if self.is_plan_approval_overlay() => {
                Some(Msg::CloseOverlay)
            }

            _ => None,
        }
    }

    pub(crate) fn is_tool_approval_overlay(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::ToolApproval(_)))
    }

    fn is_plan_approval_overlay(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::PlanApproval(_)))
    }

    fn map_sse(&self, event: SseEvent) -> Msg {
        match event {
            SseEvent::Connected => Msg::SseConnected,
            SseEvent::Disconnected => Msg::SseDisconnected,
            SseEvent::Init { active_turns } => Msg::SseInit { active_turns },
            SseEvent::TurnBefore {
                nous_id,
                session_id,
                turn_id,
            } => Msg::SseTurnBefore {
                nous_id,
                session_id,
                turn_id,
            },
            SseEvent::TurnAfter {
                nous_id,
                session_id,
            } => Msg::SseTurnAfter {
                nous_id,
                session_id,
            },
            SseEvent::ToolCalled { nous_id, tool_name } => {
                Msg::SseToolCalled { nous_id, tool_name }
            }
            SseEvent::ToolFailed {
                nous_id,
                tool_name,
                error,
            } => Msg::SseToolFailed {
                nous_id,
                tool_name,
                error,
            },
            SseEvent::StatusUpdate { nous_id, status } => {
                Msg::SseStatusUpdate { nous_id, status }
            }
            SseEvent::SessionCreated {
                nous_id,
                session_id,
            } => Msg::SseSessionCreated {
                nous_id,
                session_id,
            },
            SseEvent::SessionArchived {
                nous_id,
                session_id,
            } => Msg::SseSessionArchived {
                nous_id,
                session_id,
            },
            SseEvent::DistillBefore { nous_id } => Msg::SseDistillBefore { nous_id },
            SseEvent::DistillStage { nous_id, stage } => Msg::SseDistillStage { nous_id, stage },
            SseEvent::DistillAfter { nous_id } => Msg::SseDistillAfter { nous_id },
            SseEvent::Ping => Msg::Tick,
        }
    }

    fn map_stream(&self, event: StreamEvent) -> Msg {
        match event {
            StreamEvent::TurnStart {
                session_id,
                nous_id,
                turn_id,
            } => Msg::StreamTurnStart {
                session_id,
                nous_id,
                turn_id,
            },
            StreamEvent::TextDelta(text) => Msg::StreamTextDelta(text),
            StreamEvent::ThinkingDelta(text) => Msg::StreamThinkingDelta(text),
            StreamEvent::ToolStart { tool_name, tool_id } => {
                Msg::StreamToolStart { tool_name, tool_id }
            }
            StreamEvent::ToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
            } => Msg::StreamToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
            },
            StreamEvent::ToolApprovalRequired {
                turn_id,
                tool_name,
                tool_id,
                input,
                risk,
                reason,
            } => Msg::StreamToolApprovalRequired {
                turn_id,
                tool_name,
                tool_id,
                input,
                risk,
                reason,
            },
            StreamEvent::ToolApprovalResolved { tool_id, decision } => {
                Msg::StreamToolApprovalResolved { tool_id, decision }
            }
            StreamEvent::PlanProposed { plan } => Msg::StreamPlanProposed { plan },
            StreamEvent::PlanStepStart { plan_id, step_id } => {
                Msg::StreamPlanStepStart { plan_id, step_id }
            }
            StreamEvent::PlanStepComplete {
                plan_id,
                step_id,
                status,
            } => Msg::StreamPlanStepComplete {
                plan_id,
                step_id,
                status,
            },
            StreamEvent::PlanComplete { plan_id, status } => {
                Msg::StreamPlanComplete { plan_id, status }
            }
            StreamEvent::TurnComplete { outcome } => Msg::StreamTurnComplete { outcome },
            StreamEvent::TurnAbort { reason } => Msg::StreamTurnAbort { reason },
            StreamEvent::Error(msg) => Msg::StreamError(msg),
        }
    }

    // --- State update ---

    pub async fn update(&mut self, msg: Msg) {
        crate::update::update(self, msg).await;
    }

    // --- Actions ---

    pub(crate) fn send_message(&mut self, text: &str) {
        let agent_id = match &self.focused_agent {
            Some(id) => id.clone(),
            None => return,
        };

        if self.active_turn_id.is_some() {
            if let Some(ref session_id) = self.focused_session_id {
                let client = self.client.clone();
                let session_id = session_id.clone();
                let text = text.to_string();
                tokio::spawn(async move {
                    if let Err(e) = client.queue_message(&session_id, &text).await {
                        tracing::error!("failed to queue message: {e}");
                    }
                });
            }
            return;
        }

        self.messages.push(ChatMessage {
            role: "user".to_string(),
            text: text.to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
            tool_calls: Vec::new(),
        });
        self.scroll_to_bottom();

        let session_key = self
            .focused_agent
            .as_ref()
            .and_then(|id| {
                self.agents.iter().find(|a| a.id == *id).and_then(|a| {
                    self.focused_session_id.as_ref().and_then(|sid| {
                        a.sessions
                            .iter()
                            .find(|s| s.id == *sid)
                            .map(|s| s.key.clone())
                    })
                })
            })
            .unwrap_or_else(|| "main".to_string());

        let rx = streaming::stream_message(
            &self.config.url,
            self.client.token(),
            &agent_id,
            &session_key,
            text,
        );
        self.stream_rx = Some(rx);
    }

    pub(crate) fn handle_tab_completion(&mut self) {
        let text_before_cursor = &self.input.text[..self.input.cursor];
        if let Some(at_pos) = text_before_cursor.rfind('@') {
            let prefix = &text_before_cursor[at_pos + 1..];

            if let Some(ref mut tc) = self.tab_completion {
                if tc.prefix == prefix || (!tc.candidates.is_empty() && tc.insert_start == at_pos) {
                    tc.index = (tc.index + 1) % tc.candidates.len();
                    let candidate = &tc.candidates[tc.index];

                    self.input
                        .text
                        .replace_range(at_pos..self.input.cursor, &format!("@{} ", candidate));
                    self.input.cursor = at_pos + 1 + candidate.len() + 1;
                    return;
                }
            }

            let candidates: Vec<String> = self
                .agents
                .iter()
                .filter(|a| {
                    a.id.starts_with(prefix)
                        || a.name.to_lowercase().starts_with(&prefix.to_lowercase())
                })
                .map(|a| a.id.clone())
                .collect();

            if !candidates.is_empty() {
                let first = candidates[0].clone();
                self.tab_completion = Some(TabCompletion {
                    prefix: prefix.to_string(),
                    candidates,
                    index: 0,
                    insert_start: at_pos,
                });
                self.input
                    .text
                    .replace_range(at_pos..self.input.cursor, &format!("@{} ", first));
                self.input.cursor = at_pos + 1 + first.len() + 1;
            }
        }
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    pub(crate) fn save_scroll_state(&mut self) {
        if let Some(ref id) = self.focused_agent {
            self.scroll_states.insert(
                id.clone(),
                SavedScrollState {
                    scroll_offset: self.scroll_offset,
                    auto_scroll: self.auto_scroll,
                },
            );
        }
    }

    pub(crate) fn restore_scroll_state(&mut self) {
        if let Some(ref id) = self.focused_agent {
            if let Some(state) = self.scroll_states.get(id) {
                self.scroll_offset = state.scroll_offset;
                self.auto_scroll = state.auto_scroll;
            } else {
                self.scroll_to_bottom();
            }
        }
    }

    pub(crate) fn prev_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos - 1;
        while p > 0 && !self.input.text.is_char_boundary(p) {
            p -= 1;
        }
        p
    }

    pub(crate) fn next_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos + 1;
        while p < self.input.text.len() && !self.input.text.is_char_boundary(p) {
            p += 1;
        }
        p
    }

    // --- View ---

    pub fn view(&self, frame: &mut Frame) {
        view::render(self, frame);
    }
}
