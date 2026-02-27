// Agora — channel abstraction types (Spec 34)
//
// ChannelProvider is the contract every channel (Signal, Slack, etc.) implements.
// The agora doesn't know about Signal or Slack — it knows about ChannelProviders.

import type { InboundMessage, TurnOutcome, TurnStreamEvent } from "../nous/pipeline/types.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import type { SessionStore } from "../mneme/store.js";
import type { CommandRegistry } from "../semeion/commands.js";
import type { NousManager } from "../nous/manager.js";

// ---------------------------------------------------------------------------
// Capabilities — what a channel can and can't do
// ---------------------------------------------------------------------------

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
  /** Supports rich formatting beyond markdown (blocks, embeds) */
  richFormatting: boolean;
  /** Max text length per message (0 = unlimited) */
  maxTextLength: number;
}

// ---------------------------------------------------------------------------
// Context — what agora provides to each channel on start
// ---------------------------------------------------------------------------

export interface ChannelContext {
  /** Dispatch an inbound message through the nous pipeline */
  dispatch: (msg: InboundMessage) => Promise<TurnOutcome>;
  /** Stream an inbound message through the nous pipeline */
  dispatchStream?: (msg: InboundMessage) => AsyncIterable<TurnStreamEvent>;
  /** The full runtime config */
  config: AletheiaConfig;
  /** Session store for thread/session lookups */
  store: SessionStore;
  /** NousManager — for manager-level operations (thread resolution, etc.) */
  manager: NousManager;
  /** Abort signal for graceful shutdown */
  abortSignal: AbortSignal;
  /** Command registry for slash-command handling */
  commands?: CommandRegistry;
  /** Watchdog ref (may be set after start) */
  watchdog?: import("../daemon/watchdog.js").Watchdog | null;
}

// ---------------------------------------------------------------------------
// Send parameters — what the caller provides to send outbound
// ---------------------------------------------------------------------------

export interface ChannelSendParams {
  /** Target identifier (channel-specific format) */
  to: string;
  /** Message text (markdown) */
  text: string;
  /** Account ID within the channel (for multi-account setups) */
  accountId?: string;
  /** Thread/reply context */
  threadId?: string;
  /** File attachments (paths) */
  attachments?: string[];
  /** Sender identity override (agent name, emoji) */
  identity?: ChannelIdentity;
  /** Disable markdown formatting */
  markdown?: boolean;
}

export interface ChannelSendResult {
  /** Whether the message was sent successfully */
  sent: boolean;
  /** Error message if send failed */
  error?: string;
}

export interface ChannelIdentity {
  name?: string;
  emoji?: string;
  avatarUrl?: string;
}

// ---------------------------------------------------------------------------
// Probe — health check
// ---------------------------------------------------------------------------

export interface ChannelProbeResult {
  ok: boolean;
  latencyMs?: number;
  error?: string;
  details?: Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// The provider contract
// ---------------------------------------------------------------------------

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
   */
  start(ctx: ChannelContext): Promise<void>;

  /**
   * Send a message outbound through this channel.
   * Called by the agora registry when routing determines this channel.
   */
  send(params: ChannelSendParams): Promise<ChannelSendResult>;

  /**
   * Send a typing indicator (if supported).
   */
  sendTyping?(to: string, accountId?: string, stop?: boolean): Promise<void>;

  /**
   * Gracefully stop the channel.
   */
  stop(): Promise<void>;

  /**
   * Health probe — is this channel connected and functional?
   */
  probe?(): Promise<ChannelProbeResult>;
}
