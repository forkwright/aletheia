#![expect(
    clippy::unwrap_used,
    reason = "test helper; ApiClient construction failure indicates a bug in test setup"
)]
use std::collections::{HashMap, HashSet};

use super::*;

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
        credential_label: crate::config::CredentialLabel::None,
    };
    let client = ApiClient::new(
        &config.url,
        config.token.as_ref().map(|t| t.expose_secret().to_owned()),
    )
    .unwrap();
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
            context_tokens_used: None,
            context_tokens_total: None,
            saved_sessions: HashMap::new(),
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
            command_history: Vec::new(),
            command_history_index: None,
            selection: SelectionContext::default(),
            selected_message: None,
            tool_expanded: HashSet::new(),
            filter: FilterState::default(),
            keymap: KeyMap::build(&HashMap::new()),
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
        distill_completed_at: None,
        unread_count: 0,
        tools: Vec::new(),
    }
}
