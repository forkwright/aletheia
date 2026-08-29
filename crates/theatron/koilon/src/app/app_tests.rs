#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing and contextual panics"
)]
mod tests {
    use super::super::test_helpers::*;
    use super::super::{App, DEFAULT_TERMINAL_HEIGHT, DEFAULT_TERMINAL_WIDTH};
    use crate::config::{Config, CredentialLabel};
    use crate::state::{ChatMessage, OpsState};
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn health_body(status: &str, check_status: &str) -> String {
        serde_json::json!({
            "status": status,
            "version": "0.13.1",
            "git_sha": "abc123",
            "uptime_seconds": 300,
            "checks": [
                {"name": "providers", "status": check_status, "message": "provider offline"}
            ],
            "data_dir": "/tmp/data"
        })
        .to_string()
    }

    async fn spawn_startup_server(health_status: u16, health_body: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local startup test server");
        let addr = listener
            .local_addr()
            .expect("read startup test server address");

        tokio::spawn(async move {
            for _ in 0..2 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0_u8; 4096];
                let read = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                // WHY: `App::init` -> `connect()` fetches the operator-only
                // detailed report via `health_details()`, which hits
                // `/api/v1/system/health` -- not the unauthenticated
                // `/api/health` liveness probe (`{"status"}` only, no
                // `checks`). Routing the detailed body here mirrors what the
                // production client actually calls.
                let (status, reason, body) = if path == "/api/v1/system/health" {
                    (health_status, "Service Unavailable", health_body.as_str())
                } else if path == "/api/v1/nous" {
                    (200, "OK", r#"{"nous":[]}"#)
                } else {
                    (404, "Not Found", r#"{"error":"not found"}"#)
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        format!("http://{addr}")
    }

    /// A minimal test server for `:reauth` (#6818): `reauthenticate` only
    /// probes `/api/v1/system/health` (it deliberately never calls
    /// `connect()`'s full `/api/v1/nous` startup sequence), so this serves
    /// exactly that route on an open-ended accept loop rather than
    /// `spawn_startup_server`'s fixed two-request/two-route shape.
    async fn spawn_health_only_server(status: u16, reason: &'static str, body: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind reauth test server");
        let addr = listener
            .local_addr()
            .expect("read reauth test server address");

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = [0_u8; 4096];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        format!("http://{addr}")
    }

    fn config_for_url(url: String) -> Config {
        Config {
            url,
            token: None,
            default_agent: None,
            default_session: None,
            workspace_root: None,
            bell: false,
            keybindings: HashMap::new(),
            theme: None,
            credential_label: CredentialLabel::None,
        }
    }

    #[test]
    fn app_constructs_with_defaults() {
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

    #[tokio::test]
    async fn init_accepts_503_health_body_and_keeps_check_list() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let url = spawn_startup_server(503, health_body("unhealthy", "fail")).await;
        let app = App::init(config_for_url(url))
            .await
            .expect("503 with health body must not abort startup");

        let health = app
            .layout
            .metrics
            .health
            .expect("startup should retain parsed health response");
        assert_eq!(health.status, "unhealthy");
        assert_eq!(health.checks[0].name, "providers");
        assert_eq!(health.checks[0].status, "fail");
        assert_eq!(app.layout.metrics.api_healthy, Some(false));
    }

    #[test]
    fn app_with_messages_populates_dashboard_correctly() {
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
            tool_calls: Vec::new(),
            kind: crate::state::MessageKind::default(),
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
            tool_calls: Vec::new(),
            kind: crate::state::MessageKind::default(),
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
            tool_calls: Vec::new(),
            kind: crate::state::MessageKind::default(),
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

    #[tokio::test]
    async fn reauthenticate_rejects_a_blank_token_before_any_network_call() {
        let mut app = test_app();
        let err = app
            .reauthenticate("   ")
            .await
            .expect_err("a blank token must be rejected");
        assert!(
            matches!(err, crate::error::Error::TokenRequired { .. }),
            "expected TokenRequired, got {err:?}"
        );
        assert!(
            app.config.token.is_none(),
            "rejecting a blank token must not touch the existing credential"
        );
    }

    #[tokio::test]
    async fn reauthenticate_surfaces_auth_rejected_and_leaves_config_untouched_on_401() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let url = spawn_health_only_server(
            401,
            "Unauthorized",
            r#"{"error":{"code":"auth_failed","message":"invalid token"}}"#.to_string(),
        )
        .await;
        let mut app = test_app();
        app.config.url = url;

        let err = app
            .reauthenticate("still-bad-token")
            .await
            .expect_err("a 401 from the verification probe must fail reauthenticate");
        assert!(
            matches!(err, crate::error::Error::AuthRejected { .. }),
            "expected AuthRejected (#6818), got {err:?}"
        );
        // WHY(#6818): reauthenticate verifies the candidate BEFORE
        // committing it — a mistyped/still-wrong token must not replace
        // whatever credential (however broken) was already in place.
        assert!(
            app.config.token.is_none(),
            "a candidate token that fails verification must not be committed to Config"
        );
        assert_eq!(
            app.client.token(),
            None,
            "a candidate token that fails verification must not reach the live client either"
        );
    }

    #[tokio::test]
    async fn reauthenticate_surfaces_gateway_unreachable_distinctly_from_auth_rejected() {
        let mut app = test_app();
        // WHY: test_app()'s default URL (localhost:18789) has no listener,
        // so the probe hits a connection refusal rather than a 401/403 —
        // the other branch of the same classification this issue exists
        // to split apart.
        let err = app
            .reauthenticate("some-token")
            .await
            .expect_err("an unreachable gateway must fail reauthenticate");
        assert!(
            matches!(err, crate::error::Error::GatewayUnreachable { .. }),
            "expected GatewayUnreachable, not AuthRejected, got {err:?}"
        );
    }

    // WHY(#6818): the success path of `reauthenticate` is deliberately not
    // exercised end-to-end here. Past the verification probe it calls
    // `Config::store_new_token`, which resolves the real OS config
    // directory (`dirs::config_dir()`, not overridable without mutating
    // process env — denied by this crate's `unsafe_code = "deny"`) and
    // writes through `secret_store.rs` — a developer running `cargo test`
    // locally would have this test silently overwrite their real saved
    // koilon credential. Each piece of that path is covered hermetically
    // instead: `config.rs`'s `persist_new_token_*` tests (explicit `base`,
    // tempdir) and `apply_new_token_updates_in_memory_state_synchronously`
    // for the Config half, and skene's
    // `set_token_rebuilds_headers_used_by_subsequent_requests` for the
    // client-rebuild half. The two failure-path tests above cover
    // `reauthenticate` itself because both return before `store_new_token`
    // is ever called.
}
