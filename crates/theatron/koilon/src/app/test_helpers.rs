#![expect(
    clippy::expect_used,
    reason = "test helper; panics with context on impossible failures"
)]
use std::collections::{HashMap, HashSet};

use super::*;

/// Repoints an already-built test [`App`] at a local test server, replacing
/// its client the same way a real config change would.
pub(crate) fn point_app_at(app: &mut App, url: &str) {
    app.config.url = url.to_string();
    app.client = match ApiClient::new(url, None) {
        Ok(client) => client,
        // kanon:ignore RUST/expect — test helper; panics with context on impossible failures
        Err(e) => panic!("test ApiClient::new failed: {e}"),
    };
}

/// Awaits exactly one queued background task and feeds its result back
/// through [`App::update`], the same path production code takes.
pub(crate) async fn drain_one_background(app: &mut App) {
    let Some(result) = app.background_tasks.join_next().await else {
        panic!("expected one background task");
    };
    let msg = match result {
        Ok(msg) => msg,
        Err(e) => panic!("background task failed: {e}"),
    };
    app.update(msg).await;
}

pub(crate) fn test_app() -> App {
    // kanon:ignore RUST/no-silent-result-swallow — idempotent crypto-provider install in test helper
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = Config {
        url: "http://localhost:18789".to_string(), // kanon:ignore SECURITY/hardcoded-loopback-url -- test fixture, hardcoded loopback for in-process test harness // kanon:ignore SECURITY/hardcoded-loopback-url -- test fixture, hardcoded loopback for in-process test harness,
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
    // kanon:ignore RUST/expect — test helper; panics with context on impossible failures
    .expect("ApiClient::new with localhost URL should not fail");
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
            submitted_decisions: Vec::new(),
            new_session_status: ControlMutationStatus::Idle,
            agents_load_failed: false,
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
            stream_phase: crate::state::StreamPhase::Idle,
            streaming_line_buffer: String::new(),
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
            command_history: Vec::new(),
            command_history_index: None,
            selection: SelectionContext::default(),
            selected_message: None,
            tool_expanded: HashSet::new(),
            filter: FilterState::default(),
            keymap: KeyMap::build(&HashMap::new()),
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
            metrics: crate::state::MetricsState::new(),
            editor: crate::state::editor::EditorState::default(),
            pending_g: false,
            bell_enabled: false,
            notifications: NotificationStore::default(),
        },
        background_tasks: tokio::task::JoinSet::new(),
    }
}

pub(crate) fn test_app_with_messages(msgs: Vec<(&str, &str)>) -> App {
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
            kind: MessageKind::default(),
        });
    }
    app
}

pub(crate) fn test_agent(id: &str, name: &str) -> AgentState {
    let name = name.to_string();
    let name_lower = name.to_lowercase();
    AgentState {
        id: crate::id::ApiNousId::from(id),
        name,
        name_lower,
        emoji: None,
        status: AgentStatus::Idle,
        backend_health: BackendHealth::Healthy,
        active_tool: None,
        sessions: Vec::new(),
        model: Some("test-model".to_string()),
        compaction_stage: None,
        distill_completed_at: None,
        unread_count: 0,
        tools: Vec::new(),
    }
}

/// Serves `response` verbatim to every connection until dropped.
///
/// Shared plumbing for the failing/canned test servers below; tests hold the
/// returned [`tokio::task::JoinHandle`] so the listener lives for the test.
async fn raw_response_server(response: &'static str) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(e) => panic!("bind test server: {e}"),
    };
    let addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(e) => panic!("read test server address: {e}"),
    };
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _addr)) = listener.accept().await else {
                break;
            };
            let _connection = tokio::spawn(async move {
                let mut request = [0_u8; 1024];
                if stream.read(&mut request).await.is_err() {
                    return;
                }
                if let Err(e) = stream.write_all(response.as_bytes()).await {
                    tracing::debug!("failed to write test response: {e}");
                }
            });
        }
    });
    (format!("http://{addr}"), handle)
}

/// Local HTTP server that answers every request with `500 Internal Server Error`.
pub(crate) async fn failing_server() -> (String, tokio::task::JoinHandle<()>) {
    raw_response_server(concat!(
        "HTTP/1.1 500 Internal Server Error\r\n",
        "content-type: text/plain\r\n",
        "content-length: 19\r\n",
        "connection: close\r\n",
        "\r\n",
        "backend unavailable"
    ))
    .await
}

/// Local HTTP server that answers by request-path prefix with a 200 JSON
/// response; unmatched paths get a 500. Routes are `(path_prefix, json_body)`.
pub(crate) async fn routing_server(
    routes: Vec<(String, String)>,
) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(e) => panic!("bind routing test server: {e}"),
    };
    let addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(e) => panic!("read routing test server address: {e}"),
    };
    let routes = std::sync::Arc::new(routes);
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _addr)) = listener.accept().await else {
                break;
            };
            let routes = std::sync::Arc::clone(&routes);
            let _connection = tokio::spawn(async move {
                let mut request = [0_u8; 2048];
                let Ok(n) = stream.read(&mut request).await else {
                    return;
                };
                let request = String::from_utf8_lossy(request.get(..n).unwrap_or_default());
                let path = request.split_whitespace().nth(1).unwrap_or_default();
                let response = match routes
                    .iter()
                    .find(|(prefix, _body)| path.starts_with(prefix.as_str()))
                {
                    Some((_prefix, body)) => format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    ),
                    None => concat!(
                        "HTTP/1.1 500 Internal Server Error\r\n",
                        "content-type: text/plain\r\n",
                        "content-length: 19\r\n",
                        "connection: close\r\n",
                        "\r\n",
                        "backend unavailable"
                    )
                    .to_string(),
                };
                if let Err(e) = stream.write_all(response.as_bytes()).await {
                    tracing::debug!("failed to write test response: {e}");
                }
            });
        }
    });
    (format!("http://{addr}"), handle)
}
