// Tests for agent workspace scaffolding
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, readFileSync, existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import { validateAgentId, scaffoldAgent } from "./scaffold.js";

const TEST_ROOT = `/tmp/scaffold-test-${Date.now()}`;

function setupFixture() {
  const nousDir = join(TEST_ROOT, "nous");
  const templateDir = join(nousDir, "_example");
  const configPath = join(TEST_ROOT, "aletheia.json");

  mkdirSync(templateDir, { recursive: true });

  for (const file of ["AGENTS.md", "GOALS.md", "MEMORY.md", "TOOLS.md", "CONTEXT.md", "PROSOCHE.md"]) {
    writeFileSync(join(templateDir, file), `# ${file}\nTemplate content.\n`);
  }
  writeFileSync(join(templateDir, "SOUL.md"), "# Example Soul\nTemplate.\n");
  writeFileSync(join(templateDir, "IDENTITY.md"), "name: Atlas\nemoji: 🗺️\n");
  writeFileSync(join(templateDir, "USER.md"), "# User\nTemplate.\n");

  const config = {
    agents: { defaults: {}, list: [{ id: "syn", workspace: join(nousDir, "syn") }] },
    bindings: [{ agentId: "syn", match: { channel: "signal" } }],
  };
  writeFileSync(configPath, JSON.stringify(config, null, 2));

  return { nousDir, templateDir, configPath };
}

describe("validateAgentId", () => {
  it("accepts valid IDs", () => {
    expect(validateAgentId("atlas").valid).toBe(true);
    expect(validateAgentId("my-agent").valid).toBe(true);
    expect(validateAgentId("a1").valid).toBe(true);
    expect(validateAgentId("test-agent-99").valid).toBe(true);
  });

  it("rejects empty ID", () => {
    const result = validateAgentId("");
    expect(result.valid).toBe(false);
    expect(result.reason).toContain("empty");
  });

  it("rejects uppercase", () => {
    expect(validateAgentId("Atlas").valid).toBe(false);
  });

  it("rejects spaces", () => {
    expect(validateAgentId("my agent").valid).toBe(false);
  });

  it("rejects reserved names", () => {
    expect(validateAgentId("_example").valid).toBe(false);
    expect(validateAgentId("_onboarding").valid).toBe(false);
  });

  it("rejects too long", () => {
    expect(validateAgentId("a".repeat(31)).valid).toBe(false);
  });

  it("rejects trailing hyphen", () => {
    expect(validateAgentId("test-").valid).toBe(false);
  });
});

describe("scaffoldAgent", () => {
  let fixture: ReturnType<typeof setupFixture>;

  beforeEach(() => { fixture = setupFixture(); });
  afterEach(() => { rmSync(TEST_ROOT, { recursive: true, force: true }); });

  it("creates workspace with all expected files", () => {
    const result = scaffoldAgent({
      id: "atlas",
      name: "Atlas",
      emoji: "🗺️",
      ...fixture,
    });

    expect(result.workspace).toBe(join(fixture.nousDir, "atlas"));
    expect(existsSync(result.workspace)).toBe(true);
    expect(result.filesCreated).toContain("AGENTS.md");
    expect(result.filesCreated).toContain("SOUL.md");
    expect(result.filesCreated).toContain("IDENTITY.md");
    expect(result.filesCreated).toContain("USER.md");
    expect(result.filesCreated).toContain("MEMORY.md");
    expect(result.filesCreated.length).toBeGreaterThanOrEqual(9);
  });

  it("writes correct IDENTITY.md", () => {
    scaffoldAgent({ id: "atlas", name: "Atlas", emoji: "🗺️", ...fixture });
    const content = readFileSync(join(fixture.nousDir, "atlas", "IDENTITY.md"), "utf-8");
    expect(content).toContain("name: Atlas");
    expect(content).toContain("emoji: 🗺️");
  });

  it("writes onboarding SOUL.md, not template", () => {
    scaffoldAgent({ id: "atlas", name: "Atlas", ...fixture });
    const soul = readFileSync(join(fixture.nousDir, "atlas", "SOUL.md"), "utf-8");
    expect(soul).toContain("Onboarding");
    expect(soul).toContain("first conversation");
    expect(soul).toContain("Atlas");
    expect(soul).not.toContain("Example Soul");
  });

  it("updates config with agent entry and web binding", () => {
    scaffoldAgent({ id: "atlas", name: "Atlas", ...fixture });
    const config = JSON.parse(readFileSync(fixture.configPath, "utf-8"));
    const agent = config.agents.list.find((a: { id: string }) => a.id === "atlas");
    expect(agent).toBeDefined();
    expect(agent.name).toBe("Atlas");
    expect(agent.identity.emoji).toBe("🤖");
    const binding = config.bindings.find((b: { agentId: string }) => b.agentId === "atlas");
    expect(binding).toBeDefined();
    expect(binding.match.channel).toBe("web");
  });

  it("rejects duplicate agent ID", () => {
    expect(() => scaffoldAgent({ id: "syn", name: "Syn Dupe", ...fixture }))
      .toThrow("already exists in config");
  });

  it("rejects if workspace already exists", () => {
    mkdirSync(join(fixture.nousDir, "taken"), { recursive: true });
    expect(() => scaffoldAgent({ id: "taken", name: "Taken", ...fixture }))
      .toThrow("Workspace already exists");
  });

  it("uses default emoji when none provided", () => {
    scaffoldAgent({ id: "helper", name: "Helper", ...fixture });
    const identity = readFileSync(join(fixture.nousDir, "helper", "IDENTITY.md"), "utf-8");
    expect(identity).toContain("emoji: 🤖");
  });

  it("rejects invalid ID", () => {
    expect(() => scaffoldAgent({ id: "Bad Name", name: "Bad", ...fixture }))
      .toThrow("Invalid agent ID");
  });
});
