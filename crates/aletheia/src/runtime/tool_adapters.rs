//! Tool service adapters (moved from commands/server/mod.rs).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use agora::types::{ChannelProvider, SendParams};
use nous::cross::{CrossNousMessage, CrossNousRouter};
use organon::types::{CrossNousService, MessageService};

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

pub(crate) struct SignalAdapter(pub Arc<dyn ChannelProvider>);

impl MessageService for SignalAdapter {
    fn send_message(
        &self,
        to: &str,
        text: &str,
        _from_nous: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let params = SendParams {
            to: to.to_owned(),
            text: text.to_owned(),
            account_id: None,
            thread_id: None,
            attachments: None,
        };
        let provider = Arc::clone(&self.0);
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
