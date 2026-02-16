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
        "Send a message to a user or group via Signal. Use for proactive communication.",
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
      const to = input.to as string;
      let text = input.text as string;

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
