use anyhow::Result;
use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
use ratatui::Frame;
use tokio::sync::mpsc;

use crate::api::client::ApiClient;
use crate::api::sse::SseConnection;
use crate::api::streaming;
use crate::api::types::*;
use crate::config::Config;
use crate::events::{Event, StreamEvent};
use crate::msg::{Msg, OverlayKind};
use crate::theme::ThemePalette;
use crate::view;

// --- Agent state ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    Streaming,
    Compacting,
}

#[derive(Debug, Clone)]
pub struct AgentState {
    pub id: String,
    pub name: String,
    pub emoji: Option<String>,
    pub status: AgentStatus,
    pub active_tool: Option<String>,
    pub tool_started_at: Option<std::time::Instant>,
    pub sessions: Vec<Session>,
    pub compaction_stage: Option<String>,
    /// Indicates this agent completed a turn while not focused.
    /// Cleared when the user switches to this agent.
    pub has_notification: bool,
}

// --- Chat message ---

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub is_streaming: bool,
}

// --- Input state ---

#[derive(Debug, Default)]
pub struct InputState {
    pub text: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
}

// --- Overlay state ---

#[derive(Debug)]
pub enum Overlay {
    Help,
    AgentPicker { cursor: usize },
    SystemStatus,
    ToolApproval(ToolApprovalOverlay),
    PlanApproval(PlanApprovalOverlay),
}

#[derive(Debug)]
pub struct ToolApprovalOverlay {
    pub turn_id: String,
    pub tool_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub risk: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct PlanApprovalOverlay {
    pub plan_id: String,
    pub steps: Vec<PlanStepApproval>,
    pub total_cost_cents: u32,
    pub cursor: usize,
}

#[derive(Debug)]
pub struct PlanStepApproval {
    pub id: u32,
    pub label: String,
    pub role: String,
    pub checked: bool,
}

// --- App ---

pub struct App {
    pub config: Config,
    pub client: ApiClient,
    pub theme: ThemePalette,
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
    stream_rx: Option<mpsc::Receiver<StreamEvent>>,

    // SSE
    sse: Option<SseConnection>,
    pub sse_connected: bool,

    // Scroll
    pub scroll_offset: usize,
    pub auto_scroll: bool,

    // Tick counter for spinner animation
    pub tick_count: u64,
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
            stream_rx: None,
            sse: None,
            sse_connected: false,
            scroll_offset: 0,
            auto_scroll: true,
            tick_count: 0,
        };

        // Connect and authenticate
        app.connect().await?;

        Ok(app)
    }

    async fn connect(&mut self) -> Result<()> {
        // Health check
        if !self.client.health().await.unwrap_or(false) {
            anyhow::bail!(
                "cannot reach gateway at {}. Is it running?",
                self.config.url
            );
        }

        // Auth mode detection
        match self.client.auth_mode().await {
            Ok(mode) => match mode.mode.as_str() {
                "none" => {
                    tracing::info!("no auth required");
                }
                "token" => {
                    if self.client.token().is_none() {
                        anyhow::bail!("gateway requires token auth. Pass --token or set ALETHEIA_TOKEN");
                    }
                }
                _ => {
                    // session/password — try login if we have no token
                    if self.client.token().is_none() {
                        anyhow::bail!(
                            "gateway requires authentication. Pass --token or set ALETHEIA_TOKEN"
                        );
                        // TODO: interactive login prompt (requires raw terminal handling before ratatui init)
                    }
                }
            },
            Err(e) => {
                tracing::warn!("could not detect auth mode: {e}, proceeding without auth");
            }
        }

        // Load agents
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

        // Focus default agent or first
        self.focused_agent = self
            .config
            .default_agent
            .clone()
            .or_else(|| self.agents.first().map(|a| a.id.clone()));

        // Load sessions for focused agent
        if let Some(ref agent_id) = self.focused_agent {
            if let Ok(sessions) = self.client.sessions(agent_id).await {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.sessions = sessions;
                }
            }
            // Load history for the first/default session
            self.load_focused_session().await;
        }

        // Load daily cost
        if let Ok(cents) = self.client.today_cost_cents().await {
            self.daily_cost_cents = cents;
        }

        // Start SSE connection
        self.sse = Some(SseConnection::connect(
            &self.config.url,
            self.client.token(),
        ));

        Ok(())
    }

    async fn load_focused_session(&mut self) {
        let agent_id = match &self.focused_agent {
            Some(id) => id.clone(),
            None => return,
        };

        let agent = match self.agents.iter().find(|a| a.id == agent_id) {
            Some(a) => a,
            None => return,
        };

        // Use explicit session, or find the most recently active primary session
        let session = if let Some(ref key) = self.config.default_session {
            agent.sessions.iter().find(|s| s.key == *key)
        } else {
            // Pick the most recently updated non-background session.
            // messageCount is unreliable after distillation (resets to post-compact count),
            // so updatedAt is the true indicator of the active conversation.
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
                            // Only show user and assistant messages in chat
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
                _ => None,
            },
            TermEvent::Resize(w, h) => Some(Msg::Resize(w, h)),
            _ => None,
        }
    }

    fn map_key(&self, key: KeyEvent) -> Option<Msg> {
        // If overlay is open, handle overlay keys
        if self.overlay.is_some() {
            return self.map_overlay_key(key);
        }

        match (key.modifiers, key.code) {
            // Quit
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::CONTROL, KeyCode::Char('q')) => Some(Msg::Quit),

            // Layout
            (KeyModifiers::CONTROL, KeyCode::Char('f')) => Some(Msg::ToggleSidebar),
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => Some(Msg::ToggleThinking),

            // Overlays
            (_, KeyCode::F(1)) => Some(Msg::OpenOverlay(OverlayKind::Help)),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                Some(Msg::OpenOverlay(OverlayKind::AgentPicker))
            }

            // Scroll
            (_, KeyCode::PageUp) => Some(Msg::ScrollPageUp),
            (_, KeyCode::PageDown) => Some(Msg::ScrollPageDown),
            (KeyModifiers::SHIFT, KeyCode::Up) => Some(Msg::ScrollUp),
            (KeyModifiers::SHIFT, KeyCode::Down) => Some(Msg::ScrollDown),

            // Input editing
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

            // Char input
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(Msg::CharInput(c))
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

            // Tool approval shortcuts
            (_, KeyCode::Char('a' | 'A')) if self.is_tool_approval_overlay() => {
                Some(Msg::OverlaySelect) // Approve
            }
            (_, KeyCode::Char('d' | 'D')) if self.is_tool_approval_overlay() => {
                Some(Msg::CloseOverlay) // Deny (handled in update)
            }

            // Plan approval shortcuts
            (_, KeyCode::Char(' ')) if self.is_plan_approval_overlay() => {
                Some(Msg::OverlaySelect) // Toggle step
            }
            (_, KeyCode::Char('a' | 'A')) if self.is_plan_approval_overlay() => {
                Some(Msg::OverlaySelect) // Approve all
            }
            (_, KeyCode::Char('c' | 'C')) if self.is_plan_approval_overlay() => {
                Some(Msg::CloseOverlay) // Cancel
            }

            _ => None,
        }
    }

    fn is_tool_approval_overlay(&self) -> bool {
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
            SseEvent::ToolCalled {
                nous_id,
                tool_name,
            } => Msg::SseToolCalled {
                nous_id,
                tool_name,
            },
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
            SseEvent::DistillStage { nous_id, stage } => {
                Msg::SseDistillStage { nous_id, stage }
            }
            SseEvent::DistillAfter { nous_id } => Msg::SseDistillAfter { nous_id },
            SseEvent::Ping => Msg::Tick, // Treat ping as a tick
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
            StreamEvent::ToolStart {
                tool_name,
                tool_id,
            } => Msg::StreamToolStart {
                tool_name,
                tool_id,
            },
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
            StreamEvent::ToolApprovalResolved {
                tool_id,
                decision,
            } => Msg::StreamToolApprovalResolved {
                tool_id,
                decision,
            },
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
        match msg {
            // --- Input ---
            Msg::CharInput(c) => {
                self.input.text.insert(self.input.cursor, c);
                self.input.cursor += c.len_utf8();
                self.input.history_index = None;
            }
            Msg::Backspace => {
                if self.input.cursor > 0 {
                    let prev = self.prev_char_boundary(self.input.cursor);
                    self.input.text.drain(prev..self.input.cursor);
                    self.input.cursor = prev;
                }
            }
            Msg::Delete => {
                if self.input.cursor < self.input.text.len() {
                    let next = self.next_char_boundary(self.input.cursor);
                    self.input.text.drain(self.input.cursor..next);
                }
            }
            Msg::CursorLeft => {
                if self.input.cursor > 0 {
                    self.input.cursor = self.prev_char_boundary(self.input.cursor);
                }
            }
            Msg::CursorRight => {
                if self.input.cursor < self.input.text.len() {
                    self.input.cursor = self.next_char_boundary(self.input.cursor);
                }
            }
            Msg::CursorHome => self.input.cursor = 0,
            Msg::CursorEnd => self.input.cursor = self.input.text.len(),
            Msg::DeleteWord => {
                // Delete back to previous word boundary
                let mut pos = self.input.cursor;
                // Skip trailing spaces
                while pos > 0 && self.input.text.as_bytes().get(pos - 1) == Some(&b' ') {
                    pos -= 1;
                }
                // Skip word chars
                while pos > 0 && self.input.text.as_bytes().get(pos - 1) != Some(&b' ') {
                    pos -= 1;
                }
                self.input.text.drain(pos..self.input.cursor);
                self.input.cursor = pos;
            }
            Msg::ClearLine => {
                self.input.text.clear();
                self.input.cursor = 0;
            }
            Msg::HistoryUp => {
                if !self.input.history.is_empty() {
                    let idx = match self.input.history_index {
                        Some(i) if i + 1 < self.input.history.len() => i + 1,
                        None => 0,
                        Some(i) => i,
                    };
                    self.input.history_index = Some(idx);
                    self.input.text =
                        self.input.history[self.input.history.len() - 1 - idx].clone();
                    self.input.cursor = self.input.text.len();
                }
            }
            Msg::HistoryDown => {
                match self.input.history_index {
                    Some(0) => {
                        self.input.history_index = None;
                        self.input.text.clear();
                        self.input.cursor = 0;
                    }
                    Some(i) => {
                        let idx = i - 1;
                        self.input.history_index = Some(idx);
                        self.input.text =
                            self.input.history[self.input.history.len() - 1 - idx].clone();
                        self.input.cursor = self.input.text.len();
                    }
                    None => {}
                }
            }
            Msg::Submit => {
                let text = self.input.text.trim().to_string();
                if text.is_empty() {
                    return;
                }
                self.input.history.push(text.clone());
                self.input.text.clear();
                self.input.cursor = 0;
                self.input.history_index = None;
                self.send_message(&text);
            }

            // --- Navigation ---
            Msg::ScrollUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(3);
                self.auto_scroll = false;
            }
            Msg::ScrollDown => {
                if self.scroll_offset >= 3 {
                    self.scroll_offset -= 3;
                } else {
                    self.scroll_offset = 0;
                    self.auto_scroll = true;
                }
            }
            Msg::ScrollPageUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(20);
                self.auto_scroll = false;
            }
            Msg::ScrollPageDown => {
                if self.scroll_offset >= 20 {
                    self.scroll_offset -= 20;
                } else {
                    self.scroll_offset = 0;
                    self.auto_scroll = true;
                }
            }
            Msg::ScrollToBottom => {
                self.scroll_offset = 0;
                self.auto_scroll = true;
            }
            Msg::FocusAgent(id) => {
                // Clear notification on the agent we're switching to
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == id) {
                    agent.has_notification = false;
                }
                self.focused_agent = Some(id.clone());
                self.load_focused_session().await;
            }
            Msg::NextAgent => {
                if let Some(ref current) = self.focused_agent {
                    if let Some(idx) = self.agents.iter().position(|a| a.id == *current) {
                        let next = (idx + 1) % self.agents.len();
                        let id = self.agents[next].id.clone();
                        self.focused_agent = Some(id);
                        self.load_focused_session().await;
                    }
                }
            }
            Msg::PrevAgent => {
                if let Some(ref current) = self.focused_agent {
                    if let Some(idx) = self.agents.iter().position(|a| a.id == *current) {
                        let prev = if idx == 0 {
                            self.agents.len() - 1
                        } else {
                            idx - 1
                        };
                        let id = self.agents[prev].id.clone();
                        self.focused_agent = Some(id);
                        self.load_focused_session().await;
                    }
                }
            }

            // --- Layout ---
            Msg::ToggleSidebar => self.sidebar_visible = !self.sidebar_visible,
            Msg::ToggleThinking => self.thinking_expanded = !self.thinking_expanded,
            Msg::OpenOverlay(kind) => {
                self.overlay = Some(match kind {
                    OverlayKind::Help => Overlay::Help,
                    OverlayKind::AgentPicker => Overlay::AgentPicker { cursor: 0 },
                    OverlayKind::SystemStatus => Overlay::SystemStatus,
                });
            }
            Msg::CloseOverlay => {
                // If denying a tool approval, send the deny
                if let Some(Overlay::ToolApproval(ref approval)) = self.overlay {
                    let turn_id = approval.turn_id.clone();
                    let tool_id = approval.tool_id.clone();
                    let client = self.client.clone();
                    tokio::spawn(async move {
                        if let Err(e) = client.deny_tool(&turn_id, &tool_id).await {
                            tracing::error!("failed to deny tool: {e}");
                        }
                    });
                }
                // If cancelling a plan, send the cancel
                if let Some(Overlay::PlanApproval(ref plan)) = self.overlay {
                    let plan_id = plan.plan_id.clone();
                    let client = self.client.clone();
                    tokio::spawn(async move {
                        if let Err(e) = client.cancel_plan(&plan_id).await {
                            tracing::error!("failed to cancel plan: {e}");
                        }
                    });
                }
                self.overlay = None;
            }
            Msg::Resize(_, _) => {} // ratatui handles this

            // --- Overlay interaction ---
            Msg::OverlayUp => match &mut self.overlay {
                Some(Overlay::AgentPicker { cursor }) => {
                    *cursor = cursor.saturating_sub(1);
                }
                Some(Overlay::PlanApproval(plan)) => {
                    plan.cursor = plan.cursor.saturating_sub(1);
                }
                _ => {}
            },
            Msg::OverlayDown => match &mut self.overlay {
                Some(Overlay::AgentPicker { cursor }) => {
                    let max = self.agents.len().saturating_sub(1);
                    *cursor = (*cursor + 1).min(max);
                }
                Some(Overlay::PlanApproval(plan)) => {
                    let max = plan.steps.len().saturating_sub(1);
                    plan.cursor = (plan.cursor + 1).min(max);
                }
                _ => {}
            },
            Msg::OverlaySelect => {
                match &self.overlay {
                    Some(Overlay::AgentPicker { cursor }) => {
                        if let Some(agent) = self.agents.get_mut(*cursor) {
                            agent.has_notification = false;
                            let id = agent.id.clone();
                            self.focused_agent = Some(id);
                            self.overlay = None;
                            self.load_focused_session().await;
                        }
                    }
                    Some(Overlay::ToolApproval(approval)) => {
                        let turn_id = approval.turn_id.clone();
                        let tool_id = approval.tool_id.clone();
                        let client = self.client.clone();
                        tokio::spawn(async move {
                            if let Err(e) = client.approve_tool(&turn_id, &tool_id).await {
                                tracing::error!("failed to approve tool: {e}");
                            }
                        });
                        self.overlay = None;
                    }
                    Some(Overlay::PlanApproval(plan)) => {
                        // Space toggles, A approves
                        // For now, treat select as approve-all
                        let plan_id = plan.plan_id.clone();
                        let client = self.client.clone();
                        tokio::spawn(async move {
                            if let Err(e) = client.approve_plan(&plan_id).await {
                                tracing::error!("failed to approve plan: {e}");
                            }
                        });
                        self.overlay = None;
                    }
                    _ => {
                        self.overlay = None;
                    }
                }
            }
            Msg::OverlayFilter(_) | Msg::OverlayFilterBackspace => {
                // TODO: fuzzy filter in Phase 2
            }

            // --- SSE ---
            Msg::SseConnected => {
                self.sse_connected = true;
            }
            Msg::SseDisconnected => {
                self.sse_connected = false;
            }
            Msg::SseInit { active_turns } => {
                for turn in active_turns {
                    if let Some(agent) = self.agents.iter_mut().find(|a| a.id == turn.nous_id) {
                        agent.status = AgentStatus::Working;
                    }
                }
            }
            Msg::SseTurnBefore { nous_id, .. } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.status = AgentStatus::Working;
                    agent.active_tool = None;
                }
            }
            Msg::SseTurnAfter {
                nous_id,
                session_id,
            } => {
                let is_focused = self.focused_agent.as_deref() == Some(&nous_id);
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.status = AgentStatus::Idle;
                    agent.active_tool = None;
                    agent.tool_started_at = None;
                    // Set notification if this agent isn't currently focused
                    if !is_focused {
                        agent.has_notification = true;
                    }
                }
                // Reload history if this is our focused agent/session
                if is_focused {
                    if self.focused_session_id.as_deref() == Some(&session_id) {
                        // Only reload if we're not currently streaming (we already have the data)
                        if self.active_turn_id.is_none() {
                            self.load_focused_session().await;
                        }
                    }
                }
            }
            Msg::SseToolCalled {
                nous_id,
                tool_name,
            } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.active_tool = Some(tool_name);
                    agent.tool_started_at = Some(std::time::Instant::now());
                }
            }
            Msg::SseToolFailed { nous_id, .. } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.active_tool = None;
                    agent.tool_started_at = None;
                }
            }
            Msg::SseStatusUpdate { nous_id, status } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.status = match status.as_str() {
                        "working" => AgentStatus::Working,
                        "streaming" => AgentStatus::Streaming,
                        "compacting" => AgentStatus::Compacting,
                        _ => AgentStatus::Idle,
                    };
                }
            }
            Msg::SseSessionCreated { nous_id, .. } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    // Reload sessions for this agent
                    if let Ok(sessions) = self.client.sessions(&nous_id).await {
                        agent.sessions = sessions;
                    }
                }
            }
            Msg::SseSessionArchived { nous_id, session_id } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.sessions.retain(|s| s.id != session_id);
                }
            }
            Msg::SseDistillBefore { nous_id } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.status = AgentStatus::Compacting;
                    agent.compaction_stage = Some("starting".to_string());
                }
            }
            Msg::SseDistillStage { nous_id, stage } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.compaction_stage = Some(stage);
                }
            }
            Msg::SseDistillAfter { nous_id } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.status = AgentStatus::Idle;
                    agent.compaction_stage = None;
                }
                // Reload history if focused
                if self.focused_agent.as_deref() == Some(&nous_id) {
                    self.load_focused_session().await;
                }
            }

            // --- Streaming ---
            Msg::StreamTurnStart { turn_id, nous_id, .. } => {
                self.active_turn_id = Some(turn_id);
                self.streaming_text.clear();
                self.streaming_thinking.clear();
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.status = AgentStatus::Streaming;
                }
            }
            Msg::StreamTextDelta(text) => {
                self.streaming_text.push_str(&text);
                if self.auto_scroll {
                    self.scroll_offset = 0;
                }
            }
            Msg::StreamThinkingDelta(text) => {
                self.streaming_thinking.push_str(&text);
            }
            Msg::StreamToolStart { tool_name, .. } => {
                if let Some(ref agent_id) = self.focused_agent {
                    if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                        agent.active_tool = Some(tool_name);
                        agent.tool_started_at = Some(std::time::Instant::now());
                    }
                }
            }
            Msg::StreamToolResult { .. } => {
                if let Some(ref agent_id) = self.focused_agent {
                    if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                        agent.active_tool = None;
                        agent.tool_started_at = None;
                    }
                }
            }
            Msg::StreamToolApprovalRequired {
                turn_id,
                tool_name,
                tool_id,
                input,
                risk,
                reason,
            } => {
                self.overlay = Some(Overlay::ToolApproval(ToolApprovalOverlay {
                    turn_id,
                    tool_id,
                    tool_name,
                    input,
                    risk,
                    reason,
                }));
            }
            Msg::StreamToolApprovalResolved { .. } => {
                if self.is_tool_approval_overlay() {
                    self.overlay = None;
                }
            }
            Msg::StreamPlanProposed { plan } => {
                self.overlay = Some(Overlay::PlanApproval(PlanApprovalOverlay {
                    plan_id: plan.id,
                    total_cost_cents: plan.total_estimated_cost_cents,
                    cursor: 0,
                    steps: plan
                        .steps
                        .into_iter()
                        .map(|s| PlanStepApproval {
                            id: s.id,
                            label: s.label,
                            role: s.role,
                            checked: true,
                        })
                        .collect(),
                }));
            }
            Msg::StreamPlanStepStart { .. }
            | Msg::StreamPlanStepComplete { .. }
            | Msg::StreamPlanComplete { .. } => {
                // TODO: update plan progress widget
            }
            Msg::StreamTurnComplete { outcome } => {
                // Finalize the streamed message
                if !self.streaming_text.is_empty() {
                    self.messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        text: self.streaming_text.clone(),
                        timestamp: None,
                        model: Some(outcome.model),
                        is_streaming: false,
                    });
                }
                self.streaming_text.clear();
                self.streaming_thinking.clear();
                self.active_turn_id = None;
                self.stream_rx = None;
                if let Some(ref agent_id) = self.focused_agent {
                    if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                        agent.status = AgentStatus::Idle;
                        agent.active_tool = None;
                    }
                }
                // Update cost
                if let Ok(cents) = self.client.today_cost_cents().await {
                    self.daily_cost_cents = cents;
                }
            }
            Msg::StreamTurnAbort { reason } => {
                tracing::info!("turn aborted: {reason}");
                self.streaming_text.clear();
                self.streaming_thinking.clear();
                self.active_turn_id = None;
                self.stream_rx = None;
            }
            Msg::StreamError(msg) => {
                tracing::error!("stream error: {msg}");
                self.active_turn_id = None;
                self.stream_rx = None;
            }

            // --- API responses ---
            Msg::AgentsLoaded(agents) => {
                self.agents = agents
                    .into_iter()
                    .map(|a| AgentState {
                        id: a.id,
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
            }
            Msg::SessionsLoaded { nous_id, sessions } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == nous_id) {
                    agent.sessions = sessions;
                }
            }
            Msg::HistoryLoaded { messages, .. } => {
                self.messages = messages
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
                        })
                    })
                    .collect();
                self.scroll_to_bottom();
            }
            Msg::CostLoaded { daily_total_cents } => {
                self.daily_cost_cents = daily_total_cents;
            }
            Msg::AuthResult(_) | Msg::ApiError(_) => {}

            // --- Quit ---
            Msg::Quit => self.should_quit = true,

            // --- Tick ---
            Msg::Tick => {
                self.tick_count = self.tick_count.wrapping_add(1);
            }
        }
    }

    // --- Actions ---

    fn send_message(&mut self, text: &str) {
        let agent_id = match &self.focused_agent {
            Some(id) => id.clone(),
            None => return,
        };

        // If there's already an active turn, queue the message instead
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

        // Add user message to display
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            text: text.to_string(),
            timestamp: None,
            model: None,
            is_streaming: false,
        });
        self.scroll_to_bottom();

        // Determine session key
        let session_key = self
            .focused_agent
            .as_ref()
            .and_then(|id| {
                self.agents
                    .iter()
                    .find(|a| a.id == *id)
                    .and_then(|a| {
                        self.focused_session_id.as_ref().and_then(|sid| {
                            a.sessions.iter().find(|s| s.id == *sid).map(|s| s.key.clone())
                        })
                    })
            })
            .unwrap_or_else(|| "main".to_string());

        // Start streaming response
        let rx = streaming::stream_message(
            &self.config.url,
            self.client.token(),
            &agent_id,
            &session_key,
            text,
        );
        self.stream_rx = Some(rx);
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    fn prev_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos - 1;
        while p > 0 && !self.input.text.is_char_boundary(p) {
            p -= 1;
        }
        p
    }

    fn next_char_boundary(&self, pos: usize) -> usize {
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


/// Extract only text content from a JSON array of Anthropic content blocks.
/// Tool_use blocks are silently skipped — if a message is purely tool calls,
/// this returns None and the message is filtered out of the chat view.
fn extract_texts_from_array(arr: &[serde_json::Value]) -> Option<String> {
    let mut texts = Vec::new();

    for block in arr {
        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                if !t.is_empty() {
                    texts.push(t.to_string());
                }
            }
        }
    }

    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

/// Extract displayable text from a history message's content field.
/// Content can be: a plain string, a stringified JSON array, a JSON array, or null.
fn extract_text_content(content: &Option<serde_json::Value>) -> Option<String> {
    let content = content.as_ref()?;

    // Plain string content — but might be a stringified JSON array
    if let Some(s) = content.as_str() {
        if s.is_empty() {
            return None;
        }
        // Try parsing as JSON array (double-stringified content from API)
        if s.starts_with('[') {
            if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(s) {
                return extract_texts_from_array(&parsed);
            }
        }
        return Some(s.to_string());
    }

    // Already a JSON array of content blocks
    if let Some(arr) = content.as_array() {
        return extract_texts_from_array(arr);
    }

    None
}
