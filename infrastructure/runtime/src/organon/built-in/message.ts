// Outbound message tool — sends via Signal
import type { ToolHandler } from "../registry.js";

export interface MessageSender {
  send(to: string, text: string): Promise<void>;
}

export interface MessageToolOpts {
  sender?: MessageSender;
  allowedRecipients?: string[];
  maxLength?: number;
}

const MAX_MESSAGE_LENGTH = 4000;

export function createMessageTool(opts: MessageToolOpts = {}): ToolHandler {
  const { sender, allowedRecipients, maxLength = MAX_MESSAGE_LENGTH } = opts;

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
        type: "object",
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

      if (!sender) {
        return JSON.stringify({ error: "Signal not connected" });
      }

      if (allowedRecipients && allowedRecipients.length > 0) {
        if (!allowedRecipients.some((r) => to === r || to.includes(r))) {
          return JSON.stringify({ error: "Recipient not in allowlist" });
        }
      }

      if (text.length > maxLength) {
        text = text.slice(0, maxLength);
      }

      await sender.send(to, text);
      return JSON.stringify({ sent: true, to, length: text.length });
    },
  };
}

export const messageTool = createMessageTool();
