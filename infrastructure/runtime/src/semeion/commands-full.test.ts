// Full command execution tests — covers !status, !help, !sessions, !reset, etc.
import { describe, expect, it, vi } from "vitest";
import { createDefaultRegistry } from "./commands.js";

function makeCtx(overrides: Record<string, unknown> = {}) {
  return {
    sender: "uuid-123",
    senderName: "Test User",
    isGroup: false,
    accountId: "main",
    target: { account: "+1", recipient: "+2" },
    client: {} as never,
    store: {
      getMetrics: vi.fn().mockReturnValue({
        usage: { totalInputTokens: 100000, totalOutputTokens: 50000, totalCacheReadTokens: 30000, totalCacheWriteTokens: 5000, turnCount: 42 },
        perNous: { syn: { activeSessions: 2, totalMessages: 15, lastActivity: new Date().toISOString() } },
        usageByNous: { syn: { inputTokens: 80000, outputTokens: 40000 } },
      }),
      findSessionsByKey: vi.fn().mockReturnValue([
        { id: "ses_1", nousId: "syn", messageCount: 10, updatedAt: new Date().toISOString() },
      ]),
      findSessionById: vi.fn().mockReturnValue(null),
      archiveSession: vi.fn(),
      approveContactByCode: vi.fn().mockReturnValue({ sender: "+999", channel: "signal" }),
      denyContactByCode: vi.fn().mockReturnValue(true),
      getPendingRequests: vi.fn().mockReturnValue([
        { senderName: "John", code: "ABC123", createdAt: new Date().toISOString() },
      ]),
    } as never,
    config: {
      agents: {
        defaults: { model: { primary: "claude-opus-4-6", fallbacks: [] } },
        list: [
          { id: "syn", name: "Syn" },
          { id: "eiron", name: "Eiron" },
        ],
      },
    } as never,
    manager: {} as never,
    watchdog: {
      getStatus: vi.fn().mockReturnValue([
        { name: "neo4j", healthy: true },
        { name: "qdrant", healthy: true },
      ]),
    } as never,
    skills: {
      size: 1,
      listAll: vi.fn().mockReturnValue([{ name: "web-research", description: "Research a topic" }]),
    } as never,
    ...overrides,
  } as never;
}

describe("command execution", () => {
  const registry = createDefaultRegistry();

  it("!ping returns status indicator", async () => {
    const match = registry.match("!ping");
    expect(match).not.toBeNull();
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("**pong**");
    expect(result).toContain("uptime");
  });

  it("!help lists commands as table", async () => {
    const match = registry.match("!help");
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("**Commands**");
    expect(result).toContain("| Command | Description |");
    expect(result).toContain("`/ping`");
    expect(result).toContain("`/status`");
  });

  it("help uses ! prefix in group context", async () => {
    const match = registry.match("!help");
    const result = await match!.handler.execute("", makeCtx({ target: { account: "+1", groupId: "grp1" } }));
    expect(result).toContain("`!ping`");
    expect(result).toContain("`!status`");
  });

  it("!commands is alias for help", async () => {
    const match = registry.match("!commands");
    expect(match!.handler.name).toBe("help");
  });

  it("!status shows system overview with tables", async () => {
    const match = registry.match("!status");
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("## Aletheia Status");
    expect(result).toContain("**Uptime:**");
    expect(result).toContain("| Syn |");
    expect(result).toContain("**Tokens:**");
    expect(result).toContain("**Cache:**");
  });

  it("!status shows service table", async () => {
    const match = registry.match("!status");
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("### Services");
    expect(result).toContain("| neo4j | \u2713 healthy |");
  });

  it("!status shows degraded when services unhealthy", async () => {
    const ctx = makeCtx({
      watchdog: {
        getStatus: vi.fn().mockReturnValue([
          { name: "neo4j", healthy: false },
        ]),
      },
    });
    const match = registry.match("!status");
    const result = await match!.handler.execute("", ctx);
    expect(result).toContain("DEGRADED");
    expect(result).toContain("\u2717 down");
  });

  it("!sessions lists active sessions as table", async () => {
    const match = registry.match("!sessions");
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("**Active sessions**");
    expect(result).toContain("| syn |");
  });

  it("!sessions returns no sessions message", async () => {
    const ctx = makeCtx();
    ((ctx as unknown as { store: { findSessionsByKey: ReturnType<typeof vi.fn> } }).store.findSessionsByKey).mockReturnValue([]);
    const match = registry.match("!sessions");
    const result = await match!.handler.execute("", ctx);
    expect(result).toContain("No active sessions");
  });

  it("!reset archives sessions", async () => {
    const ctx = makeCtx();
    const match = registry.match("!reset");
    const result = await match!.handler.execute("", ctx);
    expect(result).toContain("archived");
    expect(result).toContain("10 messages");
  });

  it("!reset handles no sessions", async () => {
    const ctx = makeCtx();
    ((ctx as unknown as { store: { findSessionsByKey: ReturnType<typeof vi.fn> } }).store.findSessionsByKey).mockReturnValue([]);
    const match = registry.match("!reset");
    const result = await match!.handler.execute("", ctx);
    expect(result).toContain("No active session");
  });

  it("!agent shows current agent formatted", async () => {
    const match = registry.match("!agent");
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("**Agent:**");
    expect(result).toContain("`Syn`");
  });

  it("!agent with args returns routing note", async () => {
    const match = registry.match("!agent");
    const result = await match!.handler.execute("eiron", makeCtx());
    expect(result).toContain("config bindings");
  });

  it("!skills lists skills as table", async () => {
    const match = registry.match("!skills");
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("**Available skills**");
    expect(result).toContain("| `web-research` |");
  });

  it("!skills with no skills loaded", async () => {
    const ctx = makeCtx({ skills: { size: 0, listAll: vi.fn().mockReturnValue([]) } });
    const match = registry.match("!skills");
    const result = await match!.handler.execute("", ctx);
    expect(result).toContain("No skills loaded");
  });

  it("!approve approves contact", async () => {
    const match = registry.match("!approve");
    const result = await match!.handler.execute("ABC123", makeCtx());
    expect(result).toContain("**Approved contact:**");
  });

  it("!approve requires code", async () => {
    const match = registry.match("!approve");
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("**Usage:**");
  });

  it("!approve handles no match", async () => {
    const ctx = makeCtx();
    ((ctx as unknown as { store: { approveContactByCode: ReturnType<typeof vi.fn> } }).store.approveContactByCode).mockReturnValue(null);
    const match = registry.match("!approve");
    const result = await match!.handler.execute("BADCODE", ctx);
    expect(result).toContain("No pending request");
  });

  it("!deny denies contact", async () => {
    const match = registry.match("!deny");
    const result = await match!.handler.execute("ABC123", makeCtx());
    expect(result).toContain("**Denied**");
  });

  it("!deny requires code", async () => {
    const match = registry.match("!deny");
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("**Usage:**");
  });

  it("!contacts lists pending requests as table", async () => {
    const match = registry.match("!contacts");
    const result = await match!.handler.execute("", makeCtx());
    expect(result).toContain("**Pending contact requests**");
    expect(result).toContain("| John |");
    expect(result).toContain("`ABC123`");
  });

  it("!contacts with no pending", async () => {
    const ctx = makeCtx();
    ((ctx as unknown as { store: { getPendingRequests: ReturnType<typeof vi.fn> } }).store.getPendingRequests).mockReturnValue([]);
    const match = registry.match("!contacts");
    const result = await match!.handler.execute("", ctx);
    expect(result).toContain("No pending");
  });
});
