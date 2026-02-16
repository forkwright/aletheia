// SSE event stream consumer — dispatches Signal messages to NousManager
import { createLogger } from "../koina/logger.js";
import type { NousManager, InboundMessage } from "../nous/manager.js";
import { SignalClient } from "./client.js";
import { sendMessage, sendTyping, sendReadReceipt, type SendTarget } from "./sender.js";
import type { SignalAccount } from "../taxis/schema.js";

const log = createLogger("semeion:listen");

export interface SignalEnvelope {
  sourceNumber?: string;
  sourceUuid?: string;
  sourceName?: string;
  timestamp?: number;
  dataMessage?: {
    timestamp?: number;
    message?: string;
    attachments?: Array<{
      id?: string;
      contentType?: string;
      filename?: string;
      size?: number;
    }>;
    mentions?: Array<{
      name?: string;
      number?: string;
      uuid?: string;
      start?: number;
      length?: number;
    }>;
    groupInfo?: { groupId?: string; groupName?: string };
    quote?: { text?: string };
  };
  editMessage?: {
    dataMessage?: SignalEnvelope["dataMessage"];
  };
  syncMessage?: unknown;
  reactionMessage?: {
    emoji?: string;
    targetTimestamp?: number;
    targetAuthor?: string;
    isRemove?: boolean;
  };
}

interface SseEvent {
  event?: string;
  data?: string;
  id?: string;
}

export interface ListenerOpts {
  accountId: string;
  account: SignalAccount;
  manager: NousManager;
  client: SignalClient;
  baseUrl: string;
  abortSignal?: AbortSignal;
  boundGroupIds?: Set<string>;
  onStatusRequest?: (target: SendTarget) => Promise<void>;
}

const MAX_CONCURRENT_TURNS = 3;
let activeTurns = 0;

export async function startListener(opts: ListenerOpts): Promise<void> {
  const { accountId, account, manager, client, baseUrl, abortSignal, boundGroupIds, onStatusRequest } = opts;
  const accountPhone = account.account ?? accountId;

  log.info(`Starting SSE listener for account ${accountId}`);

  const backoff = { min: 1000, max: 10000, current: 1000 };

  while (!abortSignal?.aborted) {
    try {
      await consumeEventStream(
        baseUrl,
        accountPhone,
        (envelope) => handleEnvelope(envelope, accountId, account, manager, client, boundGroupIds, onStatusRequest),
        abortSignal,
      );
      backoff.current = backoff.min;
    } catch (err) {
      if (abortSignal?.aborted) break;
      log.warn(`SSE stream error: ${err instanceof Error ? err.message : err}`);
    }

    if (abortSignal?.aborted) break;

    const jitter = backoff.current * (0.8 + Math.random() * 0.4);
    log.info(`Reconnecting SSE in ${Math.round(jitter)}ms`);
    await sleep(jitter, abortSignal);

    backoff.current = Math.min(backoff.current * 2, backoff.max);
  }

  log.info(`SSE listener stopped for account ${accountId}`);
}

async function consumeEventStream(
  baseUrl: string,
  account: string,
  onEnvelope: (envelope: SignalEnvelope) => void,
  abortSignal?: AbortSignal,
): Promise<void> {
  const url = `${baseUrl}/api/v1/events?account=${encodeURIComponent(account)}`;

  const res = await fetch(url, {
    headers: { Accept: "text/event-stream" },
    signal: abortSignal,
  });

  if (!res.ok) {
    throw new Error(`SSE connect failed: ${res.status} ${res.statusText}`);
  }

  if (!res.body) {
    throw new Error("SSE response has no body");
  }

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let currentEvent: SseEvent = {};

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });

      const lines = buffer.split("\n");
      buffer = lines.pop() ?? "";

      for (const rawLine of lines) {
        // Strip trailing \r (SSE spec allows \r\n line endings)
        const line = rawLine.replace(/\r$/, "");

        // Skip SSE comments
        if (line.startsWith(":")) continue;

        if (line === "") {
          if (currentEvent.data && (!currentEvent.event || currentEvent.event === "receive")) {
            try {
              const payload = JSON.parse(currentEvent.data);
              const envelope = payload?.envelope as SignalEnvelope | undefined;
              if (envelope) {
                onEnvelope(envelope);
              }
            } catch (parseErr) {
              log.warn(
                `Failed to parse SSE data: ${parseErr instanceof Error ? parseErr.message : parseErr}`,
              );
            }
          }
          currentEvent = {};
        } else if (line.startsWith("event:")) {
          currentEvent.event = line.slice(6).trim();
        } else if (line.startsWith("data:")) {
          // SSE spec: strip one leading space after colon if present
          const data = line.startsWith("data: ") ? line.slice(6) : line.slice(5);
          currentEvent.data = currentEvent.data
            ? currentEvent.data + "\n" + data
            : data;
        } else if (line.startsWith("id:")) {
          currentEvent.id = line.slice(3).trim();
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

function handleEnvelope(
  envelope: SignalEnvelope,
  accountId: string,
  account: SignalAccount,
  manager: NousManager,
  client: SignalClient,
  boundGroupIds?: Set<string>,
  onStatusRequest?: (target: SendTarget) => Promise<void>,
): void {
  if (envelope.syncMessage) return;

  const dataMessage =
    envelope.editMessage?.dataMessage ?? envelope.dataMessage;
  if (!dataMessage) return;

  // Accept messages with text, quoted text, or attachments
  const messageText = dataMessage.message
    ?? dataMessage.quote?.text
    ?? (dataMessage.attachments?.length ? "<attachment>" : null);
  if (!messageText) return;

  const sender = envelope.sourceNumber ?? envelope.sourceUuid;
  if (!sender) return;

  const accountPhone = account.account ?? accountId;
  if (sender === accountPhone) return;

  const isGroup = !!dataMessage.groupInfo?.groupId;
  const groupId = dataMessage.groupInfo?.groupId;

  if (!checkAccess(sender, isGroup, groupId, account, boundGroupIds)) {
    log.debug(`Blocked message from ${sender} (policy)`);
    return;
  }

  // Mention gating for groups — skip for bound groups (dedicated agent channels)
  const isBoundGroup = isGroup && groupId && boundGroupIds?.has(groupId);
  if (isGroup && !isBoundGroup && account.requireMention !== false) {
    const isMentioned = dataMessage.mentions?.some(
      (m) => m.number === accountPhone || normalizePhone(m.number ?? "") === normalizePhone(accountPhone),
    );
    if (!isMentioned) {
      log.debug(`Skipping group message — not mentioned`);
      return;
    }
  }

  let text = messageText;

  if (dataMessage.message && dataMessage.mentions?.length) {
    text = hydrateMentions(dataMessage.message, dataMessage.mentions, accountPhone);
  }

  // Append attachment info so the agent is aware
  if (dataMessage.attachments?.length) {
    for (const att of dataMessage.attachments) {
      const name = att.filename ?? "unnamed";
      const type = att.contentType ?? "unknown";
      const size = att.size ? `${Math.round(att.size / 1024)}KB` : "unknown size";
      text += `\n[Attachment: ${name} (${type}, ${size})${att.id ? ` id=${att.id}` : ""}]`;
    }
  }

  log.info(
    `Inbound ${isGroup ? "group" : "DM"} from ${envelope.sourceName ?? sender}: ${text.slice(0, 80)}`,
  );

  const target: SendTarget = {
    account: accountPhone,
    recipient: isGroup ? undefined : sender,
    groupId: isGroup ? groupId : undefined,
  };

  // Status command — bypass agent turn pipeline
  if (onStatusRequest && !isGroup && text.trim().toLowerCase() === "status") {
    log.info("Status command received, handling directly");
    onStatusRequest(target).catch((err) =>
      log.warn(`Status command failed: ${err instanceof Error ? err.message : err}`),
    );
    return;
  }

  if (account.sendReadReceipts && !isGroup && envelope.timestamp) {
    sendReadReceipt(client, target, envelope.timestamp).catch((err) =>
      log.warn(`Read receipt failed: ${err}`),
    );
  }

  sendTyping(client, target).catch((err) =>
    log.warn(`Typing indicator failed: ${err}`),
  );

  // For DMs, prefer UUID for routing (bindings use UUIDs)
  const peerId = isGroup ? groupId! : (envelope.sourceUuid ?? sender);
  const peerKind = isGroup ? "group" : "dm";
  const sessionKey = `signal:${peerId}`;

  const msg: InboundMessage = {
    text,
    channel: "signal",
    peerId,
    peerKind,
    accountId,
    sessionKey,
  };

  // Concurrency guard — reject if at limit to protect API and SQLite
  if (activeTurns >= MAX_CONCURRENT_TURNS) {
    log.warn(`Concurrency limit reached (${activeTurns}/${MAX_CONCURRENT_TURNS}), dropping message`);
    sendMessage(client, target, "I'm handling several conversations right now. Give me a moment and try again.", { markdown: false })
      .catch((err) => log.warn(`Failed to send busy message: ${err}`));
    return;
  }

  // Fire-and-forget — the session mutex in manager.ts serializes same-session turns,
  // while different nous/sessions process concurrently
  activeTurns++;
  processTurn(manager, msg, client, target).finally(() => {
    activeTurns--;
  });
}

async function processTurn(
  manager: NousManager,
  msg: InboundMessage,
  client: SignalClient,
  target: SendTarget,
): Promise<void> {
  try {
    const outcome = await manager.handleMessage(msg);

    sendTyping(client, target, true).catch(() => {});

    if (outcome.text) {
      await sendMessage(client, target, outcome.text);
    }

    log.info(
      `Turn complete: ${outcome.nousId} session=${outcome.sessionId} tools=${outcome.toolCalls} in=${outcome.inputTokens} out=${outcome.outputTokens}`,
    );
  } catch (err) {
    sendTyping(client, target, true).catch(() => {});
    log.error(`Turn failed: ${err instanceof Error ? err.message : err}`);
    if (err instanceof Error && err.stack) log.error(err.stack);

    try {
      await sendMessage(
        client,
        target,
        "I encountered an error processing that message. Please try again.",
        { markdown: false },
      );
    } catch (sendErr) {
      log.error(`Failed to send error message: ${sendErr}`);
    }
  }
}

function checkAccess(
  sender: string,
  isGroup: boolean,
  groupId: string | undefined,
  account: SignalAccount,
  boundGroupIds?: Set<string>,
): boolean {
  if (isGroup) {
    if (account.groupPolicy === "disabled") return false;
    if (account.groupPolicy === "open") return true;
    // Allowlist mode: check explicit groupAllowFrom, then fall back to bindings
    if (groupId && isInAllowlist(groupId, account.groupAllowFrom)) return true;
    if (groupId && boundGroupIds?.has(groupId)) return true;
    return false;
  }

  if (account.dmPolicy === "disabled") return false;
  if (account.dmPolicy === "open") return true;
  if (account.dmPolicy === "pairing") {
    log.warn(`DM policy "pairing" not implemented — treating as "open"`);
    return true;
  }
  return isInAllowlist(sender, account.allowFrom);
}

function normalizePhone(phone: string): string {
  const digits = phone.replace(/\D/g, "");
  // US numbers: 10 digits → prepend country code
  if (digits.length === 10) return "1" + digits;
  return digits;
}

function isInAllowlist(
  sender: string,
  allowlist: Array<string | number>,
): boolean {
  if (!allowlist.length) return false;
  const senderNorm = normalizePhone(sender);

  for (const entry of allowlist) {
    const normalized = String(entry);
    if (normalized === "*") return true;
    if (sender === normalized) return true;
    if (senderNorm === normalizePhone(normalized)) return true;
  }

  return false;
}

function hydrateMentions(
  text: string,
  mentions: Array<{ name?: string; number?: string; uuid?: string; start?: number; length?: number }>,
  selfAccount?: string,
): string {
  let result = text;

  const sorted = [...mentions].sort((a, b) => (b.start ?? 0) - (a.start ?? 0));

  for (const mention of sorted) {
    if (mention.start == null || mention.length == null) continue;

    // Strip self-mentions (the bot's own mention placeholder)
    const isSelf =
      selfAccount &&
      (mention.number === selfAccount ||
        normalizePhone(mention.number ?? "") === normalizePhone(selfAccount));
    if (isSelf) {
      result =
        result.slice(0, mention.start) +
        result.slice(mention.start + mention.length);
      continue;
    }

    const id = mention.uuid ?? mention.number ?? mention.name ?? "unknown";
    result =
      result.slice(0, mention.start) +
      `@${id}` +
      result.slice(mention.start + mention.length);
  }

  return result.trim();
}

function sleep(ms: number, abortSignal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    abortSignal?.addEventListener("abort", () => {
      clearTimeout(timer);
      resolve();
    }, { once: true });
  });
}
