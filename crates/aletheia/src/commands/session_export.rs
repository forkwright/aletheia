//! `aletheia export <session-id>`: export a session as Markdown or JSON.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;

use clap::Args;
use snafu::prelude::*;

use pylon::client::{GatewayClient, HistoryResponse, SessionReplayResponse, SessionResponse};

use crate::error::Result;

#[derive(Debug, Clone, Args)]
pub(crate) struct SessionExportArgs {
    /// Session ID to export
    // kanon:ignore RUST/primitive-for-domain-id — CLI arg struct field; clap parses from string, newtype would require custom FromStr
    pub session_id: String,

    /// Output format: `md` (default) or `json`
    #[arg(long, default_value = "md")]
    pub format: ExportFormat,

    /// Write output to this file instead of stdout
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Server URL
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
    pub url: String,

    /// Bearer token for authenticated endpoints
    #[arg(long, env = "ALETHEIA_TOKEN")]
    pub token: Option<String>,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub(crate) enum ExportFormat {
    Md,
    Json,
}

pub(crate) async fn run(args: &SessionExportArgs) -> Result<()> {
    validate_args(args)?;
    let client = build_client(args)?;

    let rendered = match args.format {
        ExportFormat::Md => {
            let session = client
                .session(&args.session_id)
                .await
                .whatever_context("failed to fetch session")?;
            let history = client
                .history(&args.session_id)
                .await
                .whatever_context("failed to fetch session history")?;
            render_markdown(&session, &history)
        }
        ExportFormat::Json => {
            let replay = client
                .session_replay(&args.session_id)
                .await
                .whatever_context("failed to fetch session replay export")?;
            render_json(&replay)?
        }
    };

    write_output(&rendered, args.output.as_deref())
}

fn validate_args(args: &SessionExportArgs) -> Result<()> {
    if args.session_id.trim().is_empty() {
        whatever!("<SESSION_ID> must not be empty");
    }
    if let Err(e) = reqwest::Url::parse(&args.url) {
        whatever!("--url is not a valid URL: {e} (got {:?})", args.url);
    }
    Ok(())
}

fn build_client(args: &SessionExportArgs) -> Result<GatewayClient> {
    // WARNING: credentials may be in default headers -- warn if sending to non-local, non-HTTPS
    if !args.url.starts_with("https://")
        && !args.url.contains("localhost")
        && !args.url.contains("127.0.0.1")
        && !args.url.contains("[::1]")
    {
        tracing::warn!(
            base_url = %args.url,
            "sending credentials over non-HTTPS to non-localhost URL"
        );
    }
    GatewayClient::new(&args.url, args.token.clone())
        .whatever_context("failed to build HTTP client")
}

fn render_markdown(session: &SessionResponse, history: &HistoryResponse) -> String {
    let mut out = String::new();

    // kanon:ignore RUST/no-silent-result-swallow — writing to a String never fails; std::fmt::Write returns Result for trait uniformity
    let _ = writeln!(out, "# Session: {}", session.session_key);
    // kanon:ignore RUST/no-silent-result-swallow — writing to a String never fails
    let _ = writeln!(out, "Started: {}", session.created_at);

    for msg in &history.messages {
        out.push_str("\n---\n\n");
        match msg.role.as_str() {
            "tool" | "tool_result" => {
                let name = msg.tool_name.as_deref().unwrap_or("unknown");
                // kanon:ignore RUST/no-silent-result-swallow — writing to a String never fails
                let _ = writeln!(out, "## Tool Call: {name} — {}", msg.created_at);
                // kanon:ignore RUST/no-silent-result-swallow — writing to a String never fails
                let _ = writeln!(out, "**Output:** {}", msg.content);
            }
            role => {
                let heading = capitalize_first(role);
                // kanon:ignore RUST/no-silent-result-swallow — writing to a String never fails
                let _ = writeln!(out, "## {heading} — {}", msg.created_at);
                out.push_str(&msg.content);
                out.push('\n');
            }
        }
    }

    out
}

fn render_json(replay: &SessionReplayResponse) -> Result<String> {
    serde_json::to_string_pretty(replay)
        .whatever_context("failed to serialize session replay export to JSON")
}

fn write_output(content: &str, path: Option<&std::path::Path>) -> Result<()> {
    match path {
        #[expect(
            clippy::disallowed_methods,
            reason = "aletheia CLI commands use synchronous filesystem operations for session export writes"
        )]
        Some(p) => {
            std::fs::write(p, content)
                .with_whatever_context(|_| format!("failed to write to {}", p.display()))?;
            // WHY: restrict session export to owner-only (0600) — contains private conversation history
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))
                    .with_whatever_context(|_| {
                        format!("failed to set permissions on {}", p.display())
                    })?;
            }
            Ok(())
        }
        None => std::io::stdout()
            .write_all(content.as_bytes())
            .whatever_context("failed to write to stdout"),
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let mut s = c.to_uppercase().collect::<String>();
            s.push_str(chars.as_str());
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pylon::client::{
        ReplayMessage, ReplaySession, ReplayToolAuditRecord, ReplayTurnAttempt, ReplayUsageRecord,
    };

    fn base_args() -> SessionExportArgs {
        SessionExportArgs {
            session_id: "valid-session-id".to_owned(),
            format: ExportFormat::Md,
            output: None,
            url: "http://127.0.0.1:18789".to_owned(),
            token: None,
        }
    }

    fn replay_with_audit_fields() -> SessionReplayResponse {
        SessionReplayResponse {
            version: 1,
            export_type: "sessionReplay".to_owned(),
            exported_at: "2026-06-28T00:00:00Z".to_owned(),
            session: ReplaySession {
                id: "session/requires encoding".to_owned(),
                nous_id: "alice".to_owned(),
                session_key: "main".to_owned(),
                status: "active".to_owned(),
                session_type: "primary".to_owned(),
                model: Some("mock-model".to_owned()),
                message_count: 1,
                token_count_estimate: 12,
                distillation_count: 0,
                created_at: "2026-06-28T00:00:00Z".to_owned(),
                updated_at: "2026-06-28T00:01:00Z".to_owned(),
                parent_session_id: None,
                thread_id: None,
                transport: None,
                display_name: Some("Audit fixture".to_owned()),
                last_input_tokens: 5,
                bootstrap_hash: Some("hash-fixture".to_owned()),
                last_distilled_at: None,
                computed_context_tokens: 12,
            },
            messages: vec![ReplayMessage {
                id: 7,
                seq: 42,
                role: "tool_result".to_owned(),
                content: "tool failed".to_owned(),
                tool_call_id: Some("toolu-1".to_owned()),
                tool_name: Some("read_file".to_owned()),
                token_estimate: 3,
                is_distilled: false,
                created_at: "2026-06-28T00:00:30Z".to_owned(),
            }],
            usage_records: vec![ReplayUsageRecord {
                turn_seq: 42,
                input_tokens: 10,
                output_tokens: 4,
                cache_read_tokens: 2,
                cache_write_tokens: 1,
                model: Some("mock-model".to_owned()),
            }],
            tool_audit_records: vec![ReplayToolAuditRecord {
                id: 3,
                nous_id: "alice".to_owned(),
                turn_seq: 42,
                tool_call_id: "toolu-1".to_owned(),
                tool_name: "read_file".to_owned(),
                duration_ms: 25,
                is_error: true,
                outcome: "error".to_owned(),
                result: Some("permission denied".to_owned()),
                approval: Some("approved".to_owned()),
                receipt: Some("receipt-1".to_owned()),
                created_at: "2026-06-28T00:00:31Z".to_owned(),
            }],
            turn_attempts: vec![ReplayTurnAttempt {
                version: 1,
                turn_id: "01JTESTTURN000000000000001".to_owned(),
                session_id: "session/requires encoding".to_owned(),
                nous_id: "alice".to_owned(),
                status: "failed".to_owned(),
                stage: Some("execute".to_owned()),
                error_code: Some("tool_failed".to_owned()),
                error_message: Some("tool failed".to_owned()),
                model: Some("mock-model".to_owned()),
                messages_persisted: Some(1),
                expected_messages: Some(3),
                created_at: "2026-06-28T00:00:32Z".to_owned(),
            }],
        }
    }

    #[test]
    fn replay_route_encodes_reserved_session_id_bytes() {
        let path = pylon::client::routes::session_replay("a/b c?d#e");
        assert_eq!(path, "/api/v1/sessions/a%2Fb%20c%3Fd%23e/replay");
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn json_export_preserves_replay_audit_fields() {
        let rendered = render_json(&replay_with_audit_fields()).expect("render replay export");
        let value: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered export is valid JSON");
        let field = |pointer: &str| {
            value
                .pointer(pointer)
                .expect("expected replay export JSON field")
        };

        assert_eq!(field("/exportType").as_str(), Some("sessionReplay"));
        assert_eq!(field("/messages/0/toolCallId").as_str(), Some("toolu-1"));
        assert_eq!(field("/messages/0/toolName").as_str(), Some("read_file"));
        assert_eq!(field("/usageRecords/0/turnSeq").as_i64(), Some(42));
        assert_eq!(field("/usageRecords/0/inputTokens").as_i64(), Some(10));
        assert_eq!(field("/usageRecords/0/cacheWriteTokens").as_i64(), Some(1));
        assert_eq!(
            field("/toolAuditRecords/0/toolCallId").as_str(),
            Some("toolu-1")
        );
        assert_eq!(field("/toolAuditRecords/0/isError").as_bool(), Some(true));
        assert_eq!(field("/toolAuditRecords/0/outcome").as_str(), Some("error"));
        assert_eq!(
            field("/turnAttempts/0/turnId").as_str(),
            Some("01JTESTTURN000000000000001")
        );
        assert_eq!(field("/turnAttempts/0/status").as_str(), Some("failed"));
        assert_eq!(
            field("/turnAttempts/0/errorCode").as_str(),
            Some("tool_failed")
        );
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test assertions")]
    fn validate_rejects_empty_session_id() {
        let mut a = base_args();
        a.session_id = String::new();
        let err = validate_args(&a).unwrap_err();
        assert!(
            err.to_string().contains("<SESSION_ID> must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test assertions")]
    fn validate_rejects_whitespace_only_session_id() {
        let mut a = base_args();
        a.session_id = "   ".to_owned();
        let err = validate_args(&a).unwrap_err();
        assert!(
            err.to_string().contains("<SESSION_ID> must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test assertions")]
    fn validate_rejects_malformed_url() {
        let mut a = base_args();
        a.url = "not a url".to_owned();
        let err = validate_args(&a).unwrap_err();
        assert!(
            err.to_string().contains("--url is not a valid URL"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_accepts_well_formed_args() {
        assert!(validate_args(&base_args()).is_ok());
    }
}
