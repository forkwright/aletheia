// Config read tool tests
import { describe, it, expect, vi } from "vitest";
import { createConfigReadTool } from "./config-read.js";

function makeConfig() {
  return {
    agents: {
      list: [
        { id: "syn", name: "Syn", workspace: "/tmp/syn", model: null, tools: { allow: [], deny: [] }, heartbeat: null },
        { id: "eiron", name: "Eiron", workspace: "/tmp/eiron", model: "claude-haiku", tools: { allow: [], deny: [] }, heartbeat: null },
      ],
      default: "syn",
      defaults: { model: "claude-sonnet" },
    },
    bindings: [
      { agentId: "syn", channel: "signal", peerId: "+1234" },
      { agentId: "eiron", channel: "signal", peerId: "+5678" },
    ],
    cron: {
      jobs: [
        { id: "heartbeat", agentId: "syn", schedule: "every 45m" },
        { id: "global", schedule: "every 1h" },
      ],
    },
    gateway: { port: 18789, bind: "0.0.0.0" },
    plugins: {
      enabled: true,
      entries: {
        "aletheia-memory": { enabled: true },
        "test-plugin": { enabled: false },
      },
    },
  } as never;
}

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp/syn" };

describe("createConfigReadTool", () => {
  it("has valid definition", () => {
    const tool = createConfigReadTool(makeConfig());
    expect(tool.definition.name).toBe("config_read");
  });

  it("returns agent config", async () => {
    const tool = createConfigReadTool(makeConfig());
    const result = await tool.execute({ section: "agent" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.id).toBe("syn");
    expect(parsed.name).toBe("Syn");
  });

  it("returns all agents list", async () => {
    const tool = createConfigReadTool(makeConfig());
    const result = await tool.execute({ section: "agents" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed).toHaveLength(2);
    expect(parsed[0].id).toBe("syn");
  });

  it("returns bindings for current agent", async () => {
    const tool = createConfigReadTool(makeConfig());
    const result = await tool.execute({ section: "bindings" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed).toHaveLength(1);
    expect(parsed[0].peerId).toBe("+1234");
  });

  it("returns cron jobs for current agent", async () => {
    const tool = createConfigReadTool(makeConfig());
    const result = await tool.execute({ section: "cron" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed).toHaveLength(2); // syn job + global job (no agentId filter)
  });

  it("returns gateway config", async () => {
    const tool = createConfigReadTool(makeConfig());
    const result = await tool.execute({ section: "gateway" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.port).toBe(18789);
  });

  it("returns plugins config", async () => {
    const tool = createConfigReadTool(makeConfig());
    const result = await tool.execute({ section: "plugins" }, ctx);
    const parsed = JSON.parse(result);
    expect(parsed.enabled).toBe(true);
    expect(parsed.count).toBe(2);
    expect(parsed.plugins).toHaveLength(2);
  });

  it("returns error for unknown section", async () => {
    const tool = createConfigReadTool(makeConfig());
    const result = await tool.execute({ section: "unknown" }, ctx);
    expect(JSON.parse(result).error).toContain("Unknown section");
  });

  it("returns error when config not available", async () => {
    const tool = createConfigReadTool();
    const result = await tool.execute({ section: "agent" }, ctx);
    expect(JSON.parse(result).error).toContain("not available");
  });

  it("returns error for unknown nous", async () => {
    const tool = createConfigReadTool(makeConfig());
    const result = await tool.execute({ section: "agent" }, { ...ctx, nousId: "unknown" });
    expect(JSON.parse(result).error).toContain("Unknown nous");
  });
});
