# agora

## At a glance

Channel registry plus first-class Signal and Matrix providers for external messaging. Depends on koina and taxis. Entry point: `src/lib.rs` (ChannelListener, ChannelRegistry, MessageRouter).

## Depth

Channel registry and provider implementations for external messaging (Signal and Matrix).

## Read first

1. `src/types.rs`: ChannelProvider trait, InboundMessage, SendParams, ChannelCapabilities
2. `src/registry.rs`: ChannelRegistry (name-based provider dispatch with metrics)
3. `src/router.rs`: MessageRouter (inbound message routing to nous agents)
4. `src/semeion/mod.rs`: SignalProvider (Signal channel implementation)
5. `src/matrix/mod.rs`: MatrixProvider (Matrix Client-Server API implementation)
6. `src/listener.rs`: ChannelListener (merges inbound messages from all providers)

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
| `MatrixProvider` | `matrix/mod.rs` | Matrix channel: multi-account Client-Server API sync/send |
| `RouteDecision` | `router.rs` | Resolved routing target: `nous_id` + `session_key` + `MatchReason` |

## Patterns

- **Provider trait**: object-safe via `Pin<Box<dyn Future>>`, stored as `Arc<dyn ChannelProvider>`.
- **Routing priority**: exact group binding > exact source binding > channel wildcard > global default.
- **Command authority**: only an exact account-scoped direct-message binding can resolve to Operator; group, wildcard, and global routes are Public.
- **Ingress privacy**: built-in provider normalization sets `raw = None`; no transport config switch retains provider envelopes.
- **Connection resilience**: `AccountState` buffers outbound messages during disconnects, exponential reconnect backoff.
- **Listener cleanup**: `JoinSet` owns provider and forwarding tasks; accepted handlers drain before shutdown.
- **Subscription metrics**: each Signal/Matrix account task owns one `ActiveSubscriptionGuard`; the listener never counts the aggregate again.

## Recent substrate notes

- Channel capabilities must report only behavior actually implemented by the provider; Signal claims were intentionally narrowed.
- Matrix checkpoints an accepted sync batch before forwarding it, so restart cannot replay an already-accepted batch.

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
