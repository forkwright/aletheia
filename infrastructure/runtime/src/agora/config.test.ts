// Tests for Slack channel config schema validation (Spec 34)
import { describe, it, expect } from "vitest";

// We test via the full AletheiaConfig schema to ensure Slack config integrates properly
import { AletheiaConfigSchema } from "../taxis/schema.js";

function parseConfig(channels: unknown) {
  return AletheiaConfigSchema.safeParse({
    models: { primary: { provider: "anthropic", model: "claude-sonnet-4-20250514" } },
    agents: { list: [] },
    bindings: [],
    channels,
  });
}

describe("Slack channel config schema", () => {
  it("accepts minimal slack config", async () => {
    const result = await parseConfig({
      signal: { enabled: true, accounts: {} },
      slack: {
        enabled: true,
        appToken: "xapp-1-A0123",
        botToken: "xoxb-1234",
      },
    });
    expect(result.success).toBe(true);
    if (result.success) {
      const slack = result.data.channels.slack;
      expect(slack.enabled).toBe(true);
      expect(slack.mode).toBe("socket"); // default
      expect(slack.dmPolicy).toBe("open"); // default
      expect(slack.requireMention).toBe(true); // default
    }
  });

  it("accepts full slack config with all fields", async () => {
    const result = await parseConfig({
      signal: { enabled: true, accounts: {} },
      slack: {
        enabled: true,
        mode: "socket",
        appToken: "xapp-1-A0123",
        botToken: "xoxb-1234",
        dmPolicy: "allowlist",
        groupPolicy: "disabled",
        allowedChannels: ["C0123456789"],
        allowedUsers: ["U0123456789"],
        requireMention: false,
        identity: { useAgentIdentity: false },
      },
    });
    expect(result.success).toBe(true);
    if (result.success) {
      const slack = result.data.channels.slack;
      expect(slack.dmPolicy).toBe("allowlist");
      expect(slack.groupPolicy).toBe("disabled");
      expect(slack.allowedChannels).toEqual(["C0123456789"]);
      expect(slack.requireMention).toBe(false);
      expect(slack.identity.useAgentIdentity).toBe(false);
    }
  });

  it("omitting slack entirely is valid (optional)", async () => {
    const result = await parseConfig({
      signal: { enabled: true, accounts: {} },
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.channels.slack).toBeUndefined();
    }
  });

  it("rejects invalid mode", async () => {
    const result = await parseConfig({
      signal: { enabled: true, accounts: {} },
      slack: {
        enabled: true,
        mode: "websocket", // invalid
        appToken: "xapp-1",
        botToken: "xoxb-1",
      },
    });
    expect(result.success).toBe(false);
  });

  it("rejects invalid dmPolicy", async () => {
    const result = await parseConfig({
      signal: { enabled: true, accounts: {} },
      slack: {
        enabled: true,
        dmPolicy: "everyone", // invalid
      },
    });
    expect(result.success).toBe(false);
  });

  it("defaults enabled to false", async () => {
    const result = await parseConfig({
      signal: { enabled: true, accounts: {} },
      slack: {},
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.channels.slack.enabled).toBe(false);
    }
  });

  it("defaults allowedChannels to empty array", async () => {
    const result = await parseConfig({
      signal: { enabled: true, accounts: {} },
      slack: { enabled: true },
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.channels.slack.allowedChannels).toEqual([]);
    }
  });
});
