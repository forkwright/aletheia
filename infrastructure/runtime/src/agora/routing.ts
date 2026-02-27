// Agora routing — target format parsing and channel resolution (Spec 34, Phase 4)
//
// Targets arrive as strings from the message tool. This module parses them
// into a channel ID + channel-specific address, resolving ambiguity.
//
// Format rules:
//   "slack:C0123456789"     → { channel: "slack", to: "C0123456789" }
//   "slack:@username"       → { channel: "slack", to: "@username" }
//   "slack:U0123456789"     → { channel: "slack", to: "U0123456789" }
//   "signal:+1234567890"    → { channel: "signal", to: "+1234567890" }
//   "+1234567890"           → { channel: "signal", to: "+1234567890" }  (legacy)
//   "group:ABCDEF"          → { channel: "signal", to: "group:ABCDEF" } (legacy)
//   "u:handle"              → { channel: "signal", to: "u:handle" }     (legacy)
//
// The general pattern: "channel:address" with Signal as the default for
// unqualified targets that match known Signal formats.

import type { AgoraRegistry } from "./registry.js";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ResolvedTarget {
  /** Which channel to send through */
  channel: string;
  /** The channel-specific address (channel ID, phone, group ID, etc.) */
  to: string;
}

export interface RoutingError {
  error: string;
}

export type RoutingResult = ResolvedTarget | RoutingError;

export function isRoutingError(result: RoutingResult): result is RoutingError {
  return "error" in result;
}

// ---------------------------------------------------------------------------
// Known channel prefixes
// ---------------------------------------------------------------------------

const CHANNEL_PREFIXES = ["slack", "signal", "discord", "matrix"] as const;
type ChannelPrefix = (typeof CHANNEL_PREFIXES)[number];

function isChannelPrefix(s: string): s is ChannelPrefix {
  return (CHANNEL_PREFIXES as readonly string[]).includes(s);
}

// ---------------------------------------------------------------------------
// Legacy Signal patterns (unqualified)
// ---------------------------------------------------------------------------

/** Phone number: starts with + followed by digits */
const PHONE_RE = /^\+\d{7,15}$/;

/** Signal group: group:BASE64ID */
const SIGNAL_GROUP_RE = /^group:.+$/;

/** Signal username: u:handle */
const SIGNAL_USERNAME_RE = /^u:.+$/;

function isLegacySignalTarget(target: string): boolean {
  return PHONE_RE.test(target) || SIGNAL_GROUP_RE.test(target) || SIGNAL_USERNAME_RE.test(target);
}

// ---------------------------------------------------------------------------
// Parse
// ---------------------------------------------------------------------------

/**
 * Parse a target string into a channel + address.
 *
 * Does NOT verify the channel is registered — that's the caller's job.
 * This is pure parsing.
 */
export function parseTarget(target: string): RoutingResult {
  const trimmed = target.trim();

  if (!trimmed) {
    return { error: "Empty target" };
  }

  // Check for explicit channel prefix: "channel:address"
  const colonIdx = trimmed.indexOf(":");
  if (colonIdx > 0) {
    const prefix = trimmed.slice(0, colonIdx).toLowerCase();

    // Only treat as channel prefix if it's a known channel
    // This avoids misinterpreting "group:XYZ" or "u:handle" as channel prefixes
    if (isChannelPrefix(prefix)) {
      const address = trimmed.slice(colonIdx + 1);
      if (!address) {
        return { error: `Missing address after "${prefix}:"` };
      }
      return { channel: prefix, to: address };
    }
  }

  // Legacy Signal formats (unqualified)
  if (isLegacySignalTarget(trimmed)) {
    return { channel: "signal", to: trimmed };
  }

  return { error: `Unknown target format: "${trimmed}". Use "channel:address" (e.g., slack:C0123, +1234567890)` };
}

// ---------------------------------------------------------------------------
// Resolve — parse + validate channel exists
// ---------------------------------------------------------------------------

/**
 * Resolve a target string to a channel + address, validating the channel is registered.
 */
export function resolveTarget(target: string, registry: AgoraRegistry): RoutingResult {
  const parsed = parseTarget(target);

  if (isRoutingError(parsed)) {
    return parsed;
  }

  if (!registry.has(parsed.channel)) {
    // Give a helpful error — list what IS available
    const available = registry.list();
    if (available.length === 0) {
      return { error: `No channels configured. Cannot send to "${parsed.channel}".` };
    }
    return {
      error: `Channel "${parsed.channel}" is not configured. Available: ${available.join(", ")}`,
    };
  }

  return parsed;
}
