//! Small rendering helpers for the chat view.

use dioxus::prelude::*;

use crate::components::chat::ChatState;
use crate::components::tool_approval::ToolApproval;
use crate::state::connection::ConnectionConfig;

pub(crate) fn render_approval(
    approval: crate::state::tools::ToolApprovalState,
    _chat_signal: Signal<ChatState>,
) -> Element {
    let turn_id = approval.turn_id.to_string();
    let tool_id = approval.tool_id.to_string();
    let turn_id_deny = turn_id.clone();
    let tool_id_deny = tool_id.clone();

    // WHY: capture IDs by value for the async approval/deny calls.
    let config: Signal<ConnectionConfig> = use_context();

    rsx! {
        ToolApproval {
            approval: approval,
            on_approve: move |_| {
                let turn_id = turn_id.clone();
                let tool_id = tool_id.clone();
                let cfg = config.read().clone();
                spawn(async move {
                    let client = skene::api::client::ApiClient::with_request_policy(
                        &cfg.server_url,
                        cfg.auth_token.clone(),
                        cfg.request_policy.clone(),
                    );
                    if let Ok(client) = client
                        && let Err(err) = client.approve_tool(&turn_id, &tool_id).await
                    {
                        tracing::warn!(%turn_id, %tool_id, error = %err, "tool approval request failed");
                    }
                });
            },
            on_deny: move |_| {
                let turn_id = turn_id_deny.clone();
                let tool_id = tool_id_deny.clone();
                let cfg = config.read().clone();
                spawn(async move {
                    let client = skene::api::client::ApiClient::with_request_policy(
                        &cfg.server_url,
                        cfg.auth_token.clone(),
                        cfg.request_policy.clone(),
                    );
                    if let Ok(client) = client
                        && let Err(err) = client.deny_tool(&turn_id, &tool_id).await
                    {
                        tracing::warn!(%turn_id, %tool_id, error = %err, "tool denial request failed");
                    }
                });
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
