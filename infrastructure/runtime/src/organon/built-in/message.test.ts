// Message tool tests — multi-channel routing via agora (Spec 34, Phase 4)
import { describe, expect, it, vi } from "vitest";
import { createMessageTool } from "./message.js";
import { AgoraRegistry } from "../../agora/registry.js";
import type { ChannelProvider } from "../../agora/types.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function stubProvider(id: string, sendResult = { sent: true }): ChannelProvider {
  return {
    id,
    name: id,
    capabilities: {
      threads: false, reactions: false, typing: false,
      media: false, streaming: false, richFormatting: false,
      maxTextLength: 4000,
    },
    start: vi.fn().mockResolvedValue(undefined),
    send: vi.fn().mockResolvedValue(sendResult),
    stop: vi.fn().mockResolvedValue(undefined),
  };
}

function makeRegistry(...providers: ChannelProvider[]): AgoraRegistry {
  const registry = new AgoraRegistry();
  for (const p of providers) registry.register(p);
  return registry;
}

// ---------------------------------------------------------------------------
// Legacy behavior (no registry, direct sender)
// ---------------------------------------------------------------------------

describe("createMessageTool — legacy sender", () => {
  it("has valid definition", () => {
    const tool = createMessageTool();
    expect(tool.definition.name).toBe("message");
    expect(tool.definition.input_schema.required).toContain("to");
    expect(tool.definition.input_schema.required).toContain("text");
  });

  it("returns error when no sender or registry configured", async () => {
    const tool = createMessageTool();
    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    expect(JSON.parse(result).error).toContain("No channels configured");
  });

  it("sends message via legacy sender", async () => {
    const sender = { send: vi.fn().mockResolvedValue(undefined) };
    const tool = createMessageTool({ sender });
    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    const parsed = JSON.parse(result);
    expect(parsed.sent).toBe(true);
    expect(parsed.to).toBe("+1234567890");
    expect(sender.send).toHaveBeenCalledWith("+1234567890", "hello");
  });

  it("rejects recipients not in allowlist", async () => {
    const sender = { send: vi.fn() };
    const tool = createMessageTool({ sender, allowedRecipients: ["+9999"] });
    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    expect(JSON.parse(result).error).toContain("not in allowlist");
    expect(sender.send).not.toHaveBeenCalled();
  });

  it("allows recipients in allowlist", async () => {
    const sender = { send: vi.fn().mockResolvedValue(undefined) };
    const tool = createMessageTool({ sender, allowedRecipients: ["+1234567890"] });
    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    expect(JSON.parse(result).sent).toBe(true);
  });

  it("truncates long messages", async () => {
    const sender = { send: vi.fn().mockResolvedValue(undefined) };
    const tool = createMessageTool({ sender, maxLength: 10 });
    const result = await tool.execute({ to: "+1234567890", text: "this is a very long message" });
    expect(JSON.parse(result).length).toBe(10);
    expect(sender.send).toHaveBeenCalledWith("+1234567890", "this is a ");
  });
});

// ---------------------------------------------------------------------------
// Multi-channel routing via agora registry
// ---------------------------------------------------------------------------

describe("createMessageTool — agora routing", () => {
  it("routes +phone to signal provider", async () => {
    const signal = stubProvider("signal");
    const registry = makeRegistry(signal);
    const tool = createMessageTool({ registry });

    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    const parsed = JSON.parse(result);

    expect(parsed.sent).toBe(true);
    expect(parsed.channel).toBe("signal");
    expect(signal.send).toHaveBeenCalledWith(
      expect.objectContaining({ to: "+1234567890", text: "hello" }),
    );
  });

  it("routes group:ID to signal provider", async () => {
    const signal = stubProvider("signal");
    const registry = makeRegistry(signal);
    const tool = createMessageTool({ registry });

    const result = await tool.execute({ to: "group:ABCDEF", text: "hello" });
    const parsed = JSON.parse(result);

    expect(parsed.sent).toBe(true);
    expect(parsed.channel).toBe("signal");
    expect(signal.send).toHaveBeenCalledWith(
      expect.objectContaining({ to: "group:ABCDEF" }),
    );
  });

  it("routes slack:C0123 to slack provider", async () => {
    const signal = stubProvider("signal");
    const slack = stubProvider("slack");
    const registry = makeRegistry(signal, slack);
    const tool = createMessageTool({ registry });

    const result = await tool.execute({ to: "slack:C0123456789", text: "hello from agora" });
    const parsed = JSON.parse(result);

    expect(parsed.sent).toBe(true);
    expect(parsed.channel).toBe("slack");
    expect(slack.send).toHaveBeenCalledWith(
      expect.objectContaining({ to: "C0123456789", text: "hello from agora" }),
    );
    expect(signal.send).not.toHaveBeenCalled();
  });

  it("routes slack:@username to slack provider", async () => {
    const slack = stubProvider("slack");
    const registry = makeRegistry(slack);
    const tool = createMessageTool({ registry });

    const result = await tool.execute({ to: "slack:@alice", text: "hey" });
    const parsed = JSON.parse(result);

    expect(parsed.sent).toBe(true);
    expect(parsed.channel).toBe("slack");
    expect(slack.send).toHaveBeenCalledWith(
      expect.objectContaining({ to: "@alice" }),
    );
  });

  it("routes signal:+phone explicitly", async () => {
    const signal = stubProvider("signal");
    const slack = stubProvider("slack");
    const registry = makeRegistry(signal, slack);
    const tool = createMessageTool({ registry });

    const result = await tool.execute({ to: "signal:+1234567890", text: "explicit" });
    const parsed = JSON.parse(result);

    expect(parsed.sent).toBe(true);
    expect(parsed.channel).toBe("signal");
    expect(signal.send).toHaveBeenCalled();
    expect(slack.send).not.toHaveBeenCalled();
  });

  it("errors when target channel is not configured", async () => {
    const signal = stubProvider("signal");
    const registry = makeRegistry(signal);
    const tool = createMessageTool({ registry });

    const result = await tool.execute({ to: "slack:C0123", text: "hello" });
    const parsed = JSON.parse(result);

    expect(parsed.error).toContain("not configured");
    expect(parsed.error).toContain("signal"); // shows available channels
  });

  it("errors on invalid target format", async () => {
    const signal = stubProvider("signal");
    const registry = makeRegistry(signal);
    const tool = createMessageTool({ registry });

    const result = await tool.execute({ to: "randomtext", text: "hello" });
    const parsed = JSON.parse(result);

    expect(parsed.error).toContain("Unknown target format");
  });

  it("handles send failure from provider", async () => {
    const signal = stubProvider("signal", { sent: false, error: "Connection lost" } as any);
    const registry = makeRegistry(signal);
    const tool = createMessageTool({ registry });

    const result = await tool.execute({ to: "+1234567890", text: "hello" });
    const parsed = JSON.parse(result);

    expect(parsed.error).toBe("Connection lost");
  });

  it("passes identity to send params", async () => {
    const slack = stubProvider("slack");
    const registry = makeRegistry(slack);
    const identity = { name: "Syn", emoji: "🌀" };
    const tool = createMessageTool({ registry, identity });

    await tool.execute({ to: "slack:C0123", text: "hello" });

    expect(slack.send).toHaveBeenCalledWith(
      expect.objectContaining({ identity: { name: "Syn", emoji: "🌀" } }),
    );
  });

  it("allowlist check runs before routing", async () => {
    const slack = stubProvider("slack");
    const registry = makeRegistry(slack);
    const tool = createMessageTool({ registry, allowedRecipients: ["+9999"] });

    const result = await tool.execute({ to: "slack:C0123", text: "hello" });
    const parsed = JSON.parse(result);

    expect(parsed.error).toContain("not in allowlist");
    expect(slack.send).not.toHaveBeenCalled();
  });

  it("truncates before sending through registry", async () => {
    const signal = stubProvider("signal");
    const registry = makeRegistry(signal);
    const tool = createMessageTool({ registry, maxLength: 5 });

    const result = await tool.execute({ to: "+1234567890", text: "hello world" });
    const parsed = JSON.parse(result);

    expect(parsed.length).toBe(5);
    expect(signal.send).toHaveBeenCalledWith(
      expect.objectContaining({ text: "hello" }),
    );
  });

  it("prefers registry over legacy sender", async () => {
    const signal = stubProvider("signal");
    const registry = makeRegistry(signal);
    const legacySender = { send: vi.fn() };
    const tool = createMessageTool({ registry, sender: legacySender });

    await tool.execute({ to: "+1234567890", text: "hello" });

    expect(signal.send).toHaveBeenCalled();
    expect(legacySender.send).not.toHaveBeenCalled();
  });
});
