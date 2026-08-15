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
        // SECURITY(#4788): `sender_id` carries the sending agent's identity
        // into the audit record and, immediately below, into the allowlist
        // check -- it was previously discarded (the parameter was named
        // `_from_nous` and never read), so no attribution and no policy
        // check were possible. Deliberately `sender_id`, NOT `account_id`:
        // `account_id` selects which provider account (Signal phone
        // number, Matrix account) the message is sent FROM, and an
        // earlier version of this fix put `from_nous` there instead --
        // that broke every send whose sender wasn't itself a literal
        // registered account key, because `account_id: None` is what lets
        // `SignalProvider`/`MatrixProvider` fall back to their configured
        // default account (see `SendParams::sender_id` doc).
        if !self.outbound_policy.allows(Some(from_nous), to) {
            let denied = format!("recipient not in outbound allowlist for agent '{from_nous}'");
            return Box::pin(async move { Err(denied) });
        }

        let params = SendParams {
            to: to.to_owned(),
            text: text.to_owned(),
            account_id: None,
            sender_id: Some(from_nous.to_owned()),
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::sync::Mutex;

    use agora::types::{ChannelCapabilities, InboundMessage, ProbeResult, SendResult};
    use tokio::sync::mpsc;
    use tokio::task::JoinSet;
    use tokio_util::sync::CancellationToken;

    use super::*;

    static CAPS: ChannelCapabilities = ChannelCapabilities {
        threads: false,
        reactions: false,
        typing: false,
        media: false,
        streaming: false,
        rich_formatting: false,
        max_text_length: 4096,
    };

    /// SECURITY(#4788) regression: captures the exact [`agora::types::
    /// SendParams`] `SignalAdapter::send_message` hands to the provider,
    /// so the test below can assert `account_id` is left untouched
    /// (`None`, so `SignalProvider`/`MatrixProvider` fall back to their
    /// configured default account) while `sender_id` carries `from_nous`
    /// -- the bug this test guards against put `from_nous` into
    /// `account_id` instead, which broke provider account routing for
    /// every sender that wasn't itself a registered account key.
    #[derive(Default)]
    struct CapturingProvider {
        captured: Mutex<Vec<agora::types::SendParams>>,
    }

    impl ChannelProvider for CapturingProvider {
        fn id(&self) -> &str {
            "signal"
        }

        fn name(&self) -> &str {
            "Signal"
        }

        fn capabilities(&self) -> &ChannelCapabilities {
            &CAPS
        }

        fn send<'a>(
            &'a self,
            params: &'a agora::types::SendParams,
        ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
            self.captured.lock().unwrap().push(params.clone());
            Box::pin(async { SendResult::ok() })
        }

        fn listen(
            &self,
            _poll_interval: Option<Duration>,
            _cancel: CancellationToken,
        ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
            let (_tx, rx) = mpsc::channel(1);
            (rx, JoinSet::new())
        }

        fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
            Box::pin(async {
                ProbeResult {
                    ok: true,
                    latency_ms: None,
                    error: None,
                    details: None,
                }
            })
        }
    }

    #[tokio::test]
    async fn send_message_attributes_via_sender_id_not_account_id() {
        let mut policy = OutboundMessagePolicy::default();
        policy
            .allowlist
            .insert("syn".to_owned(), vec!["+15550100".to_owned()]);
        let provider = Arc::new(CapturingProvider::default());
        let adapter = SignalAdapter {
            provider: Arc::clone(&provider) as Arc<dyn ChannelProvider>,
            outbound_policy: policy,
        };

        adapter
            .send_message("+15550100", "hello", "syn")
            .await
            .expect("allowlisted send must succeed");

        let captured = provider.captured.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0].account_id, None,
            "account_id must stay None so the provider falls back to its \
             configured default account -- putting the sender here breaks \
             multi-account routing"
        );
        assert_eq!(
            captured[0].sender_id.as_deref(),
            Some("syn"),
            "sender_id must carry the attributed sending agent"
        );
    }

    #[tokio::test]
    async fn send_message_denies_recipient_outside_allowlist_before_provider_call() {
        let policy = OutboundMessagePolicy::default(); // default_deny = true, empty allowlist
        let provider = Arc::new(CapturingProvider::default());
        let adapter = SignalAdapter {
            provider: Arc::clone(&provider) as Arc<dyn ChannelProvider>,
            outbound_policy: policy,
        };

        let err = adapter
            .send_message("+15559999", "hello", "syn")
            .await
            .expect_err("unconfigured sender must be denied");
        assert!(err.contains("allowlist"));
        assert!(
            provider.captured.lock().unwrap().is_empty(),
            "a denied send must never reach the provider"
        );
    }
}
