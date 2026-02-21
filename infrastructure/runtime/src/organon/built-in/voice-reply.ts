// Voice reply tool — synthesize speech and send as audio attachment via Signal
import { createLogger } from "../../koina/logger.js";
import { synthesize, type TtsOptions } from "../../semeion/tts.js";
import type { ToolContext, ToolHandler } from "../registry.js";

const log = createLogger("organon.voice-reply");

export interface VoiceReplySender {
  send(to: string, text: string, attachments: string[]): Promise<void>;
}

export function createVoiceReplyTool(sender?: VoiceReplySender): ToolHandler {
  return {
    definition: {
      name: "voice_reply",
      description:
        "Convert text to speech and send as a voice message via Signal.\n\n" +
        "USE WHEN:\n" +
        "- User prefers audio responses\n" +
        "- Content benefits from spoken delivery (stories, explanations)\n" +
        "- Accessibility needs require audio output\n\n" +
        "DO NOT USE WHEN:\n" +
        "- Text response is sufficient — voice uses more bandwidth\n" +
        "- Content has code, URLs, or structured data that doesn't work as audio\n\n" +
        "TIPS:\n" +
        "- Max 4096 chars of text\n" +
        "- Voices: alloy, echo, fable, onyx, nova, shimmer\n" +
        "- Speed adjustable 0.25x to 4.0x\n" +
        "- Add a caption for context alongside the audio",
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
