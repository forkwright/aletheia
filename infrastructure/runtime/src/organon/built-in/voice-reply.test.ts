// Voice reply tool tests
import { describe, it, expect, vi } from "vitest";
import { createVoiceReplyTool } from "./voice-reply.js";

// Mock TTS to avoid actual synthesis
vi.mock("../../semeion/tts.js", () => ({
  synthesize: vi.fn().mockResolvedValue({
    path: "/tmp/test.mp3",
    engine: "openai" as const,
    cleanup: vi.fn(),
  }),
}));

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("createVoiceReplyTool", () => {
  it("has valid definition", () => {
    const tool = createVoiceReplyTool();
    expect(tool.definition.name).toBe("voice_reply");
    expect(tool.definition.input_schema.required).toContain("to");
    expect(tool.definition.input_schema.required).toContain("text");
  });

  it("returns error when no sender", async () => {
    const tool = createVoiceReplyTool();
    const result = await tool.execute({ to: "+1234567890", text: "hello" }, ctx);
    expect(JSON.parse(result).error).toContain("Signal not connected");
  });

  it("synthesizes and sends voice message", async () => {
    const sender = { send: vi.fn().mockResolvedValue(undefined) };
    const tool = createVoiceReplyTool(sender as never);
    const result = await tool.execute({ to: "+1234567890", text: "hello" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.sent).toBe(true);
    expect(parsed.engine).toBe("openai");
    expect(sender.send).toHaveBeenCalledWith(
      "+1234567890",
      expect.any(String),
      ["/tmp/test.mp3"],
    );
  });

  it("sends with custom caption", async () => {
    const sender = { send: vi.fn().mockResolvedValue(undefined) };
    const tool = createVoiceReplyTool(sender as never);
    await tool.execute({ to: "+1234567890", text: "hello", caption: "Listen to this" }, ctx);
    expect(sender.send).toHaveBeenCalledWith("+1234567890", "Listen to this", ["/tmp/test.mp3"]);
  });

  it("handles TTS failure", async () => {
    const { synthesize } = await import("../../semeion/tts.js");
    (synthesize as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("No TTS engine"));

    const sender = { send: vi.fn() };
    const tool = createVoiceReplyTool(sender as never);
    const result = await tool.execute({ to: "+1234567890", text: "hello" }, ctx);
    expect(JSON.parse(result).error).toContain("TTS synthesis failed");
  });

  it("handles send failure", async () => {
    const sender = { send: vi.fn().mockRejectedValue(new Error("Network error")) };
    const tool = createVoiceReplyTool(sender as never);
    const result = await tool.execute({ to: "+1234567890", text: "hello" }, ctx);
    expect(JSON.parse(result).error).toContain("Send failed");
  });

  it("calls cleanup after send", async () => {
    const cleanup = vi.fn();
    const { synthesize } = await import("../../semeion/tts.js");
    (synthesize as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      path: "/tmp/test.mp3",
      engine: "openai",
      cleanup,
    });

    const sender = { send: vi.fn().mockResolvedValue(undefined) };
    const tool = createVoiceReplyTool(sender as never);
    await tool.execute({ to: "+1234567890", text: "hello" }, ctx);
    expect(cleanup).toHaveBeenCalled();
  });
});
