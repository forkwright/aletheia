// Slack inbound message listener (Spec 34, Phase 3+5+6)
//
// Handles: message events, app_mention events, DM detection, thread tracking,
// mention gating, inbound debouncing, message dedup, streaming dispatch,
// processing reactions, DM pairing flow.
//
// Reference: OpenClaw src/slack/monitor/message-handler.ts, events/messages.ts

import type { App } from "@slack/bolt";
import type { WebClient } from "@slack/web-api";
import { createLogger } from "../../../koina/logger.js";
import type { InboundMessage } from "../../../nous/pipeline/types.js";
import type { ChannelContext } from "../../types.js";
import { mrkdwnToMarkdown, markdownToMrkdwn, stripBotMention } from "./format.js";
import { startSlackStream, appendSlackStream, stopSlackStream, type SlackStreamSession } from "./streaming.js";
import { addSlackReaction, removeSlackReaction } from "./reactions.js";

const log = createLogger("agora:slack:listener");

// ---------------------------------------------------------------------------
// Message dedup — prevent processing the same Slack event twice
// (OpenClaw pattern: markMessageSeen)
// ---------------------------------------------------------------------------

const SEEN_TTL_MS = 60_000; // Keep seen messages for 60s
const SEEN_MAX_SIZE = 5000;

class MessageSeenSet {
  private seen = new Map<string, number>();

  /** Returns true if this message was already seen (= should be dropped) */
  markSeen(channelId: string, ts: string | undefined): boolean {
    if (!ts) return false;
    const key = `${channelId}:${ts}`;
    if (this.seen.has(key)) return true;
    this.seen.set(key, Date.now());
    this.prune();
    return false;
  }

  private prune(): void {
    if (this.seen.size < SEEN_MAX_SIZE) return;
    const cutoff = Date.now() - SEEN_TTL_MS;
    for (const [key, time] of this.seen) {
      if (time < cutoff) this.seen.delete(key);
    }
  }
}

// ---------------------------------------------------------------------------
// Inbound debouncer — coalesce rapid messages from the same user/thread
// (OpenClaw pattern: createInboundDebouncer)
// ---------------------------------------------------------------------------

interface DebouncedMessage {
  message: SlackMessage;
  wasMentioned: boolean;
}

interface DebounceEntry {
  messages: DebouncedMessage[];
  timer: ReturnType<typeof setTimeout>;
}

export class InboundDebouncer {
  private pending = new Map<string, DebounceEntry>();

  constructor(
    private readonly debounceMs: number,
    private readonly onFlush: (key: string, messages: DebouncedMessage[]) => Promise<void>,
  ) {}

  enqueue(key: string, msg: DebouncedMessage): void {
    const existing = this.pending.get(key);
    if (existing) {
      clearTimeout(existing.timer);
      existing.messages.push(msg);
      existing.timer = setTimeout(() => this.flush(key), this.debounceMs);
    } else {
      const entry: DebounceEntry = {
        messages: [msg],
        timer: setTimeout(() => this.flush(key), this.debounceMs),
      };
      this.pending.set(key, entry);
    }
  }

  private flush(key: string): void {
    const entry = this.pending.get(key);
    if (!entry) return;
    this.pending.delete(key);
    this.onFlush(key, entry.messages).catch((err) => {
      log.error(`Debounce flush failed for ${key}: ${err instanceof Error ? err.message : err}`);
    });
  }

  /** Flush everything immediately (for shutdown) */
  flushAll(): void {
    for (const [key, entry] of this.pending) {
      clearTimeout(entry.timer);
      this.onFlush(key, entry.messages).catch(() => {});
    }
    this.pending.clear();
  }
}

// ---------------------------------------------------------------------------
// Slack event types (subset we care about)
// ---------------------------------------------------------------------------

interface SlackMessage {
  type?: string;
  subtype?: string;
  user?: string;
  bot_id?: string;
  text?: string;
  ts?: string;
  thread_ts?: string;
  channel?: string;
  channel_type?: string;
  files?: Array<{
    url_private?: string;
    url_private_download?: string;
    name?: string;
    mimetype?: string;
    size?: number;
  }>;
  // Set on app_mention events
  event_ts?: string;
}

// ---------------------------------------------------------------------------
// Listener configuration
// ---------------------------------------------------------------------------

export interface SlackListenerConfig {
  /** Slack Bolt app instance */
  app: App;
  /** WebClient for API calls */
  webClient: WebClient;
  /** Bot's own user ID (from auth.test) — for self-filtering and mention stripping */
  botUserId: string;
  /** Team ID from auth.test — needed for streaming */
  teamId: string;
  /** Agora channel context — for dispatching to nous */
  ctx: ChannelContext;
  /** Require @mention in group channels (default: true) */
  requireMention: boolean;
  /** DM policy */
  dmPolicy: "open" | "allowlist" | "pairing" | "disabled";
  /** Allowed user IDs for allowlist policy */
  allowedUsers: string[];
  /** Allowed channel IDs for allowlist policy */
  allowedChannels: string[];
  /** Group channel policy */
  groupPolicy: "open" | "allowlist" | "disabled";
  /** Debounce window in ms (default: 1500) */
  debounceMs?: number;
  /** Enable native Slack text streaming (Phase 5) */
  streaming: boolean;
  /** Reaction config (Phase 5) */
  reactions: {
    enabled: boolean;
    processingEmoji: string;
  };
}

// ---------------------------------------------------------------------------
// Build InboundMessage from Slack event
// ---------------------------------------------------------------------------

function buildInboundMessage(params: {
  text: string;
  channelId: string;
  userId: string;
  channelType: string;
  threadTs?: string;
  files?: SlackMessage["files"];
}): InboundMessage {
  const { text, channelId, userId, channelType, threadTs } = params;

  // Determine peer kind from Slack channel type
  let peerKind: string;
  switch (channelType) {
    case "im":
      peerKind = "direct";
      break;
    case "mpim":
      peerKind = "group_dm";
      break;
    default:
      peerKind = "channel";
  }

  // Build message — only include threadId when defined (exactOptionalPropertyTypes)
  const msg: InboundMessage = {
    text,
    channel: "slack",
    peerId: channelId,
    peerKind,
    accountId: userId,
  };
  if (threadTs) msg.threadId = threadTs;
  return msg;
}

// ---------------------------------------------------------------------------
// Authorization checks
// ---------------------------------------------------------------------------

function isDirectMessage(channelType?: string): boolean {
  return channelType === "im";
}

/**
 * Check if a DM is allowed by policy. Returns:
 * - "allowed" — dispatch the message
 * - "blocked" — silently drop it
 * - "pairing" — initiate pairing flow (caller should send challenge)
 */
function checkDmAccess(userId: string, config: SlackListenerConfig): "allowed" | "blocked" | "pairing" {
  switch (config.dmPolicy) {
    case "open":
      return "allowed";
    case "allowlist":
      return config.allowedUsers.includes(userId) ? "allowed" : "blocked";
    case "pairing": {
      // Static allowlist first
      if (config.allowedUsers.includes(userId)) return "allowed";
      // Dynamic approved contacts via store
      if (config.ctx.store.isApprovedContact(userId, "slack")) return "allowed";
      return "pairing";
    }
    case "disabled":
      return "blocked";
    default:
      return "blocked";
  }
}

/**
 * Initiate a pairing flow for an unknown Slack DM sender.
 * Creates a contact request in the store and sends a challenge message.
 */
async function initiatePairing(
  userId: string,
  channelId: string,
  config: SlackListenerConfig,
): Promise<void> {
  const { store } = config.ctx;
  const { id, challengeCode } = store.createContactRequest(
    userId,
    userId, // senderName — Slack user ID; could resolve display name later
    "slack",
    undefined, // accountId — single Slack workspace for now
  );
  log.info(`Pairing request #${id} from Slack user ${userId} (code: ${challengeCode})`);

  try {
    await config.webClient.chat.postMessage({
      channel: channelId,
      text: `I don't recognize you yet. Ask an admin to approve your access with code: \`${challengeCode}\`\n\nThey can use \`!approve ${challengeCode}\` in chat or the web UI.`,
    });
  } catch (err) {
    log.warn(`Failed to send pairing message to ${userId}: ${err instanceof Error ? err.message : err}`);
  }
}

function isAllowedChannel(channelId: string, config: SlackListenerConfig): boolean {
  switch (config.groupPolicy) {
    case "open":
      return true;
    case "allowlist":
      return config.allowedChannels.includes(channelId);
    case "disabled":
      return false;
    default:
      return false;
  }
}

// ---------------------------------------------------------------------------
// Streaming dispatch — consume TurnStreamEvents and pipe to Slack ChatStreamer
// ---------------------------------------------------------------------------

interface StreamDispatchParams {
  ctx: ChannelContext;
  inbound: InboundMessage;
  webClient: WebClient;
  channelId: string;
  threadTs?: string | undefined;
  userId: string;
  channelType: string;
  teamId: string;
}

/**
 * Dispatch an inbound message using streaming. Consumes TurnStreamEvent async
 * iterable and pipes text_delta events to Slack's ChatStreamer.
 *
 * For channel messages without a thread, we first post a placeholder to create
 * a thread_ts, then stream within that thread. For DMs and existing threads,
 * we stream directly.
 *
 * Falls back to non-streaming dispatch on any stream error.
 */
async function dispatchWithStreaming(params: StreamDispatchParams): Promise<void> {
  const { ctx, inbound, webClient, channelId, threadTs, userId, channelType, teamId } = params;

  // Streaming requires thread_ts. For DMs, every message is implicitly threaded.
  // For channels without an existing thread, we need to create one first.
  let streamThreadTs = threadTs;
  if (!streamThreadTs && channelType !== "im") {
    // Post an initial message to start a thread — the response ts becomes our thread
    try {
      const initial = await webClient.chat.postMessage({
        channel: channelId,
        text: "…", // Placeholder — will be replaced by streaming content
      });
      streamThreadTs = initial.ts;
    } catch (err) {
      log.warn(`Failed to create thread for streaming, falling back to normal dispatch: ${err instanceof Error ? err.message : err}`);
      await ctx.dispatch(inbound);
      return;
    }
  }

  // For DMs, use the message's own ts as the thread anchor
  if (!streamThreadTs && channelType === "im") {
    // In DMs, we stream as a reply — we need a thread_ts
    // Post a placeholder that becomes the thread root
    try {
      const initial = await webClient.chat.postMessage({
        channel: channelId,
        text: "…",
      });
      streamThreadTs = initial.ts;
    } catch (err) {
      log.warn(`Failed to create DM thread for streaming, falling back: ${err instanceof Error ? err.message : err}`);
      await ctx.dispatch(inbound);
      return;
    }
  }

  if (!streamThreadTs) {
    log.warn("No thread_ts available for streaming — falling back to normal dispatch");
    await ctx.dispatch(inbound);
    return;
  }

  let session: SlackStreamSession | null = null;
  let hasContent = false;

  try {
    // Start consuming the stream
    const stream = ctx.dispatchStream!(inbound);

    for await (const event of stream) {
      switch (event.type) {
        case "text_delta": {
          // Lazy-start the stream on first text
          if (!session) {
            const streamParams: Parameters<typeof startSlackStream>[0] = {
              client: webClient,
              channel: channelId,
              threadTs: streamThreadTs!,
              text: markdownToMrkdwn(event.text),
              teamId,
            };
            if (channelType === "im") streamParams.userId = userId;
            session = await startSlackStream(streamParams);
            hasContent = true;
          } else {
            await appendSlackStream({
              session,
              text: markdownToMrkdwn(event.text),
            });
            hasContent = true;
          }
          break;
        }

        case "turn_complete": {
          // Finalize the stream
          if (session && !session.stopped) {
            await stopSlackStream({ session });
          } else if (!hasContent && event.outcome?.text) {
            // Turn completed with text but no text_delta events were emitted
            // (e.g., cached response). Post as normal message.
            await webClient.chat.postMessage({
              channel: channelId,
              thread_ts: streamThreadTs!,
              text: markdownToMrkdwn(event.outcome.text),
            });
          }
          break;
        }

        case "turn_abort":
        case "error": {
          // Clean up the stream on error
          if (session && !session.stopped) {
            const errText = event.type === "error" ? event.message : event.reason;
            await stopSlackStream({
              session,
              text: `⚠️ ${errText ?? "Processing interrupted"}`,
            });
          }
          break;
        }

        // Silently consume other events (tool_start, tool_result, thinking_delta, etc.)
        default:
          break;
      }
    }

    // Safety: ensure stream is stopped if turn_complete never fired
    if (session && !session.stopped) {
      await stopSlackStream({ session });
    }

    // Clean up the placeholder if we never streamed any content
    if (!hasContent && streamThreadTs && !threadTs) {
      // Delete the "…" placeholder we created
      await webClient.chat.delete({
        channel: channelId,
        ts: streamThreadTs,
      }).catch(() => {}); // Best-effort
    }
  } catch (err) {
    log.error(`Streaming dispatch failed: ${err instanceof Error ? err.message : err}`);

    // Try to stop any active stream
    if (session && !session.stopped) {
      await stopSlackStream({ session }).catch(() => {});
    }

    // Fall back to non-streaming dispatch
    log.info("Falling back to non-streaming dispatch after stream failure");
    try {
      await ctx.dispatch(inbound);
    } catch (fallbackErr) {
      log.error(`Fallback dispatch also failed: ${fallbackErr instanceof Error ? fallbackErr.message : fallbackErr}`);
    }
  }
}

// ---------------------------------------------------------------------------
// Public: register event handlers
// ---------------------------------------------------------------------------

/**
 * Register Slack event handlers on the Bolt app.
 * Handles message events and app_mention events.
 */
export function registerSlackListeners(config: SlackListenerConfig): InboundDebouncer {
  const seenSet = new MessageSeenSet();
  const { app, botUserId, ctx } = config;
  const debounceMs = config.debounceMs ?? 1500;

  // Debouncer: coalesce rapid messages, then dispatch to nous
  const debouncer = new InboundDebouncer(debounceMs, async (_key, messages) => {
    const last = messages[messages.length - 1];
    if (!last) return;

    // Combine text from debounced messages
    const combinedText =
      messages.length === 1
        ? (last.message.text ?? "")
        : messages
            .map((m) => m.message.text ?? "")
            .filter(Boolean)
            .join("\n");

    const wasMentioned = messages.some((m) => m.wasMentioned);
    const channelId = last.message.channel ?? "";
    const userId = last.message.user ?? last.message.bot_id ?? "unknown";
    const channelType = last.message.channel_type ?? "channel";
    const threadTs = last.message.thread_ts;

    // Strip bot mention from combined text
    let cleanText = wasMentioned
      ? stripBotMention(combinedText, botUserId)
      : combinedText;

    // Convert mrkdwn → markdown for the nous pipeline
    cleanText = mrkdwnToMarkdown(cleanText);

    if (!cleanText.trim()) return;

    // Check for !command prefix — handle via shared CommandRegistry
    if ((cleanText.startsWith("!") || cleanText.startsWith("/")) && ctx.commands) {
      const match = ctx.commands.match(cleanText);
      if (match) {
        const { handler, args: cmdArgs } = match;

        // Admin check — only allowedUsers can run admin commands
        if (handler.adminOnly && !config.allowedUsers.includes(userId)) {
          const deny: { channel: string; text: string; thread_ts?: string } = {
            channel: channelId,
            text: "⛔ This command requires admin access.",
          };
          const denyThread = last.message.thread_ts ?? last.message.ts;
          if (denyThread) deny.thread_ts = denyThread;
          await config.webClient.chat.postMessage(deny);
          return;
        }

        try {
          // Build a CommandContext with Slack-appropriate values.
          // Signal-specific fields (client, target) are stubbed — commands that
          // need them will fail gracefully; pairing commands only need store.
          const cmdCtx = {
            sender: userId,
            senderName: userId,
            isGroup: channelType !== "im",
            accountId: "slack",
            target: {} as never,   // Not used — Slack replies via webClient
            client: {} as never,   // Not used — Slack replies via webClient
            store: ctx.store,
            config: ctx.config,
            manager: ctx.manager,
            watchdog: ctx.watchdog ?? null,
            skills: null,
          };
          const result = await handler.execute(cmdArgs, cmdCtx);
          if (result) {
            const threadTarget = last.message.thread_ts ?? last.message.ts;
            const postArgs: { channel: string; text: string; thread_ts?: string } = {
              channel: channelId,
              text: markdownToMrkdwn(result),
            };
            if (threadTarget) postArgs.thread_ts = threadTarget;
            await config.webClient.chat.postMessage(postArgs);
          }
        } catch (err) {
          log.error(`Slack command !${handler.name} failed: ${err instanceof Error ? err.message : err}`);
        }
        return; // Commands don't dispatch to nous
      }
    }

    // Only include threadTs when defined (exactOptionalPropertyTypes)
    const buildParams: Parameters<typeof buildInboundMessage>[0] = {
      text: cleanText,
      channelId,
      userId,
      channelType,
    };
    if (threadTs) buildParams.threadTs = threadTs;
    const inbound = buildInboundMessage(buildParams);

    // Resolve message timestamp for reactions (use the last message's ts)
    const messageTs = last.message.ts ?? "";

    try {
      // Add processing reaction if enabled
      if (config.reactions.enabled && messageTs) {
        await addSlackReaction({
          client: config.webClient,
          channel: channelId,
          timestamp: messageTs,
          emoji: config.reactions.processingEmoji,
        });
      }

      // Use streaming dispatch if enabled and available
      if (config.streaming && ctx.dispatchStream) {
        await dispatchWithStreaming({
          ctx,
          inbound,
          webClient: config.webClient,
          channelId,
          threadTs,
          userId,
          channelType,
          teamId: config.teamId,
        });
      } else {
        await ctx.dispatch(inbound);
      }
    } catch (err) {
      log.error(`Failed to dispatch Slack message: ${err instanceof Error ? err.message : err}`);
    } finally {
      // Remove processing reaction
      if (config.reactions.enabled && messageTs) {
        await removeSlackReaction({
          client: config.webClient,
          channel: channelId,
          timestamp: messageTs,
          emoji: config.reactions.processingEmoji,
        }).catch(() => {}); // Best-effort removal
      }
    }
  });

  // --- message event handler ---
  app.event("message", async ({ event }) => {
    const message = event as SlackMessage;

    // Skip non-message subtypes (message_changed, message_deleted, etc.)
    if (
      message.subtype &&
      message.subtype !== "file_share" &&
      message.subtype !== "me_message"
    ) {
      return;
    }

    // Skip own messages
    const senderId = message.user ?? message.bot_id;
    if (!senderId || senderId === botUserId) return;

    // Dedup
    if (seenSet.markSeen(message.channel ?? "", message.ts)) return;

    const channelType = message.channel_type ?? "";
    const channelId = message.channel ?? "";

    // Authorization check
    if (isDirectMessage(channelType)) {
      const access = checkDmAccess(senderId, config);
      if (access === "blocked") {
        log.debug(`Slack DM from ${senderId} blocked by policy`);
        return;
      }
      if (access === "pairing") {
        await initiatePairing(senderId, channelId, config);
        return;
      }
    } else {
      if (!isAllowedChannel(channelId, config)) {
        log.debug(`Slack message in ${channelId} blocked by channel policy`);
        return;
      }

      // Mention gating: in channels, require @mention unless configured otherwise
      if (config.requireMention) {
        const text = message.text ?? "";
        const isMention = text.includes(`<@${botUserId}>`);
        const isThread = Boolean(message.thread_ts);
        // Allow thread replies without mention (conversation continuity)
        if (!isMention && !isThread) {
          return;
        }
      }
    }

    // Build debounce key: same user + same thread/channel
    const threadKey = message.thread_ts
      ? `${channelId}:${message.thread_ts}`
      : channelId;
    const debounceKey = `slack:${threadKey}:${senderId}`;

    const wasMentioned = (message.text ?? "").includes(`<@${botUserId}>`);

    debouncer.enqueue(debounceKey, { message, wasMentioned });
  });

  // --- app_mention event handler ---
  // This fires for @mentions in channels. We process it as a message with wasMentioned=true.
  app.event("app_mention", async ({ event }) => {
    const mention = event as SlackMessage;
    const senderId = mention.user ?? mention.bot_id;
    if (!senderId || senderId === botUserId) return;

    const channelId = mention.channel ?? "";

    // Dedup (same event can trigger both message + app_mention)
    if (seenSet.markSeen(channelId, mention.ts ?? mention.event_ts)) return;

    // Channel authorization
    if (!isAllowedChannel(channelId, config)) {
      log.debug(`Slack mention in ${channelId} blocked by channel policy`);
      return;
    }

    const threadKey = mention.thread_ts
      ? `${channelId}:${mention.thread_ts}`
      : channelId;
    const debounceKey = `slack:${threadKey}:${senderId}`;

    debouncer.enqueue(debounceKey, {
      message: { ...mention, channel_type: "channel" },
      wasMentioned: true,
    });
  });

  log.info("Slack event listeners registered");
  return debouncer;
}
