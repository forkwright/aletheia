//! Background dispatch loop — routes inbound messages to nous actors.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn, Instrument};

use aletheia_agora::registry::ChannelRegistry;
use aletheia_agora::router::{reply_target, MessageRouter};
use aletheia_agora::types::{InboundMessage, SendParams};
use aletheia_nous::manager::NousManager;

/// Spawn a background task that dispatches inbound messages to nous actors.
///
/// Runs until the receiver channel closes (all senders dropped).
pub fn spawn_dispatcher(
    mut rx: mpsc::Receiver<InboundMessage>,
    router: Arc<MessageRouter>,
    nous_manager: Arc<NousManager>,
    channel_registry: Arc<ChannelRegistry>,
) -> JoinHandle<()> {
    let span = tracing::info_span!("message_dispatcher");
    tokio::spawn(
        async move {
            info!("dispatch loop started");
            while let Some(msg) = rx.recv().await {
                let router = Arc::clone(&router);
                let nous_mgr = Arc::clone(&nous_manager);
                let channels = Arc::clone(&channel_registry);
                let msg_span = tracing::info_span!(
                    "dispatch",
                    channel = %msg.channel,
                    sender = %msg.sender,
                );
                tokio::spawn(dispatch_one(msg, router, nous_mgr, channels).instrument(msg_span));
            }
            info!("dispatch loop stopped — all senders dropped");
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

    let Some(handle) = nous_manager.get(&decision.nous_id).cloned() else {
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

    let to = reply_target(&msg);
    let params = SendParams {
        to,
        text: turn_result.content,
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
