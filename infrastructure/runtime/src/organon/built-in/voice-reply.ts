// Voice reply tool — synthesize speech and send as audio attachment via Signal
import { createLogger } from "../../koina/logger.js";
import { synthesize, type TtsOptions } from "../../semeion/tts.js";
import type { ToolHandler, ToolContext } from "../registry.js";

const log = createLogger("organon.voice-reply");

export interface VoiceReplySender {
  send(to: string, text: string, attachments: string[]): Promise<void>;
}

export function createVoiceReplyTool(sender?: VoiceReplySender): ToolHandler {
  return {
    definition: {
      name: "voice_reply",
      description:
        "Convert text to speech and send as a voice message via Signal. " +
        "Use for accessibility, emphasis, or when audio is more appropriate than text.",
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
            description: "Text to speak (max 4096 characters)",
          },
          voice: {
            type: "string",
            description: "Voice: alloy, echo, fable, onyx, nova, shimmer (OpenAI) — default: alloy",
          },
          speed: {
            type: "number",
            description: "Speech speed: 0.25 to 4.0 — default: 1.0",
          },
          caption: {
            type: "string",
            description: "Optional text caption sent alongside the audio",
          },
        },
        required: ["to", "text"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const to = input["to"] as string;
      const text = input["text"] as string;
      const caption = (input["caption"] as string) ?? "";

      if (!sender) {
        return JSON.stringify({ error: "Signal not connected — cannot send voice" });
      }

      const ttsOpts: TtsOptions = {};
      if (input["voice"]) ttsOpts.voice = input["voice"] as string;
      if (input["speed"]) ttsOpts.speed = input["speed"] as number;

      let result;
      try {
        result = await synthesize(text, ttsOpts);
      } catch (err) {
        return JSON.stringify({
          error: `TTS synthesis failed: ${err instanceof Error ? err.message : err}`,
        });
      }

      try {
        const msg = caption || `[voice message from ${context.nousId}]`;
        await sender.send(to, msg, [result.path]);
        log.info(`Voice reply sent to ${to} via ${result.engine} (${context.nousId})`);
        return JSON.stringify({
          sent: true,
          to,
          engine: result.engine,
          textLength: text.length,
        });
      } catch (err) {
        return JSON.stringify({
          error: `Send failed: ${err instanceof Error ? err.message : err}`,
        });
      } finally {
        result.cleanup();
      }
    },
  };
}
