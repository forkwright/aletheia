# agora

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
