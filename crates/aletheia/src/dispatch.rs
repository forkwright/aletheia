//! Background dispatch loop: routes inbound messages to nous actors.

use std::sync::Arc;

use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{Instrument, debug, info, warn};

use agora::command::{self, AgentSnapshot, ChannelSnapshot, CommandContext};
use agora::registry::ChannelRegistry;
use agora::router::{MessageRouter, reply_target};
use agora::types::{InboundMessage, SendParams};
use nous::manager::NousManager;

/// Spawn a background task that dispatches inbound messages to nous actors.
///
/// Runs until the receiver channel closes (all senders dropped).
/// Per-message dispatch tasks are tracked in a `JoinSet` and drained on exit.
pub(crate) fn spawn_dispatcher(
    mut rx: mpsc::Receiver<InboundMessage>,
    router: Arc<MessageRouter>,
    nous_manager: Arc<NousManager>,
    channel_registry: Arc<ChannelRegistry>,
    mut ready_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let span = tracing::info_span!("message_dispatcher");
    tokio::spawn(
        async move {
            while !*ready_rx.borrow_and_update() {
                if ready_rx.changed().await.is_err() {
                    warn!("ready channel dropped before ready signal");
                    return;
                }
            }
            info!("dispatch loop started");

            let mut in_flight = JoinSet::new();

            while let Some(msg) = rx.recv().await {
                let router = Arc::clone(&router);
                let nous_mgr = Arc::clone(&nous_manager);
                let channels = Arc::clone(&channel_registry);
                let msg_span = tracing::info_span!(
                    "dispatch",
                    channel = %msg.channel,
                    sender = %msg.sender,
                );
                in_flight.spawn(dispatch_one(msg, router, nous_mgr, channels).instrument(msg_span));

                // WHY: Reap completed tasks periodically to prevent unbounded growth.
                while let Some(result) = in_flight.try_join_next() {
                    if let Err(e) = result {
                        warn!(error = %e, "dispatch task panicked");
                    }
                }
            }

            // WHY: Drain remaining in-flight dispatch tasks before exiting.
            info!(
                remaining = in_flight.len(),
                "dispatch loop draining in-flight tasks"
            );
            while let Some(result) = in_flight.join_next().await {
                if let Err(e) = result {
                    warn!(error = %e, "dispatch task panicked during drain");
                }
            }

            info!("dispatch loop stopped");
        }
        .instrument(span),
    )
}

async fn dispatch_one(
    msg: InboundMessage,
    router: Arc<MessageRouter>,
    nous_manager: Arc<NousManager>,
    channel_registry: Arc<ChannelRegistry>,
) {
    let Some(decision) = router.resolve(&msg) else {
        warn!(
            channel = %msg.channel,
            sender = %msg.sender,
            "no route for inbound message, dropping"
        );
        return;
    };

    // NOTE: `!`-commands are intercepted before reaching the nous agent.
    // Plain turns fall through to send_turn as before.
    if let Some(cmd) = command::parse(&msg.text) {
        debug!(
            nous_id = %decision.nous_id,
            command = cmd.name(),
            "dispatching !-command"
        );
        let reply_text = execute_command(
            &cmd,
            &decision.nous_id,
            &decision.session_key,
            &nous_manager,
            &channel_registry,
        )
        .await;
        send_reply(&msg, &reply_text, &channel_registry).await;
        return;
    }

    let Some(handle) = nous_manager.get(decision.nous_id).cloned() else {
        warn!(
            nous_id = %decision.nous_id,
            "routed to unknown nous actor, dropping"
        );
        return;
    };

    info!(
        nous_id = %decision.nous_id,
        session_key = %decision.session_key,
        matched_by = ?decision.matched_by,
        "dispatching turn"
    );

    let turn_result = match handle.send_turn(&decision.session_key, &msg.text).await {
        Ok(result) => result,
        Err(e) => {
            warn!(error = %e, nous_id = %decision.nous_id, "turn failed");
            return;
        }
    };

    send_reply(&msg, &turn_result.content, &channel_registry).await;
}

/// Build a `CommandContext` and execute a parsed command, returning the reply text.
async fn execute_command(
    cmd: &command::Command,
    nous_id: &str,
    session_key: &str,
    nous_manager: &NousManager,
    channel_registry: &ChannelRegistry,
) -> String {
    // Gather current-agent snapshot.
    let current_agent = if let Some(handle) = nous_manager.get(nous_id) {
        match handle.status().await {
            Ok(st) => {
                let model = nous_manager
                    .get_config(nous_id)
                    .map_or_else(String::new, |c| c.generation.model.clone());
                Some(AgentSnapshot {
                    id: st.id,
                    lifecycle: st.lifecycle.to_string(),
                    session_count: st.session_count,
                    active_session: st.active_session,
                    panic_count: st.panic_count,
                    uptime_secs: st.uptime.as_secs(),
                    model,
                })
            }
            Err(e) => {
                warn!(error = %e, nous_id, "failed to query agent status for command");
                None
            }
        }
    } else {
        None
    };

    // Gather all-agents snapshot.
    let all_agents = {
        let statuses = nous_manager.list().await;
        statuses
            .into_iter()
            .map(|st| {
                let model = nous_manager
                    .get_config(&st.id)
                    .map_or_else(String::new, |c| c.generation.model.clone());
                AgentSnapshot {
                    id: st.id,
                    lifecycle: st.lifecycle.to_string(),
                    session_count: st.session_count,
                    active_session: st.active_session,
                    panic_count: st.panic_count,
                    uptime_secs: st.uptime.as_secs(),
                    model,
                }
            })
            .collect()
    };

    // Gather channel health snapshots only for commands that need them.
    let channels = match cmd {
        command::Command::Channels => channel_registry
            .probe_all()
            .await
            .into_iter()
            .map(|(id, probe)| ChannelSnapshot {
                id,
                healthy: probe.ok,
                latency_ms: probe.latency_ms,
            })
            .collect(),
        _ => vec![],
    };

    let ctx = CommandContext {
        current_nous_id: nous_id.to_owned(),
        session_key: session_key.to_owned(),
        current_agent,
        all_agents,
        channels,
    };

    command::execute(cmd, &ctx)
}

/// Send a reply back through the originating channel.
async fn send_reply(msg: &InboundMessage, text: &str, channel_registry: &ChannelRegistry) {
    let to = reply_target(msg);
    let params = SendParams {
        to,
        text: text.to_owned(),
        account_id: None,
        thread_id: None,
        attachments: None,
    };

    match channel_registry.send(&msg.channel, &params).await {
        Ok(result) => {
            if !result.sent {
                warn!(
                    error = result.error.as_deref().unwrap_or("unknown"),
                    "failed to send reply"
                );
            }
        }
        Err(e) => {
            warn!(error = %e, "channel send error");
        }
    }
}
