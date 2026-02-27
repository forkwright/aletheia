// Tests for SlackChannelProvider (Spec 34, Phase 3)
//
// These test the provider's construction, config gating, and method routing.
// Socket Mode connection requires real Slack tokens, so we test the
// behavioral contract without network calls.

import { describe, expect, it } from "vitest";
import { SlackChannelProvider } from "./provider.js";
import type { AletheiaConfig } from "../../../taxis/schema.js";

function makeConfig(overrides?: Partial<NonNullable<AletheiaConfig["channels"]["slack"]>>): AletheiaConfig {
  return {
    channels: {
      slack: {
        enabled: false,
        mode: "socket",
        appToken: "xapp-1-test",
        botToken: "xoxb-test",
        dmPolicy: "open",
        groupPolicy: "allowlist",
        allowedChannels: [],
        allowedUsers: [],
        requireMention: true,
        identity: { useAgentIdentity: true },
        ...overrides,
      },
    },
  } as unknown as AletheiaConfig;
}

describe("SlackChannelProvider", () => {
  it("has correct id and name", () => {
    const provider = new SlackChannelProvider(makeConfig());
    expect(provider.id).toBe("slack");
    expect(provider.name).toBe("Slack");
  });

  it("advertises correct capabilities", () => {
    const provider = new SlackChannelProvider(makeConfig());
    expect(provider.capabilities.threads).toBe(true);
    expect(provider.capabilities.reactions).toBe(true);
    expect(provider.capabilities.typing).toBe(false);
    expect(provider.capabilities.media).toBe(true);
    expect(provider.capabilities.maxTextLength).toBe(4000);
  });

  it("is not connected before start", () => {
    const provider = new SlackChannelProvider(makeConfig());
    expect(provider.isConnected).toBe(false);
    expect(provider.botUserId).toBeUndefined();
  });

  it("send returns error when not started", async () => {
    const provider = new SlackChannelProvider(makeConfig());
    const result = await provider.send({
      to: "C12345",
      text: "hello",
    });
    expect(result.sent).toBe(false);
    expect(result.error).toContain("not started");
  });

  it("probe returns error when not started", async () => {
    const provider = new SlackChannelProvider(makeConfig());
    const result = await provider.probe();
    expect(result.ok).toBe(false);
    expect(result.error).toContain("not started");
  });

  it("start does nothing when disabled", async () => {
    const provider = new SlackChannelProvider(makeConfig({ enabled: false }));
    // Should not throw
    await provider.start({
      dispatch: async () => {},
    });
    expect(provider.isConnected).toBe(false);
  });

  it("start throws when tokens missing", async () => {
    const config = makeConfig({ enabled: true, appToken: "", botToken: "" });
    const provider = new SlackChannelProvider(config);
    await expect(
      provider.start({ dispatch: async () => {} }),
    ).rejects.toThrow(/appToken.*botToken|requires both/i);
  });

  it("stop is safe when not started", async () => {
    const provider = new SlackChannelProvider(makeConfig());
    // Should not throw
    await provider.stop();
    expect(provider.isConnected).toBe(false);
  });
});
