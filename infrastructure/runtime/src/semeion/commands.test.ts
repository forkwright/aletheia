// Command registry tests
import { describe, it, expect } from "vitest";
import { CommandRegistry, createDefaultRegistry } from "./commands.js";

describe("CommandRegistry", () => {
  it("registers and matches commands", () => {
    const reg = new CommandRegistry();
    reg.register({
      name: "test",
      description: "Test command",
      execute: async () => "done",
    });
    const match = reg.match("!test");
    expect(match).not.toBeNull();
    expect(match!.handler.name).toBe("test");
    expect(match!.args).toBe("");
  });

  it("parses arguments after command", () => {
    const reg = new CommandRegistry();
    reg.register({
      name: "send",
      description: "Send message",
      execute: async () => "sent",
    });
    const match = reg.match("!send hello world");
    expect(match!.args).toBe("hello world");
  });

  it("is case-insensitive", () => {
    const reg = new CommandRegistry();
    reg.register({
      name: "ping",
      description: "Ping",
      execute: async () => "pong",
    });
    expect(reg.match("!PING")).not.toBeNull();
    expect(reg.match("!Ping")).not.toBeNull();
  });

  it("matches / prefix (slash commands)", () => {
    const reg = new CommandRegistry();
    reg.register({
      name: "test",
      description: "Test",
      execute: async () => "done",
    });
    const match = reg.match("/test");
    expect(match).not.toBeNull();
    expect(match!.handler.name).toBe("test");
  });

  it("matches / prefix with arguments", () => {
    const reg = new CommandRegistry();
    reg.register({
      name: "model",
      description: "Switch model",
      execute: async () => "done",
    });
    const match = reg.match("/model sonnet");
    expect(match).not.toBeNull();
    expect(match!.args).toBe("sonnet");
  });

  it("returns null for non-command text", () => {
    const reg = new CommandRegistry();
    expect(reg.match("hello")).toBeNull();
    expect(reg.match("")).toBeNull();
  });

  it("returns null for unknown commands", () => {
    const reg = new CommandRegistry();
    expect(reg.match("!unknown")).toBeNull();
  });

  it("supports aliases", () => {
    const reg = new CommandRegistry();
    reg.register({
      name: "help",
      aliases: ["h", "commands"],
      description: "Help",
      execute: async () => "help text",
    });
    expect(reg.match("!h")).not.toBeNull();
    expect(reg.match("!commands")).not.toBeNull();
    expect(reg.match("!help")).not.toBeNull();
  });

  it("listAll deduplicates aliases", () => {
    const reg = new CommandRegistry();
    reg.register({
      name: "help",
      aliases: ["h", "commands"],
      description: "Help",
      execute: async () => "help",
    });
    const all = reg.listAll();
    expect(all).toHaveLength(1);
    expect(all[0]!.name).toBe("help");
  });
});

describe("createDefaultRegistry", () => {
  it("returns a registry with default commands", () => {
    const reg = createDefaultRegistry();
    const all = reg.listAll();
    expect(all.length).toBeGreaterThanOrEqual(14);
    const names = all.map((c) => c.name);
    expect(names).toContain("ping");
    expect(names).toContain("help");
    expect(names).toContain("status");
    expect(names).toContain("sessions");
    expect(names).toContain("reset");
    expect(names).toContain("agent");
    expect(names).toContain("skills");
    expect(names).toContain("model");
    expect(names).toContain("think");
    expect(names).toContain("distill");
    expect(names).toContain("blackboard");
    expect(names).toContain("approve");
    expect(names).toContain("deny");
    expect(names).toContain("contacts");
  });

  it("ping command returns pong", async () => {
    const reg = createDefaultRegistry();
    const match = reg.match("!ping");
    const result = await match!.handler.execute("", {} as never);
    expect(result).toBe("pong");
  });

  it("help alias works", () => {
    const reg = createDefaultRegistry();
    expect(reg.match("!commands")).not.toBeNull();
  });
});
