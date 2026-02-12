// Outbound message sending via signal-cli
import { createLogger } from "../koina/logger.js";
import { SignalClient } from "./client.js";
import { formatForSignal, stylesToSignalParam } from "./format.js";

const log = createLogger("semeion:send");

export interface SendTarget {
  recipient?: string;
  groupId?: string;
  username?: string;
  account: string;
}

export interface SendOpts {
  markdown?: boolean;
  attachments?: string[];
}

export async function sendMessage(
  client: SignalClient,
  target: SendTarget,
  text: string,
  opts?: SendOpts,
): Promise<void> {
  const useMarkdown = opts?.markdown !== false;

  const chunks = splitMessage(text, 2000);

  for (let i = 0; i < chunks.length; i++) {
    const chunk = chunks[i];
    if (chunk == null) continue;
    const isLast = i === chunks.length - 1;

    let message = chunk;
    let textStyle: string[] | undefined;

    if (useMarkdown) {
      const formatted = formatForSignal(message);
      message = formatted.text;
      if (formatted.styles.length > 0) {
        textStyle = stylesToSignalParam(formatted.styles);
      }
    }

    const sendParams: Parameters<SignalClient["send"]>[0] = {
      message,
      account: target.account,
    };
    if (target.recipient) sendParams.recipient = target.recipient;
    if (target.groupId) sendParams.groupId = target.groupId;
    if (target.username) sendParams.username = target.username;
    if (isLast && opts?.attachments) sendParams.attachments = opts.attachments;
    if (textStyle) sendParams.textStyle = textStyle;

    await client.send(sendParams);
  }

  log.debug(
    `Sent ${chunks.length} chunk(s) to ${target.recipient ?? target.groupId ?? target.username}`,
  );
}

export async function sendTyping(
  client: SignalClient,
  target: SendTarget,
  stop = false,
): Promise<void> {
  const typingParams: Parameters<SignalClient["sendTyping"]>[0] = {
    account: target.account,
    stop,
  };
  if (target.recipient) typingParams.recipient = target.recipient;
  if (target.groupId) typingParams.groupId = target.groupId;

  await client.sendTyping(typingParams);
}

export async function sendReadReceipt(
  client: SignalClient,
  target: SendTarget,
  targetTimestamp: number,
): Promise<void> {
  if (!target.recipient) return;
  await client.sendReceipt({
    recipient: target.recipient,
    targetTimestamp,
    account: target.account,
  });
}

export async function sendReaction(
  client: SignalClient,
  target: SendTarget,
  emoji: string,
  targetTimestamp: number,
  targetAuthor: string,
): Promise<void> {
  const reactionParams: Parameters<SignalClient["sendReaction"]>[0] = {
    emoji,
    targetTimestamp,
    targetAuthor,
    account: target.account,
  };
  if (target.recipient) reactionParams.recipient = target.recipient;
  if (target.groupId) reactionParams.groupId = target.groupId;

  await client.sendReaction(reactionParams);
}

function splitMessage(text: string, maxLen: number): string[] {
  if (text.length <= maxLen) return [text];

  const chunks: string[] = [];
  let remaining = text;

  while (remaining.length > maxLen) {
    let splitAt = remaining.lastIndexOf("\n", maxLen);
    if (splitAt === -1 || splitAt < maxLen * 0.3) {
      splitAt = remaining.lastIndexOf(" ", maxLen);
    }
    if (splitAt === -1 || splitAt < maxLen * 0.3) {
      splitAt = maxLen;
    }

    // Avoid splitting in the middle of a surrogate pair
    const code = remaining.charCodeAt(splitAt - 1);
    if (code >= 0xD800 && code <= 0xDBFF) {
      splitAt--;
    }

    chunks.push(remaining.slice(0, splitAt));
    remaining = remaining.slice(splitAt).replace(/^\n/, "");
  }

  if (remaining.length > 0) {
    chunks.push(remaining);
  }

  return chunks;
}

export function parseTarget(to: string, account: string): SendTarget {
  const target: SendTarget = { account };

  if (to.startsWith("group:")) {
    target.groupId = to.slice(6);
  } else if (to.startsWith("u:") || to.startsWith("username:")) {
    target.username = to.replace(/^(u:|username:)/, "");
  } else {
    target.recipient = to.replace(/^signal:/i, "");
  }

  return target;
}
