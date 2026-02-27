// Tests for Slack outbound sender (Spec 34, Phase 3)

import { describe, expect, it, vi } from "vitest";
import { sendSlackMessage, type SlackSenderContext } from "./sender.js";
import type { ChannelSendParams } from "../../types.js";

// Mock WebClient
function createMockWebClient(overrides?: {
  postMessage?: (args: Record<string, unknown>) => Promise<Record<string, unknown>>;
}): SlackSenderContext {
  return {
    webClient: {
      chat: {
        postMessage:
          overrides?.postMessage ??
          vi.fn().mockResolvedValue({ ok: true, ts: "1234567890.123456" }),
      },
      filesUploadV2: vi.fn().mockResolvedValue({ ok: true, files: [{ id: "F123" }] }),
    } as unknown as SlackSenderContext["webClient"],
  };
}

describe("sendSlackMessage", () => {
  it("returns error when no target specified", async () => {
    const ctx = createMockWebClient();
    const result = await sendSlackMessage(ctx, {
      to: "",
      text: "hello",
    } as ChannelSendParams);
    expect(result.sent).toBe(false);
    expect(result.error).toContain("No target");
  });

  it("sends a simple text message", async () => {
    const postMessage = vi.fn().mockResolvedValue({ ok: true, ts: "123" });
    const ctx = createMockWebClient({ postMessage });

    const result = await sendSlackMessage(ctx, {
      to: "C12345",
      text: "Hello world",
    } as ChannelSendParams);

    expect(result.sent).toBe(true);
    expect(postMessage).toHaveBeenCalledTimes(1);
    const args = postMessage.mock.calls[0]![0] as Record<string, unknown>;
    expect(args.channel).toBe("C12345");
    expect(args.text).toBe("Hello world");
  });

  it("converts markdown to mrkdwn", async () => {
    const postMessage = vi.fn().mockResolvedValue({ ok: true, ts: "123" });
    const ctx = createMockWebClient({ postMessage });

    await sendSlackMessage(ctx, {
      to: "C12345",
      text: "[link](https://example.com)",
    } as ChannelSendParams);

    const args = postMessage.mock.calls[0]![0] as Record<string, unknown>;
    expect(args.text).toBe("<https://example.com|link>");
  });

  it("skips markdown conversion when markdown=false", async () => {
    const postMessage = vi.fn().mockResolvedValue({ ok: true, ts: "123" });
    const ctx = createMockWebClient({ postMessage });

    await sendSlackMessage(ctx, {
      to: "C12345",
      text: "[link](https://example.com)",
      markdown: false,
    } as ChannelSendParams);

    const args = postMessage.mock.calls[0]![0] as Record<string, unknown>;
    expect(args.text).toBe("[link](https://example.com)");
  });

  it("chunks long messages", async () => {
    const postMessage = vi.fn().mockResolvedValue({ ok: true, ts: "123" });
    const ctx = createMockWebClient({ postMessage });

    const longText = "a".repeat(3500) + "\n\n" + "b".repeat(3500);
    await sendSlackMessage(ctx, {
      to: "C12345",
      text: longText,
      markdown: false,
    } as ChannelSendParams);

    expect(postMessage).toHaveBeenCalledTimes(2);
  });

  it("passes thread_ts for threaded replies", async () => {
    const postMessage = vi.fn().mockResolvedValue({ ok: true, ts: "123" });
    const ctx = createMockWebClient({ postMessage });

    await sendSlackMessage(ctx, {
      to: "C12345",
      text: "reply",
      threadId: "1234567890.111111",
    } as ChannelSendParams);

    const args = postMessage.mock.calls[0]![0] as Record<string, unknown>;
    expect(args.thread_ts).toBe("1234567890.111111");
  });

  it("passes identity for custom username and emoji", async () => {
    const postMessage = vi.fn().mockResolvedValue({ ok: true, ts: "123" });
    const ctx = createMockWebClient({ postMessage });

    await sendSlackMessage(ctx, {
      to: "C12345",
      text: "hello",
      identity: { name: "Syn", emoji: "cyclone" },
    } as ChannelSendParams);

    const args = postMessage.mock.calls[0]![0] as Record<string, unknown>;
    expect(args.username).toBe("Syn");
    expect(args.icon_emoji).toBe(":cyclone:");
  });

  it("falls back on missing chat:write.customize scope", async () => {
    let callCount = 0;
    const postMessage = vi.fn().mockImplementation(async (args: Record<string, unknown>) => {
      callCount++;
      if (callCount === 1 && args.username) {
        const err = new Error("missing_scope");
        (err as Error & { data: unknown }).data = {
          error: "missing_scope",
          needed: "chat:write.customize",
        };
        throw err;
      }
      return { ok: true, ts: "123" };
    });
    const ctx = createMockWebClient({ postMessage });

    const result = await sendSlackMessage(ctx, {
      to: "C12345",
      text: "hello",
      identity: { name: "Syn" },
    } as ChannelSendParams);

    expect(result.sent).toBe(true);
    expect(postMessage).toHaveBeenCalledTimes(2);
    // Second call should NOT have username
    const fallbackArgs = postMessage.mock.calls[1]![0] as Record<string, unknown>;
    expect(fallbackArgs.username).toBeUndefined();
  });

  it("returns error on non-scope Slack failure", async () => {
    const postMessage = vi.fn().mockRejectedValue(new Error("channel_not_found"));
    const ctx = createMockWebClient({ postMessage });

    const result = await sendSlackMessage(ctx, {
      to: "C_INVALID",
      text: "hello",
    } as ChannelSendParams);

    expect(result.sent).toBe(false);
    expect(result.error).toContain("channel_not_found");
  });
});
