# agora

## At a glance

Channel registry and Signal provider for external messaging. Depends on koina and taxis. Entry point: `src/lib.rs` (ChannelListener, ChannelRegistry, MessageRouter).

## Depth

Channel registry and provider implementations for external messaging (Signal). 3K lines.

## Read first

1. `src/types.rs`: ChannelProvider trait, InboundMessage, SendParams, ChannelCapabilities
2. `src/registry.rs`: ChannelRegistry (name-based provider dispatch with metrics)
3. `src/router.rs`: MessageRouter (inbound message routing to nous agents)
4. `src/semeion/mod.rs`: SignalProvider (Signal channel implementation)
5. `src/listener.rs`: ChannelListener (merges inbound messages from all providers)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `ChannelProvider` | `types.rs` | Trait: `send()`, `probe()`, `capabilities()` |
| `ChannelRegistry` | `registry.rs` | Provider lookup by channel ID, `send()` dispatch, `probe_all()` |
| `ChannelListener` | `listener.rs` | Merges inbound messages from all providers into a single `mpsc::Receiver` |
| `MessageRouter` | `router.rs` | Resolves inbound messages to nous agents (group > source > channel default > global) |
| `InboundMessage` | `types.rs` | Normalized inbound message from any channel |
| `SignalProvider` | `semeion/mod.rs` | Signal channel: multi-account, JSON-RPC to signal-cli daemon |
| `SignalClient` | `semeion/client.rs` | HTTP client for signal-cli JSON-RPC API |
| `RouteDecision` | `router.rs` | Resolved routing target: `nous_id` + `session_key` + `MatchReason` |

## Patterns

- **Provider trait**: object-safe via `Pin<Box<dyn Future>>`, stored as `Arc<dyn ChannelProvider>`.
- **Routing priority**: exact group binding > exact source binding > channel wildcard > global default.
- **Connection resilience**: `AccountState` buffers outbound messages during disconnects, exponential reconnect backoff.
- **Listener cleanup**: abort callbacks registered at spawn time via `CleanupRegistry`, disarmed by `into_receiver()`.
- **Channel identity redaction**: any phone number, Matrix ID, or account ID reaching a log, span, or `ProbeResult.details` key goes through `koina::redact::redact_channel_id` -- never a per-provider helper. `InboundMessage`'s manual `Debug` impl (`types.rs`) is the canonical example.
- **Raw payload capture is opt-in**: `InboundMessage::raw` is populated only via `types::capture_raw_payload`, gated by `taxis::config::RawPayloadPolicy` (default off, bounded, redacted). A provider's `extract_message` never calls `serde_json::to_value` on the raw envelope/event directly.

## Recent substrate notes

- Channel capabilities must report only behavior actually implemented by the provider; Signal claims were intentionally narrowed.
- Listener cleanup uses registered abort callbacks and disarms them when ownership moves to the receiver.
- Raw provider-payload retention on `InboundMessage` was unconditional and un-redacted; it is now opt-in, bounded, and PII-redacted (`RawPayloadPolicy`), and channel identity redaction was centralized in `koina::redact` rather than duplicated per provider.

## Common tasks

| Task | Where |
|------|-------|
| Add channel provider | New module (e.g., `src/slack/`), implement `ChannelProvider` trait, register in binary |
| Add routing rule | `src/router.rs` (MatchReason enum + resolve logic) |
| Modify Signal client | `src/semeion/client.rs` (JSON-RPC methods) |
| Add metric | `src/metrics.rs` (LazyLock static, init function) |

## Dependencies

Uses: koina, taxis, reqwest, serde_json, tokio, snafu, tracing
Used by: aletheia (binary)
