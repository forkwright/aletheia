# L3 API Index: agora

Crate path: `crates/agora`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// Requested channel does not exist in the registry.
    #[snafu(display("unknown channel: {id}"))]
    UnknownChannel {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A channel with this ID is already registered.
    #[snafu(display("duplicate channel: {id}"))]
    DuplicateChannel {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/listener.rs`

> Listens on registered channels, merging inbound messages into a single stream.
> 
> Dropping the listener aborts all background polling tasks through
> [`JoinSet`]'s drop behavior unless [`into_receiver`](Self::into_receiver)
> was called first, which transfers the receiver and handles to the caller.
```rust
pub struct ChannelListener {
    rx: Option<mpsc::Receiver<InboundMessage>>,
    handles: JoinSet<()>,
    /// Maximum concurrent inbound-message handler tasks.
    max_concurrent_handlers: usize,
}
```

```rust
impl ChannelListener {
    pub fn start <P> (
        provider: &P,
        poll_interval: Option<std::time::Duration>,
        cancel: CancellationToken,
    ) -> Self where
        P: ChannelProvider + ?Sized,;
    pub fn start_with_config <P> (
        provider: &P,
        poll_interval: Option<std::time::Duration>,
        cancel: CancellationToken,
        max_concurrent_handlers: usize,
    ) -> Self where
        P: ChannelProvider + ?Sized,;
    pub fn start_many <'a, I> (
        providers: I,
        poll_interval: Option<std::time::Duration>,
        cancel: &CancellationToken,
    ) -> Self where
        I: IntoIterator<Item = &'a dyn ChannelProvider>,;
    pub fn start_many_with_config <'a, I> (
        providers: I,
        poll_interval: Option<std::time::Duration>,
        cancel: &CancellationToken,
        max_concurrent_handlers: usize,
    ) -> Self where
        I: IntoIterator<Item = &'a dyn ChannelProvider>,;
    pub async fn run <F, Fut> (mut self, handler: F) where
        F: Fn(InboundMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,;
    pub fn into_receiver (mut self) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>);
}
```

## `src/matrix/client.rs`

```rust
pub struct MatrixClient {
    client: reqwest::Client,
    homeserver: String,
    access_token: String, // kanon:ignore RUST/plain-string-secret
    sync_timeout: Duration,
}
```

```rust
impl MatrixClient {
    pub fn new (homeserver: &str, access_token: &str) -> Result<Self>;
    pub fn with_timeouts (
        homeserver: &str,
        access_token: &str,
        rpc_timeout: Duration,
        sync_timeout: Duration,
    ) -> Result<Self>;
    pub async fn send_text (
        &self,
        room_id: &str,
        body: &str,
        thread_id: Option<&str>,
    ) -> Result<serde_json::Value>;
    pub async fn sync (&self, since: Option<&str>) -> Result<MatrixSyncResponse>;
    pub async fn health (&self) -> bool;
}
```

## `src/matrix/error.rs`

```rust
pub enum Error {
    /// HTTP transport or response decoding failed.
    #[snafu(display("Matrix HTTP error: {source}"))]
    Http {
        /// Underlying reqwest error.
        source: reqwest::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// JSON encoding or decoding failed.
    #[snafu(display("Matrix JSON error: {source}"))]
    Json {
        /// Underlying serde JSON error.
        source: serde_json::Error,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },

    /// Matrix API returned an unsuccessful status.
    #[snafu(display("Matrix API error {status}: {message}"))]
    Api {
        /// HTTP status code returned by the homeserver.
        status: u16,
        /// Matrix API error message.
        message: String,
        #[snafu(implicit)]
        /// Source location captured by snafu.
        location: snafu::Location,
    },
}
```

## `src/matrix/mod.rs`

```rust
pub struct MatrixSyncResponse {
    /// Batch token to pass as `since` on the next sync.
    pub next_batch: Option<String>,
    /// Joined rooms returned by sync.
    #[serde(default)]
    pub rooms: MatrixRooms,
}
```

```rust
pub struct MatrixRooms {
    /// Joined rooms keyed by room ID.
    #[serde(default)]
    pub join: HashMap<String, MatrixJoinedRoom>,
}
```

```rust
pub struct MatrixJoinedRoom {
    /// Timeline events returned for the room.
    #[serde(default)]
    pub timeline: MatrixTimeline,
}
```

```rust
pub struct MatrixTimeline {
    /// Timeline events.
    #[serde(default)]
    pub events: Vec<MatrixEvent>,
}
```

```rust
pub struct MatrixEvent {
    /// Event type, e.g. `m.room.message`.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Matrix user ID of the sender.
    pub sender: Option<String>,
    /// Event ID.
    pub event_id: Option<String>,
    /// Server timestamp in milliseconds.
    pub origin_server_ts: Option<u64>,
    /// Event content.
    #[serde(default)]
    pub content: MatrixEventContent,
    /// Raw unsigned metadata.
    #[serde(default)]
    pub unsigned: Option<serde_json::Value>,
}
```

```rust
pub struct MatrixEventContent {
    /// Matrix message type, e.g. `m.text`.
    pub msgtype: Option<String>,
    /// Plain-text body.
    pub body: Option<String>,
    /// Additional content fields retained for attachments and raw diagnostics.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
```

> Matrix channel provider implementing `ChannelProvider`.
```rust
pub struct MatrixProvider {
    accounts: HashMap<String, MatrixAccount>,
    default_account: Option<String>,
    circuit_breaker_threshold: u32,
}
```

```rust
impl MatrixProvider {
    pub fn new () -> Self;
    pub fn from_config (config: &taxis::config::MessagingConfig) -> Self;
    pub fn add_account (
        &mut self,
        account_id: String,
        client: client::MatrixClient,
        user_id: Option<String>,
        auto_start: bool,
        initial_since: Option<String>,
    );
    pub fn listen (
        &self,
        poll_interval: Option<Duration>,
        cancel: CancellationToken,
    ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>);
}
```

## `src/metrics.rs`

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

## `src/registry.rs`

> Registry of available channel providers.
> 
> Channels are registered at startup and looked up by ID during send operations.
> Uses `IndexMap` to preserve insertion order.
```rust
pub struct ChannelRegistry {
    providers: IndexMap<String, Arc<dyn ChannelProvider>>,
}
```

```rust
impl ChannelRegistry {
    pub fn new () -> Self;
    pub fn register (&mut self, provider: Arc<dyn ChannelProvider>) -> Result<()>;
    pub async fn send (&self, channel_id: &str, params: &SendParams) -> Result<SendResult>;
    pub async fn probe_all (&self) -> IndexMap<String, ProbeResult>;
}
```

## `src/router.rs`

```rust
pub struct RouteDecision<'a> {
    /// The nous agent that should handle this message.
    pub nous_id: &'a str,
    /// Session key derived from template expansion (e.g., `signal:+1234567890`).
    pub session_key: String, // kanon:ignore RUST/plain-string-secret WHY: session_key is a routing key (channel:sender template expansion), not a credential
    /// How the routing decision was determined.
    pub matched_by: MatchReason,
}
```

```rust
pub enum MatchReason {
    /// Matched by exact group ID binding on a specific channel.
    GroupBinding,
    /// Matched by exact sender binding on a specific channel.
    SourceBinding,
    /// Matched by channel-level wildcard (`source = "*"`).
    ChannelDefault,
    /// Fell through to the global default nous.
    GlobalDefault,
}
```

> Routes inbound channel messages to the appropriate nous agent.
> 
> Resolution order:
> 1. Exact group match: channel + `group_id` → `nous_id`
> 2. Exact source match: channel + source → `nous_id`
> 3. Default for channel: channel + `"*"` → `nous_id`
> 4. Global default: the nous with `default: true`
> 5. No match → `None`
```rust
pub struct MessageRouter {
    bindings: Vec<ChannelBinding>,
    default_nous: Option<String>,
}
```

```rust
impl MessageRouter {
    pub fn new (bindings: Vec<ChannelBinding>, default_nous: Option<String>) -> Self;
    pub fn resolve (&self, msg: &InboundMessage) -> Option<RouteDecision<'_>>;
}
```

```rust
pub fn reply_target (msg: &InboundMessage) -> String
```

## `src/semeion/client.rs`

```rust
pub struct SignalClient {
    client: reqwest::Client,
    rpc_url: String,
    health_url: String,
    health_timeout: Duration,
    receive_timeout: Duration,
}
```

```rust
impl SignalClient {
    pub fn new (base_url: &str) -> Result<Self>;
    pub fn with_timeouts (
        base_url: &str,
        rpc_timeout: Duration,
        health_timeout: Duration,
        receive_timeout: Duration,
    ) -> Result<Self>;
    pub async fn rpc (
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<Option<serde_json::Value>>;
    pub async fn send_message (&self, params: &SendParams) -> Result<Option<serde_json::Value>>;
    pub async fn health (&self) -> bool;
    pub async fn receive (&self, account: Option<&str>) -> Result<Vec<SignalEnvelope>>;
}
```

```rust
pub struct SendParams {
    /// Message text to send.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Phone number recipient (for direct messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<String>,
    /// Group ID recipient (for group messages, mutually exclusive with `recipient`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// Signal account phone number to send from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// File paths to attach to the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
}
```

## `src/semeion/connection.rs`

```rust
pub enum ConnectionState {
    /// Signal-cli daemon is reachable.
    Connected,
    /// Attempting to reconnect after failure.
    Reconnecting {
        /// Number of reconnection attempts made so far.
        attempt: u32,
    },
    /// Circuit breaker tripped after too many consecutive failures.
    /// Polling has stopped; only periodic health checks run.
    Halted {
        /// Total consecutive failures when the circuit breaker tripped.
        total_failures: u32,
    },
}
```

```rust
pub struct ConnectionHealthReport {
    /// Current connection state.
    pub state: ConnectionState,
    /// Messages waiting in the outbound buffer.
    pub buffered_messages: usize,
    /// Total messages dropped due to overflow.
    pub dropped_count: u64,
}
```

## `src/semeion/envelope.rs`

```rust
pub struct SignalEnvelope {
    /// Sender's phone number (e.g., `"+1234567890"`).
    pub source_number: Option<String>,
    /// Sender's UUID (alternative identifier when phone number is unavailable).
    pub source_uuid: Option<String>,
    /// Sender's display name from their Signal profile.
    pub source_name: Option<String>,
    /// Unix timestamp in milliseconds when the envelope was created.
    pub timestamp: Option<u64>,
    /// Data message payload (the actual message content).
    #[serde(default)]
    pub data_message: Option<DataMessage>,
    /// Sync message from a linked device (ignored for inbound processing).
    #[serde(default)]
    pub sync_message: Option<serde_json::Value>,
    /// Delivery/read receipt (ignored for inbound processing).
    #[serde(default)]
    pub receipt_message: Option<serde_json::Value>,
    /// Typing indicator (ignored for inbound processing).
    #[serde(default)]
    pub typing_message: Option<serde_json::Value>,
}
```

```rust
pub struct DataMessage {
    /// Unix timestamp in milliseconds for this specific message.
    pub timestamp: Option<u64>,
    /// Text body of the message.
    pub message: Option<String>,
    /// Group metadata if this message was sent to a group.
    #[serde(default)]
    pub group_info: Option<GroupInfo>,
    /// File attachments included with the message.
    #[serde(default)]
    pub attachments: Option<Vec<Attachment>>,
}
```

```rust
pub struct GroupInfo {
    /// Base64-encoded group identifier.
    pub group_id: Option<String>,
}
```

```rust
pub struct Attachment {
    /// Signal-assigned attachment identifier.
    pub id: Option<String>,
    /// MIME type (e.g., `"image/jpeg"`, `"application/pdf"`).
    pub content_type: Option<String>,
    /// Original filename if provided by the sender.
    pub filename: Option<String>,
    /// File size in bytes.
    pub size: Option<u64>,
}
```

## `src/semeion/error.rs`

```rust
pub enum Error {
    /// JSON-RPC returned an error response.
    #[snafu(display("signal RPC error {code}: {message}"))]
    Rpc {
        code: i64,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// HTTP transport error communicating with signal-cli daemon.
    #[snafu(display("signal HTTP error: {source}"))]
    Http {
        source: reqwest::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// No Signal account configured for the requested operation.
    #[snafu(display("no signal account: {account_id}"))]
    NoAccount {
        account_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization/deserialization failure.
    #[snafu(display("signal JSON error: {source}"))]
    Json {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/semeion/mod.rs`

```rust
pub enum SignalTarget {
    /// Direct message to a phone number (e.g., `"+1234567890"`).
    Phone(String),
    /// Group message identified by base64 group ID.
    Group(String),
}
```

```rust
pub fn parse_target (to: &str) -> SignalTarget
```

> Signal channel provider implementing `ChannelProvider`.
> 
> Manages multiple Signal accounts, each backed by a `SignalClient`.
> Tracks connection state per account with reconnect backoff and
> outbound message buffering during disconnection.
```rust
pub struct SignalProvider {
    clients: HashMap<String, client::SignalClient>,
    default_account: Option<String>,
    /// Per-account connection state and outbound buffer. Lock guards
    /// connection status transitions and buffered-message drain; held
    /// briefly during send and poll-loop state updates.
    account_states: HashMap<String, Arc<Mutex<AccountState>>>, // kanon:ignore RUST/no-arc-mutex-anti-pattern WHY: already uses tokio::sync::Mutex — correct for async code
    /// Per-account auto-start flag. When `false`, `listen()` skips
    /// spawning the receive poll task for that account.
    auto_start: HashMap<String, bool>,
    buffer_capacity: usize,
    circuit_breaker_threshold: u32,
    halted_health_check_interval: Duration,
}
```

```rust
impl SignalProvider {
    pub fn new () -> Self;
    pub fn with_buffer_capacity (capacity: usize) -> Self;
    pub fn from_config (config: &taxis::config::MessagingConfig) -> Self;
    pub fn add_account (
        &mut self,
        account_id: String,
        client: client::SignalClient,
        auto_start: bool,
    );
    pub fn listen (
        &self,
        poll_interval: Option<Duration>,
        cancel: CancellationToken,
    ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>);
    pub async fn connection_health (&self) -> HashMap<String, ConnectionHealthReport>;
}
```

## `src/types.rs`

```rust
pub struct ChannelCapabilities {
    /// Whether the channel supports threaded replies.
    pub threads: bool,
    /// Whether message reactions (emoji, etc.) are supported.
    pub reactions: bool,
    /// Whether typing indicators can be sent.
    pub typing: bool,
    /// Whether file/media attachments are supported.
    pub media: bool,
    /// Whether real-time streaming delivery is supported.
    pub streaming: bool,
    /// Whether markdown or other rich text formatting is supported.
    pub rich_formatting: bool,
    /// Maximum text length in a single message (channel-imposed limit).
    pub max_text_length: usize,
}
```

```rust
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
```

```rust
pub struct SendResult {
    /// Whether the message was successfully delivered to the channel.
    pub sent: bool,
    /// Error description if the send failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
```

```rust
impl SendResult {
    pub fn ok () -> Self;
    pub fn err (message: impl Into<String>) -> Self;
}
```

```rust
pub struct ProbeResult {
    /// Whether the channel is reachable.
    pub ok: bool,
    /// Round-trip latency in milliseconds, if measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// Error description if the probe failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Provider-specific health details (e.g., per-account status).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, serde_json::Value>>,
}
```

```rust
pub struct InboundMessage {
    /// Channel this message came from (e.g., "signal").
    pub channel: String,
    /// Sender identifier (phone number, user ID, etc.).
    pub sender: String,
    /// Display name if known.
    pub sender_name: Option<String>,
    /// Group/conversation identifier (None for DM).
    pub group_id: Option<String>,
    /// Message text content.
    pub text: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Attachment file paths or identifiers.
    pub attachments: Vec<String>,
    /// Raw channel-specific payload for extensions.
    pub raw: Option<serde_json::Value>,
}
```

> The contract every channel provider must implement.
> 
> Object-safe via `Pin<Box<dyn Future>>` (matches `ToolExecutor` in organon).
> Implementations are stored as `Arc<dyn ChannelProvider>` in the registry.
```rust
pub trait ChannelProvider : Send + Sync {
    fn id (&self) -> &str;
    fn name (&self) -> &str;
    fn capabilities (&self) -> &ChannelCapabilities;
    fn send <'a> (
        &'a self,
        params: &'a SendParams,
    ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>>;
    fn listen (
        &self,
        poll_interval: Option<Duration>,
        cancel: CancellationToken,
    ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>);
    fn probe <'a> (&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>>;
}
```
