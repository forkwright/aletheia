//! Core types for the channel abstraction layer.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

/// What a channel supports.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(clippy::struct_excessive_bools, reason = "capability flags are inherently boolean")]
pub struct ChannelCapabilities {
    pub threads: bool,
    pub reactions: bool,
    pub typing: bool,
    pub media: bool,
    pub streaming: bool,
    pub rich_formatting: bool,
    pub max_text_length: usize,
}

/// Parameters for sending a message through a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendParams {
    /// Target identifier (channel-specific: phone number, group ID, etc.)
    pub to: String,
    /// Message text (markdown).
    pub text: String,
    /// Account ID within the channel (for multi-account setups).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Thread/reply context identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// File attachment paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
}

/// Result of a send operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResult {
    pub sent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Health probe result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, serde_json::Value>>,
}

/// The contract every channel provider must implement.
///
/// Object-safe via `Pin<Box<dyn Future>>` (matches `ToolExecutor` in organon).
/// Implementations are stored as `Arc<dyn ChannelProvider>` in the registry.
pub trait ChannelProvider: Send + Sync {
    /// Unique channel identifier (e.g., `"signal"`, `"slack"`).
    fn id(&self) -> &str;

    /// Human-readable display name.
    fn name(&self) -> &str;

    /// What this channel supports.
    fn capabilities(&self) -> &ChannelCapabilities;

    /// Send a message outbound through this channel.
    fn send<'a>(
        &'a self,
        params: &'a SendParams,
    ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>>;

    /// Health probe for this channel.
    fn probe<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>>;
}
