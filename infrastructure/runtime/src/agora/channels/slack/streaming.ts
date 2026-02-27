// Slack native text streaming (Spec 34, Phase 5)
//
// Uses @slack/web-api ChatStreamer (chat.startStream / appendStream / stopStream)
// to progressively render LLM output in a single updating Slack message.
//
// Reference: OpenClaw src/slack/streaming.ts
// Docs: https://docs.slack.dev/ai/developing-ai-apps#streaming

import type { WebClient } from "@slack/web-api";
import type { ChatStreamer } from "@slack/web-api/dist/chat-stream.js";
import type { ChatStartStreamArguments } from "@slack/web-api/dist/types/request/chat.js";
import { createLogger } from "../../../koina/logger.js";

const log = createLogger("agora:slack:streaming");

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface SlackStreamSession {
  /** The SDK ChatStreamer instance managing this stream. */
  streamer: ChatStreamer;
  /** Channel this stream lives in. */
  channel: string;
  /** Thread timestamp (required for streaming). */
  threadTs: string;
  /** True once stop() has been called. */
  stopped: boolean;
}

export interface StartSlackStreamParams {
  client: WebClient;
  channel: string;
  threadTs: string;
  /** Optional initial text to include in the stream start. */
  text?: string | undefined;
  /** Team ID (from auth.test) — required by Slack API. */
  teamId?: string | undefined;
  /** Recipient user ID — required for DM streaming. */
  userId?: string | undefined;
}

export interface AppendSlackStreamParams {
  session: SlackStreamSession;
  text: string;
}

export interface StopSlackStreamParams {
  session: SlackStreamSession;
  /** Optional final text to append before stopping. */
  text?: string;
}

// ---------------------------------------------------------------------------
// Stream lifecycle
// ---------------------------------------------------------------------------

/**
 * Start a new Slack text stream.
 *
 * Returns a SlackStreamSession that should be passed to appendSlackStream()
 * and stopSlackStream(). The first chunk of text can optionally be included
 * via `text`, which triggers the ChatStreamer to call chat.startStream.
 */
export async function startSlackStream(
  params: StartSlackStreamParams,
): Promise<SlackStreamSession> {
  const { client, channel, threadTs, text, teamId, userId } = params;

  log.debug(
    `Starting stream in ${channel} thread=${threadTs}` +
    `${teamId ? ` team=${teamId}` : ""}` +
    `${userId ? ` user=${userId}` : ""}`,
  );

  // Build ChatStreamer args — use proper Slack types
  const streamArgs: ChatStartStreamArguments = {
    channel,
    thread_ts: threadTs,
  };
  if (teamId) streamArgs.recipient_team_id = teamId;
  if (userId) streamArgs.recipient_user_id = userId;

  const streamer = client.chatStream(streamArgs);

  const session: SlackStreamSession = {
    streamer,
    channel,
    threadTs,
    stopped: false,
  };

  // If initial text is provided, send it as the first append which triggers
  // chat.startStream under the hood
  if (text) {
    await streamer.append({ markdown_text: text });
    log.debug(`Appended initial text (${text.length} chars)`);
  }

  return session;
}

/**
 * Append markdown text to an active Slack stream.
 *
 * Silently ignores appends to stopped streams and empty text.
 */
export async function appendSlackStream(params: AppendSlackStreamParams): Promise<void> {
  const { session, text } = params;

  if (session.stopped) {
    log.debug("Attempted to append to a stopped stream, ignoring");
    return;
  }

  if (!text) return;

  await session.streamer.append({ markdown_text: text });
}

/**
 * Stop (finalize) a Slack stream.
 *
 * After calling this the stream message becomes a normal Slack message.
 * Optionally include final text to append before stopping.
 */
export async function stopSlackStream(params: StopSlackStreamParams): Promise<void> {
  const { session, text } = params;

  if (session.stopped) {
    log.debug("Stream already stopped, ignoring duplicate stop");
    return;
  }

  session.stopped = true;

  log.debug(
    `Stopping stream in ${session.channel} thread=${session.threadTs}` +
    `${text ? ` (final text: ${text.length} chars)` : ""}`,
  );

  await session.streamer.stop(text ? { markdown_text: text } : undefined);
}
