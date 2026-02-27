// Outbound message tool — routes through agora channel abstraction (Spec 34, Phase 4)
//
// The message tool no longer knows about Signal directly. It parses targets,
// resolves the channel via agora routing, and dispatches through the registry.
//
// Target formats:
//   slack:C0123456789     → Slack channel
//   slack:@username       → Slack DM (resolved by Slack provider)
//   slack:U0123456789     → Slack DM (direct user ID)
//   signal:+1234567890    → Signal (explicit)
//   +1234567890           → Signal (legacy, backward compatible)
//   group:ABCDEF          → Signal group (legacy)
//   u:handle              → Signal username (legacy)

import type { ToolHandler } from "../registry.js";
import type { AgoraRegistry } from "../../agora/registry.js";
import type { ChannelIdentity } from "../../agora/types.js";
import { resolveTarget, isRoutingError } from "../../agora/routing.js";

export interface MessageToolOpts {
  /** Agora registry for multi-channel routing */
  registry?: AgoraRegistry;
  /** Legacy sender — used when no registry is provided (backward compat) */
  sender?: LegacySender;
  /** Restrict sending to these recipients only */
  allowedRecipients?: string[];
  /** Max message length (default 4000) */
  maxLength?: number;
  /** Default identity for outbound messages */
  identity?: ChannelIdentity;
}

export interface LegacySender {
  send(to: string, text: string): Promise<void>;
}

const MAX_MESSAGE_LENGTH = 4000;

export function createMessageTool(opts: MessageToolOpts = {}): ToolHandler {
  const {
    registry,
    sender,
    allowedRecipients,
    maxLength = MAX_MESSAGE_LENGTH,
    identity,
  } = opts;

  return {
    definition: {
      name: "message",
      description:
        "Send a message to a user or group via Signal.\n\n" +
        "USE WHEN:\n" +
        "- Proactively notifying users about completed tasks or important events\n" +
        "- Forwarding information to a different recipient than the current conversation\n" +
        "- Sending alerts or scheduled notifications\n\n" +
        "DO NOT USE WHEN:\n" +
        "- Replying to the current conversation — your response IS the reply\n" +
        "- Communicating with other agents — use sessions_send or sessions_ask\n\n" +
        "TIPS:\n" +
        "- Markdown supported in message text\n" +
        "- Groups use 'group:ID' format\n" +
        "- Messages capped at 4000 chars\n" +
        "- Recipient must be in allowlist if configured",
      input_schema: {
        type: "object" as const,
        properties: {
          to: {
            type: "string",
            description:
              "Recipient: phone (+1234567890), group (group:ID), or username (u:handle)",
          },
          text: {
            type: "string",
            description: "Message text to send (markdown supported)",
          },
        },
        required: ["to", "text"],
      },
    },
    async execute(input: Record<string, unknown>): Promise<string> {
      const to = input["to"] as string;
      let text = input["text"] as string;

      // Allowlist check (raw target, before parsing)
      if (allowedRecipients && allowedRecipients.length > 0) {
        if (!allowedRecipients.some((r) => to === r || to.includes(r))) {
          return JSON.stringify({ error: "Recipient not in allowlist" });
        }
      }

      // Truncate
      if (text.length > maxLength) {
        text = text.slice(0, maxLength);
      }

      // Route through agora if registry is available
      if (registry) {
        const resolved = resolveTarget(to, registry);
        if (isRoutingError(resolved)) {
          return JSON.stringify({ error: resolved.error });
        }

        const sendParams: import("../../agora/types.js").ChannelSendParams = {
          to: resolved.to,
          text,
        };
        if (identity) sendParams.identity = identity;

        const result = await registry.send(resolved.channel, sendParams);

        if (!result.sent) {
          return JSON.stringify({ error: result.error ?? "Send failed" });
        }

        return JSON.stringify({
          sent: true,
          to,
          channel: resolved.channel,
          length: text.length,
        });
      }

      // Legacy path — direct sender (backward compat)
      if (sender) {
        await sender.send(to, text);
        return JSON.stringify({ sent: true, to, length: text.length });
      }

      return JSON.stringify({ error: "No channels configured" });
    },
  };
}

export const messageTool = createMessageTool();
