# agora

**Purpose:** Channel registry and Signal provider for external messaging. Implements `ChannelProvider` trait and routes inbound messages to nous agents.

## Key types

| Type | Purpose |
|------|---------|
| `ChannelProvider` | Trait: `send()`, `probe()`, `capabilities()` |
| `ChannelRegistry` | Provider lookup by channel ID, `send()` dispatch, `probe_all()` |
| `ChannelListener` | Merges inbound messages from all providers into a single `mpsc::Receiver` |
| `MessageRouter` | Resolves inbound messages to nous agents (group > source > channel default > global) |
| `SignalProvider` | Signal channel: multi-account JSON-RPC to signal-cli daemon |

## Public API surface

- `agora::types` - `ChannelProvider` trait, `InboundMessage`, `SendParams`, `ChannelCapabilities`
- `agora::registry` - `ChannelRegistry` for provider lookup and dispatch
- `agora::router` - `MessageRouter`, `RouteDecision` for inbound routing to agents

## When to look here

- When implementing a new messaging channel (implement `ChannelProvider`)
- When modifying inbound message routing rules or agent assignment logic
