//! Small rendering helpers for the chat view.

use dioxus::prelude::*;

use crate::components::chat::ChatState;
use crate::components::tool_approval::ToolApproval;
use crate::state::connection::ConnectionConfig;
use crate::state::toasts::{ToastSeverity, ToastStore};

/// Resolve a pending tool approval and report failure visibly.
///
/// WHY(#7202): previously logged failures with `tracing::warn!` only --
/// the button appeared inert and the agent stayed blocked with no
/// user-visible explanation. This pushes a real error toast instead, and
/// calls the session-scoped `resolve_session_approval` (the legacy
/// `approve_tool`/`deny_tool` route pylon rejects for any scoped token,
/// `SECURITY(#5340)` at `crates/pylon/src/handlers/sessions/approvals.rs`).
/// Refuses locally, with the same visible error, if `session_id` is absent
/// rather than sending a request pylon would reject anyway.
async fn resolve_approval(
    cfg: ConnectionConfig,
    session_id: Option<String>,
    turn_id: String,
    tool_id: String,
    decision: &'static str,
) {
    let Some(session_id) = session_id else {
        tracing::warn!(%turn_id, %tool_id, decision, "tool approval has no session id");
        if let Some(mut store) = try_consume_context::<Signal<ToastStore>>() {
            store.write().push(
                ToastSeverity::Error,
                "Cannot resolve approval: no active session",
            );
        }
        return;
    };
    let client = skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone());
    let result = match client {
        Ok(client) => {
            client
                .resolve_session_approval(&session_id, &turn_id, &tool_id, decision)
                .await
        }
        Err(err) => Err(err),
    };
    if let Err(err) = result {
        tracing::warn!(%session_id, %turn_id, %tool_id, decision, error = %err, "tool approval request failed");
        if let Some(mut store) = try_consume_context::<Signal<ToastStore>>() {
            store
                .write()
                .push(ToastSeverity::Error, format!("Approval failed: {err}"));
        }
    }
}

pub(crate) fn render_approval(
    approval: crate::state::tools::ToolApprovalState,
    _chat_signal: Signal<ChatState>,
) -> Element {
    let session_id = approval.session_id.as_ref().map(ToString::to_string);
    let turn_id = approval.turn_id.to_string();
    let tool_id = approval.tool_id.to_string();
    let session_id_deny = session_id.clone();
    let turn_id_deny = turn_id.clone();
    let tool_id_deny = tool_id.clone();

    // WHY: capture IDs by value for the async approval/deny calls.
    let config: Signal<ConnectionConfig> = use_context();

    rsx! {
        ToolApproval {
            approval: approval,
            on_approve: move |_| {
                let cfg = config.read().clone();
                spawn(resolve_approval(
                    cfg,
                    session_id.clone(),
                    turn_id.clone(),
                    tool_id.clone(),
                    "approved",
                ));
            },
            on_deny: move |_| {
                let cfg = config.read().clone();
                spawn(resolve_approval(
                    cfg,
                    session_id_deny.clone(),
                    turn_id_deny.clone(),
                    tool_id_deny.clone(),
                    "denied",
                ));
            },
        }
    }
}

#[must_use]
pub(crate) fn format_tool_call(tc: &crate::state::events::ToolCallInfo) -> String {
    if tc.completed {
        let marker = if tc.is_error { "[x]" } else { "[v]" };
        match tc.duration_ms {
            Some(ms) => format!("{marker} {} ({ms}ms)", tc.tool_name),
            None => format!("{marker} {}", tc.tool_name),
        }
    } else {
        let ellipsis = "[\u{2026}]";
        format!("{ellipsis} {}", tc.tool_name)
    }
}
