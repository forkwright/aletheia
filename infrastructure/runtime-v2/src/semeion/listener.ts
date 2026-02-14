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
}

export async function startListener(opts: ListenerOpts): Promise<void> {
  const { accountId, account, manager, client, baseUrl, abortSignal } = opts;
  const accountPhone = account.account ?? accountId;

  log.info(`Starting SSE listener for account ${accountId}`);

  const backoff = { min: 1000, max: 10000, current: 1000 };

  while (!abortSignal?.aborted) {
    try {
      await consumeEventStream(
        baseUrl,
        accountPhone,
        (envelope) => handleEnvelope(envelope, accountId, account, manager, client),
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
  onEnvelope: (envelope: SignalEnvelope) => Promise<void>,
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

      for (const line of lines) {
        if (line === "") {
          if (currentEvent.data) {
            try {
              const payload = JSON.parse(currentEvent.data);
              const envelope = payload?.envelope as SignalEnvelope | undefined;
              if (envelope) {
                await onEnvelope(envelope);
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
          const data = line.slice(5);
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

async function handleEnvelope(
  envelope: SignalEnvelope,
  accountId: string,
  account: SignalAccount,
  manager: NousManager,
  client: SignalClient,
): Promise<void> {
  if (envelope.syncMessage) return;

  const dataMessage =
    envelope.editMessage?.dataMessage ?? envelope.dataMessage;
  if (!dataMessage?.message) return;

  const sender = envelope.sourceNumber ?? envelope.sourceUuid;
  if (!sender) return;

  const accountPhone = account.account ?? accountId;
  if (sender === accountPhone) return;

  const isGroup = !!dataMessage.groupInfo?.groupId;
  const groupId = dataMessage.groupInfo?.groupId;

  if (!checkAccess(sender, isGroup, account)) {
    log.debug(`Blocked message from ${sender} (policy)`);
    return;
  }

  let text = dataMessage.message;

  if (dataMessage.mentions?.length) {
    text = hydrateMentions(text, dataMessage.mentions);
  }

  log.info(
    `Inbound ${isGroup ? "group" : "DM"} from ${envelope.sourceName ?? sender}: ${text.slice(0, 80)}`,
  );

  const target: SendTarget = {
    account: accountPhone,
    recipient: isGroup ? undefined : sender,
    groupId: isGroup ? groupId : undefined,
  };

  if (account.sendReadReceipts && !isGroup && envelope.timestamp) {
    sendReadReceipt(client, target, envelope.timestamp).catch((err) =>
      log.warn(`Read receipt failed: ${err}`),
    );
  }

  sendTyping(client, target).catch((err) =>
    log.warn(`Typing indicator failed: ${err}`),
  );

  const peerId = isGroup ? groupId! : sender;
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

    await sendMessage(
      client,
      target,
      "I encountered an error processing that message. Please try again.",
      { markdown: false },
    );
  }
}

function checkAccess(
  sender: string,
  isGroup: boolean,
  account: SignalAccount,
): boolean {
  if (isGroup) {
    if (account.groupPolicy === "disabled") return false;
    if (account.groupPolicy === "open") return true;
    return isInAllowlist(sender, account.groupAllowFrom);
  }

  if (account.dmPolicy === "disabled") return false;
  if (account.dmPolicy === "open") return true;
  return isInAllowlist(sender, account.allowFrom);
}

function isInAllowlist(
  sender: string,
  allowlist: Array<string | number>,
): boolean {
  if (!allowlist.length) return false;

  for (const entry of allowlist) {
    const normalized = String(entry);
    if (normalized === "*") return true;
    if (sender === normalized) return true;
    if (sender.replace(/\D/g, "") === normalized.replace(/\D/g, "")) return true;
  }

  return false;
}

function hydrateMentions(
  text: string,
  mentions: Array<{ name?: string; number?: string; uuid?: string; start?: number; length?: number }>,
): string {
  let result = text;

  const sorted = [...mentions].sort((a, b) => (b.start ?? 0) - (a.start ?? 0));

  for (const mention of sorted) {
    if (mention.start == null || mention.length == null) continue;
    const id = mention.uuid ?? mention.number ?? mention.name ?? "unknown";
    result =
      result.slice(0, mention.start) +
      `@${id}` +
      result.slice(mention.start + mention.length);
  }

  return result;
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
