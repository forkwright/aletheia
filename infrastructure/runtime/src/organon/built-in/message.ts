// Outbound message tool — sends via Signal
import type { ToolHandler } from "../registry.js";

export interface MessageSender {
  send(to: string, text: string): Promise<void>;
}

export function createMessageTool(sender?: MessageSender): ToolHandler {
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
      const text = input.text as string;

      if (!sender) {
        return JSON.stringify({ error: "Signal not connected" });
      }

      await sender.send(to, text);
      return JSON.stringify({ sent: true, to, length: text.length });
    },
  };
}

export const messageTool = createMessageTool();
