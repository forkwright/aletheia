// Tests for Slack reactions (Spec 34, Phase 5)

import { describe, it, expect, vi, beforeEach } from "vitest";
import { addSlackReaction, removeSlackReaction } from "./reactions.js";

// ---------------------------------------------------------------------------
// Mock WebClient
// ---------------------------------------------------------------------------

function createMockClient() {
  return {
    reactions: {
      add: vi.fn().mockResolvedValue({ ok: true }),
      remove: vi.fn().mockResolvedValue({ ok: true }),
    },
  } as unknown as import("@slack/web-api").WebClient;
}

function slackError(code: string): Error {
  const err = new Error(`An API error occurred: ${code}`);
  (err as Error & { data: { error: string } }).data = { error: code };
  return err;
}

// ---------------------------------------------------------------------------
// addSlackReaction
// ---------------------------------------------------------------------------

describe("addSlackReaction", () => {
  let client: import("@slack/web-api").WebClient;

  beforeEach(() => {
    client = createMockClient();
  });

  it("adds a reaction", async () => {
    const result = await addSlackReaction({
      client,
      channel: "C123",
      timestamp: "1234.5678",
      emoji: "thumbsup",
    });

    expect(result).toBe(true);
    expect(client.reactions.add).toHaveBeenCalledWith({
      channel: "C123",
      timestamp: "1234.5678",
      name: "thumbsup",
    });
  });

  it("strips colons from emoji", async () => {
    await addSlackReaction({
      client,
      channel: "C123",
      timestamp: "1234.5678",
      emoji: ":rocket:",
    });

    expect(client.reactions.add).toHaveBeenCalledWith(
      expect.objectContaining({ name: "rocket" }),
    );
  });

  it("handles already_reacted gracefully", async () => {
    (client.reactions.add as ReturnType<typeof vi.fn>).mockRejectedValue(
      slackError("already_reacted"),
    );

    const result = await addSlackReaction({
      client,
      channel: "C123",
      timestamp: "1234.5678",
      emoji: "thumbsup",
    });

    expect(result).toBe(true);
  });

  it("returns false on other errors", async () => {
    (client.reactions.add as ReturnType<typeof vi.fn>).mockRejectedValue(
      slackError("channel_not_found"),
    );

    const result = await addSlackReaction({
      client,
      channel: "C123",
      timestamp: "1234.5678",
      emoji: "thumbsup",
    });

    expect(result).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// removeSlackReaction
// ---------------------------------------------------------------------------

describe("removeSlackReaction", () => {
  let client: import("@slack/web-api").WebClient;

  beforeEach(() => {
    client = createMockClient();
  });

  it("removes a reaction", async () => {
    const result = await removeSlackReaction({
      client,
      channel: "C123",
      timestamp: "1234.5678",
      emoji: "thumbsup",
    });

    expect(result).toBe(true);
    expect(client.reactions.remove).toHaveBeenCalledWith({
      channel: "C123",
      timestamp: "1234.5678",
      name: "thumbsup",
    });
  });

  it("handles no_reaction gracefully", async () => {
    (client.reactions.remove as ReturnType<typeof vi.fn>).mockRejectedValue(
      slackError("no_reaction"),
    );

    const result = await removeSlackReaction({
      client,
      channel: "C123",
      timestamp: "1234.5678",
      emoji: "thumbsup",
    });

    expect(result).toBe(true);
  });

  it("returns false on other errors", async () => {
    (client.reactions.remove as ReturnType<typeof vi.fn>).mockRejectedValue(
      slackError("channel_not_found"),
    );

    const result = await removeSlackReaction({
      client,
      channel: "C123",
      timestamp: "1234.5678",
      emoji: "thumbsup",
    });

    expect(result).toBe(false);
  });
});
