# Spec 34: Agora — Channel Abstraction and Slack Integration

**Status:** Implemented (Phases 1–6 complete)
**Author:** Syn
**Date:** 2026-02-27
**Spec:** 34

---

## Naming

**Agora** (ἀγορά) — the gathering place where speech happens.

| Layer | Reading |
|-------|---------|
| **L1** | The channel subsystem — where messages arrive from and are sent to external platforms |
| **L2** | The abstraction layer between nous (the thinking) and the outside world; the common ground all messaging platforms share |
| **L3** | In Athens, the agora was not the market — that's the Roman reduction. The agora was the place of gathering and speech, where different parties came to communicate. The private thought of the citizen became public speech in the agora. Different voices entered through different stoa (covered walkways), but once inside, all participated in the same discourse |
| **L4** | The module IS an agora — different channels (Signal, Slack, future platforms) enter through their own stoa but converge into a single discourse (the nous pipeline). The module doesn't communicate; it is the *place where communication becomes possible* |

**Topology:** Agora sits between semeion (which becomes Signal's stoa) and nous. Where semeion currently couples Signal directly to nous, agora interposes a common gathering point. New channels don't modify the agora — they enter through their own stoa.

**Semeion** retains its name — σημεῖον (the sign) is the artifact of Signal communication, and the module continues to implement Signal-specific protocol. But semeion becomes a channel provider *within* agora rather than a direct wire to nous.

---

## Problem

Signal integration is hardwired into `aletheia.ts` and `semeion/`. There is no channel abstraction — adding a second messaging platform means duplicating the entire listener/sender/routing flow. The current architecture:

```
Signal ← semeion/listener.ts → NousManager.handleMessage()
       ← semeion/sender.ts  ← message tool (hardcoded to Signal)
```

Every platform-specific concern (SSE parsing, signal-cli daemon management, read receipts, typing indicators, markdown → Signal formatting) is entangled with platform-agnostic concerns (message routing, agent binding resolution, session management, send/receive lifecycle).

Slack integration is the forcing function, but the real deliverable is a channel abstraction that makes the *next* integration (Discord, Matrix, email, webhook) a focused implementation rather than another architectural entanglement.

---

## Principles

1. **Channels are stoa, not the agora.** Each channel implementation handles its platform's protocol. The agora handles what's common: routing, binding resolution, lifecycle, send dispatch. A new channel never touches agora internals — it implements the interface and registers.

2. **Signal is the first channel, not the special one.** After this spec, Signal and Slack have identical architectural status. No channel gets privileged access to nous. This means refactoring Signal out of `aletheia.ts` into the same plugin interface Slack uses.

3. **CLI onboarding.** `aletheia channel add slack` guides the user through token creation, scopes, and configuration. Same pattern for any future channel. The CLI is the front door.

4. **Configuration is declarative.** Channel config lives in `channels:` in `aletheia.yaml`. Bindings already support `channel: "signal"` matching — extending to `channel: "slack"` is schema-only.

5. **No runtime entanglement.** A channel that isn't configured doesn't load. A channel that crashes doesn't take down other channels or the nous pipeline.

---

## Design

### Channel Provider Interface

```typescript
// src/agora/types.ts

export interface ChannelProvider {
  /** Unique channel identifier — used in config, bindings, routing */
  readonly id: string;

  /** Human-readable name */
  readonly name: string;

  /** What this channel supports */
  readonly capabilities: ChannelCapabilities;

  /**
   * Start listening for inbound messages.
   * Called during runtime startup if the channel is configured and enabled.
   * Must wire inbound messages to the provided dispatcher.
   */
  start(ctx: ChannelContext): Promise<void>;

  /**
   * Send a message outbound through this channel.
   * Called by the agora send dispatcher when routing determines this channel.
   */
  send(params: ChannelSendParams): Promise<ChannelSendResult>;

  /**
   * Send a typing indicator (if supported).
   */
  sendTyping?(params: ChannelTypingParams): Promise<void>;

  /**
   * Send a reaction (if supported).
   */
  sendReaction?(params: ChannelReactionParams): Promise<void>;

  /**
   * Gracefully stop the channel.
   */
  stop(): Promise<void>;

  /**
   * Health probe — is this channel connected and functional?
   */
  probe?(): Promise<ChannelProbeResult>;
}

export interface ChannelCapabilities {
  /** Supports threading (Slack threads, Signal quotes) */
  threads: boolean;
  /** Supports emoji reactions */
  reactions: boolean;
  /** Supports typing indicators */
  typing: boolean;
  /** Supports file/media attachments */
  media: boolean;
  /** Supports native streaming/progressive updates */
  streaming: boolean;
  /** Supports rich formatting (blocks, embeds) beyond markdown */
  richFormatting: boolean;
  /** Max text length per message */
  maxTextLength: number;
}

export interface ChannelContext {
  /** Dispatch an inbound message to the nous pipeline */
  dispatch: (msg: InboundMessage) => Promise<TurnOutcome>;
  /** Stream an inbound message through the nous pipeline */
  dispatchStream: (msg: InboundMessage) => AsyncIterable<TurnStreamEvent>;
  /** The runtime config */
  config: AletheiaConfig;
  /** Session store for thread/session lookups */
  store: SessionStore;
  /** Abort signal for graceful shutdown */
  abortSignal: AbortSignal;
  /** Command registry for slash-command handling */
  commands?: CommandRegistry;
  /** Logger scoped to this channel */
  log: Logger;
}

export interface ChannelSendParams {
  /** Target identifier (channel-specific format) */
  to: string;
  /** Message text (markdown) */
  text: string;
  /** Account ID within the channel (for multi-account setups) */
  accountId?: string;
  /** Thread/reply context */
  threadId?: string;
  /** Media attachments */
  media?: MediaAttachment[];
  /** Sender identity override (agent name, emoji) */
  identity?: ChannelIdentity;
}

export interface ChannelSendResult {
  /** Channel-assigned message ID */
  messageId: string;
  /** Resolved channel/conversation ID */
  channelId: string;
}

export interface ChannelIdentity {
  name?: string;
  emoji?: string;
  avatarUrl?: string;
}

export interface ChannelProbeResult {
  ok: boolean;
  latencyMs?: number;
  error?: string;
  details?: Record<string, unknown>;
}
```

### Agora Registry

```typescript
// src/agora/registry.ts

export class AgoraRegistry {
  private providers = new Map<string, ChannelProvider>();

  /** Register a channel provider */
  register(provider: ChannelProvider): void;

  /** Get a provider by channel ID */
  get(channelId: string): ChannelProvider | undefined;

  /** List all registered providers */
  list(): ChannelProvider[];

  /** Start all configured and enabled channels */
  startAll(ctx: Omit<ChannelContext, 'log'>): Promise<void>;

  /** Stop all channels gracefully */
  stopAll(): Promise<void>;

  /** Probe all channels */
  probeAll(): Promise<Map<string, ChannelProbeResult>>;

  /**
   * Send a message through the appropriate channel.
   * Resolves channel from the target format or explicit channelId.
   */
  send(channelId: string, params: ChannelSendParams): Promise<ChannelSendResult>;
}
```

### Signal as Channel Provider

Semeion's existing code refactors into a `SignalChannelProvider` implementing `ChannelProvider`:

- `semeion/listener.ts` → `SignalChannelProvider.start()` — SSE consumption, envelope parsing, mention hydration, authorization
- `semeion/sender.ts` → `SignalChannelProvider.send()` — message chunking, markdown → Signal formatting, PII scanning
- `semeion/client.ts` → stays as internal Signal-specific HTTP client
- `semeion/daemon.ts` → stays as signal-cli process management (start/stop/ready)
- `semeion/commands.ts` → commands register with agora via `ChannelContext.commands`
- `semeion/format.ts` → stays as Signal-specific formatting
- `semeion/tts.ts` → stays as Signal-specific TTS (audio messages)

The key refactor: `aletheia.ts` stops importing semeion directly. Instead:

```typescript
// aletheia.ts — after this spec
import { AgoraRegistry } from "./agora/registry.js";
import { SignalChannelProvider } from "./semeion/provider.js";
import { SlackChannelProvider } from "./agora/channels/slack/provider.js";

// During startRuntime:
const agora = new AgoraRegistry();

if (config.channels.signal?.enabled) {
  agora.register(new SignalChannelProvider(config, commandRegistry));
}
if (config.channels.slack?.enabled) {
  agora.register(new SlackChannelProvider(config));
}

await agora.startAll({ dispatch, dispatchStream, config, store, abortSignal, commands });
```

### Slack Channel Provider

```typescript
// src/agora/channels/slack/provider.ts

export class SlackChannelProvider implements ChannelProvider {
  readonly id = "slack";
  readonly name = "Slack";
  readonly capabilities: ChannelCapabilities = {
    threads: true,
    reactions: true,
    typing: false,  // Slack has no typing indicator API for bots
    media: true,
    streaming: true,  // Native text streaming via chat.startStream
    richFormatting: true,  // Block Kit (v2)
    maxTextLength: 4000,
  };

  // Uses @slack/bolt in Socket Mode (no public URL required)
  // Inbound: message events → parse → dispatch to nous
  // Outbound: WebClient.chat.postMessage with mrkdwn formatting
}
```

### Configuration Schema

```yaml
# aletheia.yaml
channels:
  signal:
    enabled: true
    accounts:
      default:
        account: "+1..."
        # ... existing signal config
  slack:
    enabled: true
    mode: socket  # "socket" (default) or "http"
    appToken: "xapp-..."  # Socket Mode app token
    botToken: "xoxb-..."  # Bot user token
    # Optional:
    dmPolicy: open  # "open" | "allowlist" | "disabled"
    groupPolicy: allowlist  # "open" | "allowlist" | "disabled"
    allowedChannels: []  # Slack channel IDs/names
    requireMention: true  # Only respond when @mentioned in channels
    identity:
      # Per-agent identity in Slack (requires chat:write.customize scope)
      useAgentIdentity: true
```

Binding example:
```yaml
bindings:
  - agentId: syn
    match:
      channel: slack
      peer:
        kind: channel
        id: C0123456789  # Slack channel ID
  - agentId: syn
    match:
      channel: slack
      peer:
        kind: direct  # DMs
```

### CLI Onboarding

```
$ aletheia channel add slack

  Slack Integration Setup
  ─────────────────────────

  Step 1: Create a Slack App

    Visit https://api.slack.com/apps and click "Create New App"
    Choose "From scratch" and select your workspace

  Step 2: Enable Socket Mode

    In your app settings, go to "Socket Mode" and enable it
    Create an App-Level Token with 'connections:write' scope
    Copy the token (starts with xapp-)

  ? App Token (xapp-...): xapp-1-A0123...

  Step 3: Bot Token

    Go to "OAuth & Permissions"
    Add these Bot Token Scopes:
      • channels:history    • channels:read
      • chat:write          • groups:history
      • groups:read         • im:history
      • im:read             • reactions:read
      • reactions:write     • users:read
      • chat:write.customize (optional — agent identity)
      • assistant:write     (optional — native streaming)

    Install the app to your workspace
    Copy the Bot User OAuth Token (starts with xoxb-)

  ? Bot Token (xoxb-...): xoxb-1234...

  Step 4: Subscribe to Events

    Go to "Event Subscriptions" → "Subscribe to bot events"
    Add these events:
      • app_mention         • message.channels
      • message.groups      • message.im
      • reaction_added

  Step 5: Configure access

  ? DM policy (open/allowlist/disabled): open
  ? Channel policy (open/allowlist/disabled): allowlist
  ? Require @mention in channels? (Y/n): Y

  ✓ Slack configuration written to aletheia.yaml
  ✓ Restart Aletheia to activate: systemctl restart aletheia

  To bind an agent to a Slack channel:
    aletheia binding add --agent syn --channel slack --peer channel:C0123456789
```

### Message Flow

**Inbound (Slack → Nous):**

```
Slack WebSocket (Socket Mode)
  → @slack/bolt App event handler
  → SlackChannelProvider.onMessage()
    → Parse Slack event → normalize to InboundMessage
      - channel: "slack"
      - peerId: channel ID or user ID
      - peerKind: "channel" | "direct" | "thread"
      - accountId: Slack account
      - text: strip bot mention, convert mrkdwn → markdown
      - threadId: Slack thread_ts
      - media: Slack file attachments
    → ctx.dispatch(msg) or ctx.dispatchStream(msg)
```

**Outbound (Nous → Slack):**

```
NousManager turn completes
  → pylon routes or agora send dispatcher
  → agora.send("slack", { to, text, threadId, identity })
  → SlackChannelProvider.send()
    → Format: markdown → Slack mrkdwn
    → Chunk at 4000 chars
    → Resolve identity from agent config
    → WebClient.chat.postMessage({ channel, text, thread_ts, username, icon_emoji })
```

### Module Structure

```
infrastructure/runtime/src/
├── agora/                        # NEW — channel abstraction
│   ├── types.ts                  # ChannelProvider interface, capabilities, params
│   ├── registry.ts               # AgoraRegistry — register, start, stop, send, probe
│   ├── format.ts                 # Shared formatting utilities (markdown normalization)
│   ├── cli.ts                    # CLI onboarding: `aletheia channel add <id>`
│   └── channels/
│       └── slack/
│           ├── provider.ts       # SlackChannelProvider implements ChannelProvider
│           ├── listener.ts       # Socket Mode event handling, message parsing
│           ├── sender.ts         # Outbound message delivery via WebClient
│           ├── format.ts         # Markdown → Slack mrkdwn conversion
│           ├── client.ts         # @slack/bolt App wrapper and WebClient factory
│           ├── types.ts          # Slack-specific types
│           ├── streaming.ts      # Native Slack text streaming (Phase 5)
│           └── reactions.ts      # Emoji reaction helpers (Phase 5)
├── semeion/                      # REFACTORED — becomes Signal channel provider
│   ├── provider.ts               # NEW — SignalChannelProvider implements ChannelProvider
│   ├── client.ts                 # Unchanged — signal-cli HTTP client
│   ├── daemon.ts                 # Unchanged — signal-cli process management
│   ├── listener.ts               # Refactored — SSE parsing, extracted from direct nous coupling
│   ├── sender.ts                 # Refactored — extracted from direct nous coupling
│   ├── format.ts                 # Unchanged — Signal markdown formatting
│   ├── commands.ts               # Unchanged — command registry
│   ├── tts.ts                    # Unchanged — text-to-speech
│   ├── transcribe.ts             # Unchanged — audio transcription
│   └── preprocess.ts             # Unchanged — link preprocessing
```

### Dependency Position

Agora sits at the same layer as semeion in the dependency graph:

| Module | May Import | Must Not Import |
|--------|-----------|-----------------|
| `agora` | `koina`, `taxis`, `mneme`, `nous` (types only), `organon` (commands type) | `pylon`, `prostheke`, `daemon`, `symbolon`, `dianoia`, `portability`, `hermeneus` |

Semeion's dependency rules remain unchanged. Agora imports semeion for the Signal provider registration, or semeion self-registers. The preferred pattern is that `aletheia.ts` creates both providers and registers them with agora.

---

## Phases

### Phase 1: Agora Core + Signal Refactor ✅

**Scope:** Create the `agora/` module with the `ChannelProvider` interface and `AgoraRegistry`. Refactor Signal out of `aletheia.ts` into `semeion/provider.ts` implementing `ChannelProvider`. Zero behavioral change — Signal works exactly as before, but through the abstraction.

**Changes:**

- Create `src/agora/types.ts` — all interfaces defined above
- Create `src/agora/registry.ts` — `AgoraRegistry` class
- Create `src/semeion/provider.ts` — `SignalChannelProvider` wrapping existing listener/sender
- Refactor `src/aletheia.ts` — replace direct semeion wiring with agora registry
- Update `src/pylon/routes/system.ts` — health probe via `agora.probeAll()`
- Update `taxis/schema.ts` — ensure `ChannelsConfig` is extensible

**Acceptance criteria:**
- [x] All existing Signal tests pass unchanged
- [x] `ChannelProvider` interface is defined and documented
- [x] `AgoraRegistry` manages provider lifecycle
- [x] `aletheia.ts` creates Signal provider via agora, not direct wiring
- [x] No behavioral change to any existing functionality
- [x] New tests for registry (register, start, stop, send dispatch)

**Result:** Merged as PR #283 (commit `93f0e442`). 15 registry tests, `SignalChannelProvider` wraps existing semeion code. Net -67 lines in `aletheia.ts` — cleaner than before.

**Tests:**
- `agora/registry.test.ts` — mock provider registration, lifecycle, send routing (15 tests)
- `semeion/provider.test.ts` — SignalChannelProvider satisfies ChannelProvider contract

---

### Phase 2: Configuration + CLI Onboarding ✅

**Scope:** Extend the config schema for Slack. Build `aletheia channel add` CLI command with interactive onboarding. This is infra — no Slack runtime code yet, just the config layer and the front door.

**Changes:**

- Extend `taxis/schema.ts` — add `SlackChannelConfig` schema under `channels.slack`
- Create `src/agora/cli.ts` — `aletheia channel add <id>` interactive wizard
  - Generic scaffolding that delegates to channel-specific onboarding steps
  - Signal gets a retroactive onboarding flow too (for consistency)
- Create `src/agora/channels/slack/config.ts` — Slack-specific config validation, token format checks
- Create `src/agora/channels/slack/onboarding.ts` — Slack-specific CLI wizard steps
- Wire CLI command into `infrastructure/runtime/src/entry.ts`

**Acceptance criteria:**
- [x] `aletheia channel add slack` runs the full onboarding wizard
- [x] Wizard validates token formats (xapp-, xoxb-) before writing config
- [x] Config is written to `aletheia.yaml` under `channels.slack`
- [x] `aletheia channel list` shows configured channels and status
- [x] `aletheia channel remove slack` removes config cleanly
- [x] Schema validation catches invalid Slack config on startup

**Result:** Merged as PR #284 (commit `1f0f6b75`). 14 new tests (7 config schema + 7 CLI). Interactive wizard validates `xapp-`/`xoxb-` prefixes, guides through scopes and event subscriptions.

**Tests:**
- `agora/cli.test.ts` — wizard flow (7 tests)
- `agora/config.test.ts` — schema validation, defaults, rejection (7 tests)

---

### Phase 3: Slack Channel Provider — Core Messaging ✅

**Scope:** Implement `SlackChannelProvider` with Socket Mode inbound and WebClient outbound. This is the first real Slack integration — messages flow both directions.

**Reference implementation:** OpenClaw (`github.com/openclaw/openclaw`, MIT, 236k stars) — their `src/slack/` directory (~15,900 lines) is production-grade Slack integration. Full analysis saved to `nous/syn/context/openclaw-slack-reference.md`. Key patterns to adopt:

- `@slack/bolt` App with `socketMode: true` — handles reconnect automatically
- Inbound debouncing (`createInboundDebouncer`) — rapid messages coalesced into single turn
- `markMessageSeen(channel, ts)` dedup — prevents duplicate event processing
- Identity override with `chat:write.customize` scope + graceful fallback on `missing_scope`
- IR-based markdown → mrkdwn conversion (not regex) — handles edge cases properly
- `AbortSignal` lifecycle for clean start/stop
- `auth.test()` on startup to get `botUserId` for self-message filtering

**Changes:**

- Add dependencies: `@slack/bolt`, `@slack/web-api`
- Create `src/agora/channels/slack/provider.ts` — `SlackChannelProvider`
- Create `src/agora/channels/slack/listener.ts` — Socket Mode event handler
  - `message` events (DM, channel, thread)
  - `app_mention` events
  - Mention-gating for channels (configurable)
  - Message normalization: strip bot mention, mrkdwn → markdown
  - Thread context extraction (thread_ts → threadId)
  - File attachment handling
  - Inbound debouncing (per OpenClaw pattern — coalesce rapid messages from same user/thread)
  - Message dedup via seen-set (per OpenClaw `markMessageSeen` pattern)
- Create `src/agora/channels/slack/sender.ts` — outbound message delivery
  - Markdown → mrkdwn formatting
  - Message chunking at 4000 chars
  - Thread replies via thread_ts
  - File upload for media via `files.uploadV2`
  - Agent identity via username + icon_emoji (chat:write.customize) with scope fallback
- Create `src/agora/channels/slack/format.ts` — bidirectional format conversion
  - Markdown → mrkdwn (bold, italic, code, links, lists, blockquotes)
  - mrkdwn → markdown (for inbound message normalization)
  - Slack user/channel mention handling (`<@U123>`, `<#C456>`)
  - Escape `&`, `<`, `>` while preserving Slack angle-bracket tokens
- Create `src/agora/channels/slack/client.ts` — @slack/bolt App wrapper
  - Socket Mode initialization with retry config
  - WebClient factory (per OpenClaw `createSlackWebClient` pattern)
  - `auth.test()` on connect for botUserId/teamId
  - Connection health monitoring
- Wire into `aletheia.ts` — register Slack provider with agora when configured

**Acceptance criteria:**
- [x] Slack bot receives DMs and responds through the nous pipeline
- [x] Slack bot receives @mentions in channels and responds in-thread
- [x] Bindings route Slack channels/DMs to correct agents
- [x] Outbound messages use Slack mrkdwn formatting
- [x] Messages >4000 chars are chunked properly
- [x] Thread context is maintained (replies stay in thread)
- [x] Agent identity appears in Slack (name + emoji) when scope permits
- [x] Graceful reconnection on WebSocket drop (handled by @slack/bolt Socket Mode)
- [x] Probe endpoint reports Slack health (auth.test() with latency)
- [x] No impact on Signal functionality (semeion tests unchanged)

**Tests:** 50 tests across 4 files (PR #294, merged):
- `agora/channels/slack/format.test.ts` — 26 tests: markdown ↔ mrkdwn conversion, chunking, mention stripping
- `agora/channels/slack/sender.test.ts` — 8 tests: delivery, identity fallback, threading, error handling
- `agora/channels/slack/listener.test.ts` — 8 tests: debouncing, coalescing, flush, key separation
- `agora/channels/slack/provider.test.ts` — 8 tests: capabilities, config gating, lifecycle safety

---

### Phase 4: Message Tool + Outbound Routing ✅

**Scope:** The `message` tool currently hardcodes Signal. After this phase, it routes to the correct channel based on target format, and agents can send to Slack channels/users.

**Changes:**

- Refactor `organon/built-in/message.ts` — accept channel-prefixed targets
  - `slack:C0123456789` → Slack channel
  - `slack:U0123456789` → Slack DM
  - `slack:@username` → Slack DM (resolved)
  - `+1234567890` or `signal:+1234567890` → Signal (backward compatible)
  - `group:...` → Signal group (backward compatible)
- Create `src/agora/routing.ts` — target format parsing, channel resolution
- Wire message tool to agora registry instead of direct Signal sender
- Update `voice_reply` tool to note Signal-only constraint

**Acceptance criteria:**
- [x] `message` tool sends to Slack when target starts with `slack:`
- [x] `message` tool sends to Signal for existing target formats (backward compat)
- [x] Error handling for invalid targets, unconfigured channels
- [x] Agents can proactively message Slack channels and users

**Tests:**
- `agora/routing.test.ts` — target parsing, channel resolution (24 tests)
- `organon/built-in/message.test.ts` — multi-channel routing (18 tests)

---

### Phase 5: Streaming + Reactions ✅

**Scope:** Native Slack text streaming (progressive message updates while the agent thinks) and reaction support (ack emoji while processing).

**Changes:**

- `src/agora/channels/slack/streaming.ts` — native Slack streaming via ChatStreamer
  - `startSlackStream()` / `appendSlackStream()` / `stopSlackStream()`
  - Uses `@slack/web-api` ChatStreamer (chat.startStream / appendStream / stopStream)
  - Lazy-started on first `text_delta` event
  - Automatic thread creation for channel messages (streaming requires thread_ts)
  - Falls back to normal send on stream error
- `src/agora/channels/slack/reactions.ts` — idempotent reaction add/remove
  - `addSlackReaction()` / `removeSlackReaction()`
  - Handles `already_reacted` and `no_reaction` gracefully
- Streaming dispatch in `listener.ts`
  - Consumes `TurnStreamEvent` async iterable from `dispatchStream`
  - Pipes `text_delta` → `appendSlackStream()` with markdown→mrkdwn conversion
  - Handles `turn_complete`, `turn_abort`, `error` events
  - Cleans up placeholder messages when no content was streamed
- Processing reaction lifecycle in `listener.ts`
  - ⏳ added on message receive, removed on turn complete (finally block)
- Config toggles in `SlackChannelConfig` schema:
  - `streaming: boolean` (default: true)
  - `reactions.enabled: boolean` (default: true)
  - `reactions.processingEmoji: string` (default: "hourglass_flowing_sand")

**Acceptance criteria:**
- [x] Agent responses stream progressively in Slack via ChatStreamer
- [x] Streaming gracefully falls back on error or unsupported workspace
- [x] ⏳ reaction appears while agent is processing
- [x] Reaction removed on completion (via finally block)
- [x] Streaming and reactions independently toggleable via config

**Tests:**
- `agora/channels/slack/streaming.test.ts` — 11 tests (lifecycle, append guards, stop idempotency)
- `agora/channels/slack/reactions.test.ts` — 7 tests (add/remove, idempotency, error handling)

---

### Phase 6: Access Control + DM Pairing ✅

**Scope:** Slack-specific access control — DM policies, channel allowlists, admin-only commands. Pairing flow for new DM users.

**Changes:**

- DM policy enforcement in Slack listener via `checkDmAccess()`
  - `open` — respond to all DMs
  - `allowlist` — check user ID against `allowedUsers` config
  - `pairing` — static allowlist → dynamic approved contacts → challenge flow
  - `disabled` — silently drop all DMs
- Channel allowlist enforcement (implemented in Phase 3, verified in Phase 6)
  - `groupPolicy: allowlist` + `allowedChannels` config
  - Mention-gating: require @mention unless in-thread or configured otherwise
- Pairing flow for Slack DMs via `initiatePairing()`
  - Unknown user sends DM → `createContactRequest()` in SessionStore → challenge code sent via Slack DM
  - Admin approves via `!approve <code>` command (shared CommandRegistry)
  - Approved contacts stored in `approved_contacts` table, checked on subsequent DMs
- `!command` handling in Slack listener
  - Shared `CommandRegistry` commands (`!approve`, `!deny`, `!contacts`, `!status`, etc.)
  - Admin gating: `adminOnly` commands require user ID in `allowedUsers`
  - Replies sent via `webClient.chat.postMessage` with thread context
  - Signal-specific fields stubbed (`client`, `target`) — commands using only `store` work correctly

**Acceptance criteria:**
- [x] DM policy respected (open, allowlist, pairing, disabled)
- [x] Channel allowlist enforced
- [x] Mention-gating works in channels
- [x] Pairing flow guides new DM users with challenge code
- [x] Admin can approve pairings via `!approve` in any channel (shared CommandRegistry)
- [ ] Policy changes via config reload without restart (deferred — cross-cutting concern for separate spec)

**Tests:**
- `agora/channels/slack/access.test.ts` — 21 tests (DM policies, pairing flow, channel allowlists, command handling, admin checks)

---

## Dependency Graph

```
Phase 1 (agora core + Signal refactor) — prerequisite for everything
  ├── Phase 2 (config + CLI) — can overlap late Phase 1
  │     └── Phase 3 (Slack core messaging) — needs config schema
  │           ├── Phase 4 (message tool + routing) — needs working send
  │           ├── Phase 5 (streaming + reactions) — needs working provider
  │           └── Phase 6 (access control + pairing) — needs working provider
```

Phase 1 is the critical path. Phases 4, 5, 6 are independent of each other once Phase 3 lands.

---

## Architecture Impact

### New Module: agora

Add to ARCHITECTURE.md module table:

| Module | Domain | Files | Public Surface |
|--------|--------|-------|----------------|
| `agora` | Channel abstraction — provider interface, registry, routing, CLI onboarding | ~15 | `AgoraRegistry`, `ChannelProvider`, `ChannelSendParams`, CLI commands |

### Initialization Order Change

Current: semeion initialized directly in `startRuntime`

After: agora initialized in `startRuntime`, semeion registered as provider via agora

```
taxis → mneme → hermeneus → organon → nous → dianoia → prostheke → daemon
                                                                      ↑
                                              agora initialized in startRuntime
                                              ├── registers SignalChannelProvider (semeion)
                                              └── registers SlackChannelProvider
```

### Dependency Rule Additions

| Module | May Import |
|--------|-----------|
| `agora` | `koina`, `taxis`, `mneme`, `nous` (InboundMessage type), `organon` (command types) |
| `semeion` | unchanged + `agora` (ChannelProvider type) |

### Config Schema Extension

```typescript
const ChannelsConfig = z.object({
  signal: SignalConfig,
  slack: SlackConfig.optional(),   // NEW
}).default({});
```

---

## Open Questions

1. **Slash commands in Slack.** Should Slack slash commands map to the existing semeion command registry, or does Slack get its own command surface? Recommendation: shared `CommandRegistry` in agora, populated by both channels. Slash commands in Slack are just a different trigger for the same commands.

2. **Multi-workspace Slack.** Do we need multi-account Slack support (analogous to Signal multi-account)? Recommendation: defer. Single workspace is sufficient. The config schema should support it structurally (`accounts: { default: ... }`) so we don't paint ourselves into a corner.

3. **Web UI awareness.** The web UI shows sessions with channel metadata. Slack sessions should display with Slack-specific context (channel name, thread link). This is UI work that can happen incrementally after Phase 3.

4. **Event bus integration.** Should channel events (connect, disconnect, message received, message sent) emit on the global event bus? Yes — this enables the watchdog to monitor channel health. Define event format in Phase 1.

---

## References

- Issue #210 — Investigate Slack integration
- `docs/gnomon.md` — Naming system and philosophy
- `docs/ARCHITECTURE.md` — Module dependency matrix
- **OpenClaw** (`github.com/openclaw/openclaw`, MIT, 236k stars) — production-grade multi-channel assistant with Slack integration. Their `src/slack/` (~15,900 lines across 40+ files) is the primary reference implementation. Key files: `monitor/provider.ts` (Socket Mode bootstrap), `send.ts` (outbound delivery), `format.ts` (mrkdwn conversion), `monitor/message-handler.ts` (inbound debouncing). Full analysis: `nous/syn/context/openclaw-slack-reference.md`
- `@slack/bolt` — https://slack.dev/bolt-js/
- Slack Socket Mode — https://api.slack.com/apis/socket-mode
- Slack Events API — https://api.slack.com/events-api
