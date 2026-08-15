//! Tool service adapters (moved from commands/server/mod.rs).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use agora::types::{ChannelProvider, SendParams};
use nous::cross::{CrossNousMessage, CrossNousRouter};
use organon::types::{CrossNousService, MessageService};
use taxis::config::OutboundMessagePolicy;

pub(crate) struct CrossNousAdapter(pub Arc<CrossNousRouter>);

impl CrossNousService for CrossNousAdapter {
    fn send(
        &self,
        from: &str,
        to: &str,
        session_key: &str,
        content: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let msg = CrossNousMessage::new(from, to, content).with_target_session(session_key);
        let router = Arc::clone(&self.0);
        Box::pin(async move {
            router
                .send(msg)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }

    fn ask(
        &self,
        from: &str,
        to: &str,
        session_key: &str,
        content: &str,
        timeout_secs: u64,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>> {
        let msg = CrossNousMessage::new(from, to, content)
            .with_target_session(session_key)
            .with_reply(Duration::from_secs(timeout_secs));
        let router = Arc::clone(&self.0);
        Box::pin(async move {
            router
                .ask(msg)
                .await
                .map(|reply| reply.content)
                .map_err(|e| e.to_string())
        })
    }
}

/// Bridges organon's `message` tool into an Agora channel provider.
///
/// SECURITY(#4788): this is the actual choke point every `message`-tool
/// send passes through -- it holds the provider directly rather than
/// going through `agora::ChannelRegistry` (that registry is used
/// elsewhere, for dispatching replies to *inbound* conversations, a
/// different trust boundary). `outbound_policy` is therefore enforced
/// here, before the provider is ever called.
pub(crate) struct SignalAdapter {
    pub provider: Arc<dyn ChannelProvider>,
    pub outbound_policy: OutboundMessagePolicy,
}

impl MessageService for SignalAdapter {
    fn send_message(
        &self,
        to: &str,
        text: &str,
        from_nous: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        // SECURITY(#4788): `account_id` carries the sending agent's identity
        // into the audit record and, immediately below, into the allowlist
        // check -- it was previously discarded (the parameter was named
        // `_from_nous` and never read), so no attribution and no policy
        // check were possible.
        if !self.outbound_policy.allows(Some(from_nous), to) {
            let denied = format!("recipient not in outbound allowlist for agent '{from_nous}'");
            return Box::pin(async move { Err(denied) });
        }

        let params = SendParams {
            to: to.to_owned(),
            text: text.to_owned(),
            account_id: Some(from_nous.to_owned()),
            thread_id: None,
            attachments: None,
        };
        let provider = Arc::clone(&self.provider);
        Box::pin(async move {
            let result = provider.send(&params).await;
            if result.sent {
                Ok(())
            } else {
                Err(result
                    .error
                    .unwrap_or_else(|| "unknown send error".to_owned()))
            }
        })
    }
}
