// Slack inbound message listener (Spec 34, Phase 3)
//
// Handles: message events, app_mention events, DM detection, thread tracking,
// mention gating, inbound debouncing, message dedup.
//
// Reference: OpenClaw src/slack/monitor/message-handler.ts, events/messages.ts

import type { App } from "@slack/bolt";
import type { WebClient } from "@slack/web-api";
import { createLogger } from "../../../koina/logger.js";
import type { InboundMessage } from "../../../nous/pipeline/types.js";
import type { ChannelContext } from "../../types.js";
import { mrkdwnToMarkdown, stripBotMention } from "./format.js";

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
  /** Agora channel context — for dispatching to nous */
  ctx: ChannelContext;
  /** Require @mention in group channels (default: true) */
  requireMention: boolean;
  /** DM policy */
  dmPolicy: "open" | "allowlist" | "disabled";
  /** Allowed user IDs for allowlist policy */
  allowedUsers: string[];
  /** Allowed channel IDs for allowlist policy */
  allowedChannels: string[];
  /** Group channel policy */
  groupPolicy: "open" | "allowlist" | "disabled";
  /** Debounce window in ms (default: 1500) */
  debounceMs?: number;
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

function isAllowedDm(userId: string, config: SlackListenerConfig): boolean {
  switch (config.dmPolicy) {
    case "open":
      return true;
    case "allowlist":
      return config.allowedUsers.includes(userId);
    case "disabled":
      return false;
    default:
      return false;
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

    // Only include threadTs when defined (exactOptionalPropertyTypes)
    const buildParams: Parameters<typeof buildInboundMessage>[0] = {
      text: cleanText,
      channelId,
      userId,
      channelType,
    };
    if (threadTs) buildParams.threadTs = threadTs;
    const inbound = buildInboundMessage(buildParams);

    try {
      await ctx.dispatch(inbound);
    } catch (err) {
      log.error(`Failed to dispatch Slack message: ${err instanceof Error ? err.message : err}`);
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
      if (!isAllowedDm(senderId, config)) {
        log.debug(`Slack DM from ${senderId} blocked by policy`);
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
