// Slack reaction helpers (Spec 34, Phase 5)
//
// Adds/removes emoji reactions on messages to signal processing state.
// Reference: OpenClaw src/slack/actions.ts

import type { WebClient } from "@slack/web-api";
import { createLogger } from "../../../koina/logger.js";

const log = createLogger("agora:slack:reactions");

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface SlackReactionParams {
  client: WebClient;
  channel: string;
  timestamp: string;
  emoji: string;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Normalize emoji — strip surrounding colons if present */
function normalizeEmoji(raw: string): string {
  return raw.trim().replace(/^:+|:+$/g, "");
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Add a reaction to a Slack message.
 * Silently succeeds if the reaction already exists.
 */
export async function addSlackReaction(params: SlackReactionParams): Promise<boolean> {
  const { client, channel, timestamp, emoji } = params;
  try {
    await client.reactions.add({
      channel,
      timestamp,
      name: normalizeEmoji(emoji),
    });
    return true;
  } catch (err) {
    // "already_reacted" is not an error — we're idempotent
    if (isAlreadyReacted(err)) return true;
    log.warn(`Failed to add reaction :${emoji}: — ${err instanceof Error ? err.message : err}`);
    return false;
  }
}

/**
 * Remove a reaction from a Slack message.
 * Silently succeeds if the reaction doesn't exist.
 */
export async function removeSlackReaction(params: SlackReactionParams): Promise<boolean> {
  const { client, channel, timestamp, emoji } = params;
  try {
    await client.reactions.remove({
      channel,
      timestamp,
      name: normalizeEmoji(emoji),
    });
    return true;
  } catch (err) {
    // "no_reaction" is not an error — we're idempotent
    if (isNoReaction(err)) return true;
    log.warn(`Failed to remove reaction :${emoji}: — ${err instanceof Error ? err.message : err}`);
    return false;
  }
}

// ---------------------------------------------------------------------------
// Error detection
// ---------------------------------------------------------------------------

function isAlreadyReacted(err: unknown): boolean {
  return isSlackError(err, "already_reacted");
}

function isNoReaction(err: unknown): boolean {
  return isSlackError(err, "no_reaction");
}

function isSlackError(err: unknown, code: string): boolean {
  if (!(err instanceof Error)) return false;
  const data = (err as Error & { data?: { error?: string } }).data;
  return data?.error === code;
}
