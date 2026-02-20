// SSE event stream consumer — dispatches Signal messages to NousManager
import { createLogger, withTurnAsync } from "../koina/logger.js";
import { TransportError } from "../koina/errors.js";
import type { InboundMessage, MediaAttachment, NousManager } from "../nous/manager.js";
import type { SignalClient } from "./client.js";
import { sendMessage, sendReadReceipt, type SendTarget, sendTyping } from "./sender.js";
import type { AletheiaConfig, SignalAccount } from "../taxis/schema.js";
import type { SessionStore } from "../mneme/store.js";
import type { CommandContext, CommandRegistry } from "./commands.js";
import type { Watchdog } from "../daemon/watchdog.js";
import type { SkillRegistry } from "../organon/skills.js";
import { preprocessLinks } from "./preprocess.js";
import { transcribeAudio } from "./transcribe.js";

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
  commands?: CommandRegistry;
  store?: SessionStore;
  config?: AletheiaConfig;
  watchdog?: Watchdog | null;
  skills?: SkillRegistry | null;
}

const MAX_CONCURRENT_TURNS = 6;
const activeTurns = new Map<string, number>();

export async function startListener(opts: ListenerOpts): Promise<void> {
  const { accountId, account, manager, client, baseUrl, abortSignal, boundGroupIds, onStatusRequest, commands, store, config, watchdog, skills } = opts;
  const accountPhone = account.account ?? accountId;

  log.info(`Starting SSE listener for account ${accountId}`);

  const backoff = { min: 1000, max: 10000, current: 1000 };

  while (!abortSignal?.aborted) {
    try {
      await consumeEventStream(
        baseUrl,
        accountPhone,
        (envelope) => handleEnvelope(envelope, accountId, account, manager, client, boundGroupIds, onStatusRequest, commands, store, config, watchdog, skills),
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

  const fetchOpts: RequestInit = {
    headers: { Accept: "text/event-stream" },
  };
  if (abortSignal) fetchOpts.signal = abortSignal;

  const res = await fetch(url, fetchOpts);

  if (!res.ok) {
    throw new TransportError(`SSE connect failed: ${res.status} ${res.statusText}`, {
      code: "SIGNAL_SSE_FAILED", recoverable: true, retryAfterMs: 5_000,
      context: { status: res.status, url },
    });
  }

  if (!res.body) {
    throw new TransportError("SSE response has no body", { code: "SIGNAL_SSE_FAILED" });
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

    // Flush any remaining event that wasn't terminated by a blank line
    if (currentEvent.data && (!currentEvent.event || currentEvent.event === "receive")) {
      try {
        const payload = JSON.parse(currentEvent.data);
        const envelope = payload?.envelope as SignalEnvelope | undefined;
        if (envelope) {
          onEnvelope(envelope);
        }
      } catch (parseErr) {
        log.warn(
          `Failed to parse final SSE data: ${parseErr instanceof Error ? parseErr.message : parseErr}`,
        );
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
  commands?: CommandRegistry,
  store?: SessionStore,
  config?: AletheiaConfig,
  watchdog?: Watchdog | null,
  skills?: SkillRegistry | null,
): void {
  if (envelope.syncMessage) return;
  if (envelope.editMessage) return;

  const dataMessage = envelope.dataMessage;
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

  const pairingTarget: SendTarget = { account: accountPhone };
  if (!isGroup) pairingTarget.recipient = sender;
  if (isGroup && groupId) pairingTarget.groupId = groupId;

  const pairingCtx: PairingContext = {
    client,
    accountId,
    accountPhone,
    senderName: envelope.sourceName ?? sender,
    target: pairingTarget,
  };
  if (store) pairingCtx.store = store;

  const access = checkAccess(sender, isGroup, groupId, account, boundGroupIds, pairingCtx);
  if (!access) {
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
      const name = sanitizeAttachmentField(att.filename ?? "unnamed");
      const type = sanitizeAttachmentField(att.contentType ?? "unknown");
      const size = att.size ? `${Math.round(att.size / 1024)}KB` : "unknown size";
      text += `\n[Attachment: ${name} (${type}, ${size})${att.id ? ` id=${att.id}` : ""}]`;
    }
  }

  log.info(
    `Inbound ${isGroup ? "group" : "DM"} from ${envelope.sourceName ?? sender}: ${text.slice(0, 80)}`,
  );

  const target: SendTarget = { account: accountPhone };
  if (!isGroup) target.recipient = sender;
  if (isGroup && groupId) target.groupId = groupId;

  // Command detection — bypass agent turn pipeline
  if (commands && store && config) {
    const match = commands.match(text);
    if (match) {
      // Enforce adminOnly commands — only allowFrom[0] (owner) can run them
      if (match.handler.adminOnly) {
        const allowFrom = account.allowFrom?.map(String) ?? [];
        const senderUuid = envelope.sourceUuid ?? sender;
        if (allowFrom.length > 0 && !allowFrom.includes(senderUuid) && !allowFrom.includes(sender)) {
          log.warn(`Admin command !${match.handler.name} blocked for ${envelope.sourceName ?? sender}`);
          void sendMessage(client, target, "That command requires admin access.", { markdown: false });
          return;
        }
      }
      log.info(`Command !${match.handler.name} from ${envelope.sourceName ?? sender}`);
      const cmdCtx: CommandContext = {
        sender: envelope.sourceUuid ?? sender,
        senderName: envelope.sourceName ?? sender,
        isGroup,
        accountId,
        target,
        client,
        store,
        config,
        manager,
        watchdog: watchdog ?? null,
        skills: skills ?? null,
      };
      match.handler
        .execute(match.args, cmdCtx)
        .then((result) => sendMessage(client, target, result, { markdown: false }))
        .catch((err) => log.warn(`Command !${match.handler.name} failed: ${err instanceof Error ? err.message : err}`));
      return;
    }
  }

  // Legacy status command fallback (if no command registry)
  if (!commands && onStatusRequest && !isGroup && text.trim().toLowerCase() === "status") {
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

  // Thread resolution: map Signal sender to canonical identity, then resolve thread + binding
  let threadId: string | undefined;
  let bindingId: string | undefined;
  let lockKey: string | undefined;
  try {
    const store = manager.sessionStore;
    // Groups share a thread per group (not per member). DMs share a thread per contact identity.
    const identity = isGroup
      ? `group:${groupId ?? peerId}`
      : store.getIdentityForSignalSender(sender, accountId);
    const nousIdForThread = isGroup ? (groupId ?? "main") : peerId;
    const thread = store.resolveThread(nousIdForThread, identity);
    const binding = store.resolveBinding(thread.id, "signal", sessionKey);
    threadId = thread.id;
    bindingId = binding.id;
    lockKey = `binding:${binding.id}`;
  } catch (err) {
    log.warn(`Thread resolution failed for signal:${peerId}: ${err instanceof Error ? err.message : err}`);
  }

  const msg: InboundMessage = {
    text,
    channel: "signal",
    peerId,
    peerKind,
    accountId,
    sessionKey,
    ...(threadId ? { threadId } : {}),
    ...(bindingId ? { bindingId } : {}),
    ...(lockKey ? { lockKey } : {}),
  };

  // Concurrency guard — reject if at limit to protect API and SQLite
  const accountTurns = activeTurns.get(accountId) ?? 0;
  if (accountTurns >= MAX_CONCURRENT_TURNS) {
    log.warn(`Concurrency limit reached for ${accountId} (${accountTurns}/${MAX_CONCURRENT_TURNS}), dropping message`);
    sendMessage(client, target, "I'm handling several conversations right now. Give me a moment and try again.", { markdown: false })
      .catch((err) => log.warn(`Failed to send busy message: ${err}`));
    return;
  }

  // Fire-and-forget — the session mutex in manager.ts serializes same-session turns,
  // while different nous/sessions process concurrently
  activeTurns.set(accountId, accountTurns + 1);
  withTurnAsync(
    { channel: "signal", sessionKey, sender: envelope.sourceName ?? sender },
    () => preprocessAndProcess(manager, msg, client, target, dataMessage.attachments, accountPhone, account.mediaMaxMb),
  ).finally(() => {
    const current = activeTurns.get(accountId) ?? 1;
    if (current <= 1) {
      activeTurns.delete(accountId);
    } else {
      activeTurns.set(accountId, current - 1);
    }
  });
}

async function preprocessAndProcess(
  manager: NousManager,
  msg: InboundMessage,
  client: SignalClient,
  target: SendTarget,
  attachments?: Array<{ id?: string; contentType?: string; filename?: string; size?: number }>,
  accountPhone?: string,
  mediaMaxMb?: number,
): Promise<void> {
  // Fetch attachments — images for vision, audio for transcription
  if (attachments?.length) {
    log.info(`Processing ${attachments.length} attachment(s)`);
    const media: MediaAttachment[] = [];
    const maxBytes = (mediaMaxMb ?? 25) * 1024 * 1024;

    for (const att of attachments) {
      if (!att.id) {
        log.info(`Attachment missing id field (keys: ${Object.keys(att).join(", ")})`);
        continue;
      }
      const ct = att.contentType ?? "";
      if (att.size && att.size > maxBytes) {
        log.info(`Skipping oversized attachment ${att.filename ?? att.id} (${att.size} bytes)`);
        continue;
      }

      try {
        const attParams: { id: string; account?: string } = { id: att.id };
        if (accountPhone) attParams.account = accountPhone;

        if (ct.startsWith("image/") || ct === "application/pdf" || ct.startsWith("text/") || ct === "application/json" || ct === "application/xml") {
          // Fetch as media for vision/document blocks
          log.info(`Fetching attachment ${att.id} (${ct})`);
          const result = await client.getAttachment(attParams);
          log.info(`getAttachment result type: ${typeof result}, length: ${typeof result === "string" ? result.length : "N/A"}`);
          if (typeof result === "string") {
            const attachment: MediaAttachment = {
              contentType: ct,
              data: result,
            };
            if (att.filename) attachment.filename = att.filename;
            media.push(attachment);
            log.info(`Fetched attachment: ${att.filename ?? att.id} (${ct}, ${Math.round(result.length / 1024)}KB base64)`);
          } else {
            log.warn(`getAttachment returned non-string for ${att.id}: ${JSON.stringify(result).slice(0, 200)}`);
          }
        } else if (ct.startsWith("audio/")) {
          const result = await client.getAttachment(attParams);
          if (typeof result === "string") {
            const transcript = await transcribeAudio(result, ct);
            if (transcript) {
              msg.text = `[Voice message transcription]: ${transcript}\n\n${msg.text}`;
              log.info(`Transcribed voice message: ${transcript.length} chars`);
            } else {
              msg.text = `[Voice message — transcription unavailable]\n\n${msg.text}`;
            }
          }
        } else {
          log.info(`Unsupported attachment type: ${ct} (${att.filename ?? att.id})`);
        }
      } catch (err) {
        log.warn(`Failed to fetch attachment ${att.id}: ${err instanceof Error ? err.message : err}`);
      }
    }

    if (media.length > 0) {
      log.info(`Passing ${media.length} media attachment(s) to manager`);
      msg.media = media;
    }
  }

  // Link pre-processing: fetch URLs and append previews
  try {
    msg.text = await preprocessLinks(msg.text);
  } catch (err) {
    log.debug(`Link preprocessing failed (non-fatal): ${err instanceof Error ? err.message : err}`);
  }
  return processTurn(manager, msg, client, target);
}

async function processTurn(
  manager: NousManager,
  msg: InboundMessage,
  client: SignalClient,
  target: SendTarget,
): Promise<void> {
  try {
    const outcome = await manager.handleMessage(msg);

    if (outcome.error) {
      log.error(`Turn completed with error: ${outcome.error}`, { nousId: outcome.nousId, sessionId: outcome.sessionId });
      await sendMessage(client, target, "I encountered an error processing that. Please try again.", { markdown: false });
      return;
    }

    sendTyping(client, target, true).catch(() => { /* typing indicator, non-critical */ });

    if (outcome.text) {
      await sendMessage(client, target, outcome.text);
    }

    log.info(
      `Turn complete: ${outcome.nousId} session=${outcome.sessionId} tools=${outcome.toolCalls} in=${outcome.inputTokens} out=${outcome.outputTokens}`,
    );
  } catch (err) {
    sendTyping(client, target, true).catch(() => { /* typing indicator, non-critical */ });
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

interface PairingContext {
  store?: SessionStore;
  client?: SignalClient;
  accountId?: string;
  accountPhone?: string;
  senderName?: string;
  target?: SendTarget;
}

function checkAccess(
  sender: string,
  isGroup: boolean,
  groupId: string | undefined,
  account: SignalAccount,
  boundGroupIds?: Set<string>,
  pairingCtx?: PairingContext,
): boolean {
  if (isGroup) {
    if (account.groupPolicy === "disabled") return false;
    if (account.groupPolicy === "open") return true;
    if (groupId && isInAllowlist(groupId, account.groupAllowFrom)) return true;
    if (groupId && boundGroupIds?.has(groupId)) return true;
    return false;
  }

  if (account.dmPolicy === "disabled") return false;
  if (account.dmPolicy === "open") return true;

  if (account.dmPolicy === "pairing") {
    // Check static allowlist first
    if (isInAllowlist(sender, account.allowFrom)) return true;
    // Check dynamic approved contacts
    if (pairingCtx?.store?.isApprovedContact(sender, "signal", pairingCtx.accountId)) return true;

    // Initiate pairing flow
    if (pairingCtx?.store && pairingCtx?.client && pairingCtx?.target) {
      const { id, challengeCode } = pairingCtx.store.createContactRequest(
        sender,
        pairingCtx.senderName ?? sender,
        "signal",
        pairingCtx.accountId,
      );
      log.info(`Pairing request #${id} from ${pairingCtx.senderName ?? sender} (code: ${challengeCode})`);
      sendMessage(pairingCtx.client, pairingCtx.target, `I don't know you yet. Ask an admin to approve code: ${challengeCode}`, { markdown: false })
        .catch((err) => log.warn(`Failed to send pairing message: ${err}`));
    }
    return false;
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
    if (mention.start === null || mention.length === null) continue;

    // Strip self-mentions (the bot's own mention placeholder)
    const isSelf =
      selfAccount &&
      (mention.number === selfAccount ||
        normalizePhone(mention.number ?? "") === normalizePhone(selfAccount));
    if (isSelf) {
      result =
        result.slice(0, mention.start!) +
        result.slice(mention.start! + mention.length!);
      continue;
    }

    const id = mention.uuid ?? mention.number ?? mention.name ?? "unknown";
    result =
      result.slice(0, mention.start!) +
      `@${id}` +
      result.slice(mention.start! + mention.length!);
  }

  return result.trim();
}

function sanitizeAttachmentField(value: string): string {
  return value.replaceAll("\n", "").replaceAll("\r", "").replaceAll(String.fromCharCode(0), "").replaceAll("[", "").replaceAll("]", "");
}

function sleep(ms: number, abortSignal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const onAbort = () => {
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(() => {
      abortSignal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    abortSignal?.addEventListener("abort", onAbort, { once: true });
  });
}
