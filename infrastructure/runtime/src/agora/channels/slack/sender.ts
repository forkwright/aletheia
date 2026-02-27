// Slack outbound sender (Spec 34, Phase 3)
//
// Handles: text delivery, message chunking, thread replies, identity override,
// file uploads. Reference: OpenClaw src/slack/send.ts

import type { WebClient, ChatPostMessageArguments } from "@slack/web-api";
import { createLogger } from "../../../koina/logger.js";
import type { ChannelIdentity, ChannelSendParams, ChannelSendResult } from "../../types.js";
import { markdownToMrkdwn, chunkMrkdwn } from "./format.js";

const log = createLogger("agora:slack:sender");

// ---------------------------------------------------------------------------
// Identity override
// ---------------------------------------------------------------------------

interface ResolvedIdentity {
  username?: string;
  iconEmoji?: string;
  iconUrl?: string;
}

function resolveSlackIdentity(identity: ChannelIdentity | undefined): ResolvedIdentity | null {
  if (!identity) return null;
  const username = identity.name?.trim() || undefined;
  const iconUrl = identity.avatarUrl?.trim() || undefined;
  const rawEmoji = identity.emoji?.trim();
  const iconEmoji =
    !iconUrl && rawEmoji
      ? /^:[^:\s]+:$/.test(rawEmoji)
        ? rawEmoji
        : `:${rawEmoji.replace(/^:|:$/g, "")}:`
      : undefined;
  if (!username && !iconUrl && !iconEmoji) return null;
  const result: ResolvedIdentity = {};
  if (username) result.username = username;
  if (iconUrl) result.iconUrl = iconUrl;
  if (iconEmoji) result.iconEmoji = iconEmoji;
  return result;
}

function isMissingScopeError(err: unknown): boolean {
  if (!(err instanceof Error)) return false;
  const data = (err as Error & { data?: { error?: string; needed?: string } }).data;
  if (data?.error?.toLowerCase() !== "missing_scope") return false;
  return data?.needed?.toLowerCase()?.includes("chat:write.customize") ?? false;
}

// ---------------------------------------------------------------------------
// Build args with conditional optional properties
// ---------------------------------------------------------------------------

function buildPostArgs(
  channel: string,
  text: string,
  threadTs: string | null,
  identity: ResolvedIdentity | null,
): ChatPostMessageArguments {
  const args: ChatPostMessageArguments = { channel, text };
  if (threadTs) args.thread_ts = threadTs;
  if (identity?.username) args.username = identity.username;
  if (identity?.iconUrl) args.icon_url = identity.iconUrl;
  if (identity?.iconEmoji) args.icon_emoji = identity.iconEmoji;
  return args;
}

// ---------------------------------------------------------------------------
// Core send
// ---------------------------------------------------------------------------

async function postMessageBestEffort(
  webClient: WebClient,
  channel: string,
  text: string,
  threadTs: string | null,
  identity: ResolvedIdentity | null,
): Promise<string> {
  const hasCustom = identity !== null;
  const args = buildPostArgs(channel, text, threadTs, identity);

  try {
    const response = await webClient.chat.postMessage(args);
    return response.ts ?? "unknown";
  } catch (err) {
    if (!hasCustom || !isMissingScopeError(err)) throw err;
    log.warn("Missing chat:write.customize scope — sending without custom identity");
    const fallback = buildPostArgs(channel, text, threadTs, null);
    const response = await webClient.chat.postMessage(fallback);
    return response.ts ?? "unknown";
  }
}

// ---------------------------------------------------------------------------
// File upload
// ---------------------------------------------------------------------------

async function uploadFile(
  webClient: WebClient,
  channel: string,
  filePath: string,
  threadTs: string | null,
): Promise<string> {
  const { createReadStream } = await import("node:fs");
  const filename = filePath.split("/").pop() ?? "file";

  // Build args without `string | undefined` values on optional fields
  const args: { channel_id: string; file: NodeJS.ReadableStream; filename: string; thread_ts?: string } = {
    channel_id: channel,
    file: createReadStream(filePath) as NodeJS.ReadableStream,
    filename,
  };
  if (threadTs) args.thread_ts = threadTs;

  const response = await webClient.filesUploadV2(args as Parameters<WebClient["filesUploadV2"]>[0]);
  const files = (response as unknown as { files?: Array<{ id?: string }> }).files;
  return files?.[0]?.id ?? "uploaded";
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export interface SlackSenderContext {
  webClient: WebClient;
}

/**
 * Send a message through Slack.
 */
export async function sendSlackMessage(
  ctx: SlackSenderContext,
  params: ChannelSendParams,
): Promise<ChannelSendResult> {
  const { webClient } = ctx;
  const { to, text, threadId, attachments, identity, markdown } = params;

  if (!to) return { sent: false, error: "No target specified" };

  try {
    const resolvedIdentity = resolveSlackIdentity(identity);
    const threadTs = threadId ?? null;

    // Convert markdown → mrkdwn (unless explicitly disabled)
    const formatted = markdown === false ? text : markdownToMrkdwn(text);
    const chunks = chunkMrkdwn(formatted);

    for (const chunk of chunks.length > 0 ? chunks : [""]) {
      await postMessageBestEffort(webClient, to, chunk, threadTs, resolvedIdentity);
    }

    if (attachments?.length) {
      for (const filePath of attachments) {
        try {
          await uploadFile(webClient, to, filePath, threadTs);
        } catch (err) {
          log.warn(`Failed to upload attachment ${filePath}: ${err instanceof Error ? err.message : err}`);
        }
      }
    }

    return { sent: true };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    log.error(`Slack send failed: ${message}`);
    return { sent: false, error: message };
  }
}
