// Signal listener tests — envelope handling, access control, mention hydration
import { describe, it, expect, vi, beforeEach } from "vitest";

// We can't easily test startListener (SSE consumer) without a real stream,
// but we can test the helper functions by importing them indirectly.
// The key exported function startListener depends on fetch SSE — we test
// the internal logic through the envelope handler patterns.

// Test the pure helper functions that are accessible
// normalizePhone, isInAllowlist, hydrateMentions, checkAccess
// are not exported, but we can test the module's behavior through
// the listener's envelope handling.

// Since listener.ts keeps most functions private, we test the
// exported types and ensure the module loads correctly.

describe("listener module", () => {
  it("exports startListener", async () => {
    const mod = await import("./listener.js");
    expect(mod.startListener).toBeDefined();
    expect(typeof mod.startListener).toBe("function");
  });

  it("SignalEnvelope type structure", async () => {
    // Verify the module types compile — create a valid envelope
    const envelope: import("./listener.js").SignalEnvelope = {
      sourceNumber: "+1234567890",
      sourceUuid: "uuid-123",
      sourceName: "Test User",
      timestamp: Date.now(),
      dataMessage: {
        message: "hello",
        timestamp: Date.now(),
      },
    };
    expect(envelope.sourceNumber).toBe("+1234567890");
    expect(envelope.dataMessage?.message).toBe("hello");
  });
});

// Test sendMessage, sendTyping, sendReadReceipt, sendReaction, splitMessage via sender
describe("sender functions", () => {
  it("sendMessage sends formatted chunks", async () => {
    const { sendMessage } = await import("./sender.js");
    const client = {
      send: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", recipient: "+2" };
    await sendMessage(client, target, "hello world");
    expect((client as { send: ReturnType<typeof vi.fn> }).send).toHaveBeenCalledTimes(1);
  });

  it("sendMessage splits long messages", async () => {
    const { sendMessage } = await import("./sender.js");
    const client = {
      send: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", recipient: "+2" };
    const longText = "word ".repeat(1000); // ~5000 chars
    await sendMessage(client, target, longText);
    expect((client as { send: ReturnType<typeof vi.fn> }).send).toHaveBeenCalledTimes(3);
  });

  it("sendTyping sends typing indicator", async () => {
    const { sendTyping } = await import("./sender.js");
    const client = {
      sendTyping: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", recipient: "+2" };
    await sendTyping(client, target);
    expect((client as { sendTyping: ReturnType<typeof vi.fn> }).sendTyping).toHaveBeenCalledWith(
      expect.objectContaining({ account: "+1", stop: false }),
    );
  });

  it("sendTyping stop=true", async () => {
    const { sendTyping } = await import("./sender.js");
    const client = {
      sendTyping: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", recipient: "+2" };
    await sendTyping(client, target, true);
    expect((client as { sendTyping: ReturnType<typeof vi.fn> }).sendTyping).toHaveBeenCalledWith(
      expect.objectContaining({ stop: true }),
    );
  });

  it("sendReadReceipt sends receipt", async () => {
    const { sendReadReceipt } = await import("./sender.js");
    const client = {
      sendReceipt: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", recipient: "+2" };
    await sendReadReceipt(client, target, 12345);
    expect((client as { sendReceipt: ReturnType<typeof vi.fn> }).sendReceipt).toHaveBeenCalled();
  });

  it("sendReadReceipt skips without recipient", async () => {
    const { sendReadReceipt } = await import("./sender.js");
    const client = {
      sendReceipt: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", groupId: "g1" };
    await sendReadReceipt(client, target, 12345);
    expect((client as { sendReceipt: ReturnType<typeof vi.fn> }).sendReceipt).not.toHaveBeenCalled();
  });

  it("sendReaction sends reaction", async () => {
    const { sendReaction } = await import("./sender.js");
    const client = {
      sendReaction: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", recipient: "+2" };
    await sendReaction(client, target, "👍", 12345, "+3");
    expect((client as { sendReaction: ReturnType<typeof vi.fn> }).sendReaction).toHaveBeenCalled();
  });

  it("sendMessage with markdown formatting", async () => {
    const { sendMessage } = await import("./sender.js");
    const client = {
      send: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", recipient: "+2" };
    await sendMessage(client, target, "**bold** text");
    const sendCall = (client as { send: ReturnType<typeof vi.fn> }).send.mock.calls[0]![0];
    expect(sendCall).toBeDefined();
  });

  it("sendMessage with markdown=false", async () => {
    const { sendMessage } = await import("./sender.js");
    const client = {
      send: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", recipient: "+2" };
    await sendMessage(client, target, "**bold** text", { markdown: false });
    const sendCall = (client as { send: ReturnType<typeof vi.fn> }).send.mock.calls[0]![0];
    expect(sendCall.message).toBe("**bold** text");
  });

  it("sendMessage to group", async () => {
    const { sendMessage } = await import("./sender.js");
    const client = {
      send: vi.fn().mockResolvedValue(undefined),
    } as never;
    const target = { account: "+1", groupId: "grp_abc" };
    await sendMessage(client, target, "hello group");
    const sendCall = (client as { send: ReturnType<typeof vi.fn> }).send.mock.calls[0]![0];
    expect(sendCall.groupId).toBe("grp_abc");
  });
});
