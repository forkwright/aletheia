// Tests for Slack streaming (Spec 34, Phase 5)

import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  startSlackStream,
  appendSlackStream,
  stopSlackStream,
  type SlackStreamSession,
} from "./streaming.js";

// ---------------------------------------------------------------------------
// Mock WebClient.chatStream() → returns a mock ChatStreamer
// ---------------------------------------------------------------------------

function createMockStreamer() {
  return {
    append: vi.fn().mockResolvedValue(null),
    stop: vi.fn().mockResolvedValue(null),
  };
}

function createMockClient(streamer: ReturnType<typeof createMockStreamer>) {
  return {
    chatStream: vi.fn().mockReturnValue(streamer),
  } as unknown as import("@slack/web-api").WebClient;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("startSlackStream", () => {
  let streamer: ReturnType<typeof createMockStreamer>;
  let client: import("@slack/web-api").WebClient;

  beforeEach(() => {
    streamer = createMockStreamer();
    client = createMockClient(streamer);
  });

  it("creates a ChatStreamer with channel and thread_ts", async () => {
    const session = await startSlackStream({
      client,
      channel: "C123",
      threadTs: "1234.5678",
    });

    expect(client.chatStream).toHaveBeenCalledWith(
      expect.objectContaining({
        channel: "C123",
        thread_ts: "1234.5678",
      }),
    );
    expect(session.channel).toBe("C123");
    expect(session.threadTs).toBe("1234.5678");
    expect(session.stopped).toBe(false);
  });

  it("passes teamId and userId when provided", async () => {
    await startSlackStream({
      client,
      channel: "C123",
      threadTs: "1234.5678",
      teamId: "T456",
      userId: "U789",
    });

    expect(client.chatStream).toHaveBeenCalledWith(
      expect.objectContaining({
        recipient_team_id: "T456",
        recipient_user_id: "U789",
      }),
    );
  });

  it("does not include teamId/userId when not provided", async () => {
    await startSlackStream({
      client,
      channel: "C123",
      threadTs: "1234.5678",
    });

    const args = (client.chatStream as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(args).not.toHaveProperty("recipient_team_id");
    expect(args).not.toHaveProperty("recipient_user_id");
  });

  it("appends initial text when provided", async () => {
    await startSlackStream({
      client,
      channel: "C123",
      threadTs: "1234.5678",
      text: "Hello",
    });

    expect(streamer.append).toHaveBeenCalledWith({ markdown_text: "Hello" });
  });

  it("does not append when no initial text", async () => {
    await startSlackStream({
      client,
      channel: "C123",
      threadTs: "1234.5678",
    });

    expect(streamer.append).not.toHaveBeenCalled();
  });
});

describe("appendSlackStream", () => {
  it("appends text to the streamer", async () => {
    const streamer = createMockStreamer();
    const session: SlackStreamSession = {
      streamer: streamer as never,
      channel: "C123",
      threadTs: "1234.5678",
      stopped: false,
    };

    await appendSlackStream({ session, text: "world" });
    expect(streamer.append).toHaveBeenCalledWith({ markdown_text: "world" });
  });

  it("ignores appends to stopped streams", async () => {
    const streamer = createMockStreamer();
    const session: SlackStreamSession = {
      streamer: streamer as never,
      channel: "C123",
      threadTs: "1234.5678",
      stopped: true,
    };

    await appendSlackStream({ session, text: "world" });
    expect(streamer.append).not.toHaveBeenCalled();
  });

  it("ignores empty text", async () => {
    const streamer = createMockStreamer();
    const session: SlackStreamSession = {
      streamer: streamer as never,
      channel: "C123",
      threadTs: "1234.5678",
      stopped: false,
    };

    await appendSlackStream({ session, text: "" });
    expect(streamer.append).not.toHaveBeenCalled();
  });
});

describe("stopSlackStream", () => {
  it("stops the streamer", async () => {
    const streamer = createMockStreamer();
    const session: SlackStreamSession = {
      streamer: streamer as never,
      channel: "C123",
      threadTs: "1234.5678",
      stopped: false,
    };

    await stopSlackStream({ session });
    expect(streamer.stop).toHaveBeenCalledWith(undefined);
    expect(session.stopped).toBe(true);
  });

  it("includes final text when provided", async () => {
    const streamer = createMockStreamer();
    const session: SlackStreamSession = {
      streamer: streamer as never,
      channel: "C123",
      threadTs: "1234.5678",
      stopped: false,
    };

    await stopSlackStream({ session, text: "Done!" });
    expect(streamer.stop).toHaveBeenCalledWith({ markdown_text: "Done!" });
  });

  it("ignores duplicate stop calls", async () => {
    const streamer = createMockStreamer();
    const session: SlackStreamSession = {
      streamer: streamer as never,
      channel: "C123",
      threadTs: "1234.5678",
      stopped: true,
    };

    await stopSlackStream({ session });
    expect(streamer.stop).not.toHaveBeenCalled();
  });
});
