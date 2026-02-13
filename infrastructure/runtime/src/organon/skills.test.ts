// Skills registry tests
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { SkillRegistry } from "./skills.js";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

let tmpDir: string;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "skills-"));
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

function createSkill(name: string, content: string) {
  const dir = join(tmpDir, name);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "SKILL.md"), content);
}

describe("SkillRegistry", () => {
  it("loads skills from directory", () => {
    createSkill("distill", "# Distillation\n\nTrigger memory distillation.");
    const reg = new SkillRegistry();
    reg.loadFromDirectory(tmpDir);
    expect(reg.size).toBe(1);
    expect(reg.get("distill")).toBeDefined();
    expect(reg.get("distill")!.name).toBe("Distillation");
    expect(reg.get("distill")!.description).toBe("Trigger memory distillation.");
  });

  it("handles missing directory", () => {
    const reg = new SkillRegistry();
    reg.loadFromDirectory("/nonexistent/path");
    expect(reg.size).toBe(0);
  });

  it("skips directories without SKILL.md", () => {
    mkdirSync(join(tmpDir, "no-skill"), { recursive: true });
    const reg = new SkillRegistry();
    reg.loadFromDirectory(tmpDir);
    expect(reg.size).toBe(0);
  });

  it("skips SKILL.md without heading", () => {
    createSkill("bad", "No heading here, just text.");
    const reg = new SkillRegistry();
    reg.loadFromDirectory(tmpDir);
    expect(reg.size).toBe(0);
  });

  it("listAll returns all loaded skills", () => {
    createSkill("a", "# Alpha\n\nFirst skill.");
    createSkill("b", "# Beta\n\nSecond skill.");
    const reg = new SkillRegistry();
    reg.loadFromDirectory(tmpDir);
    expect(reg.listAll()).toHaveLength(2);
  });

  it("toBootstrapSection formats for system prompt", () => {
    createSkill("search", "# Memory Search\n\nSearch long-term memory.");
    const reg = new SkillRegistry();
    reg.loadFromDirectory(tmpDir);
    const section = reg.toBootstrapSection();
    expect(section).toContain("## Available Skills");
    expect(section).toContain("Memory Search");
    expect(section).toContain("Search long-term memory.");
  });

  it("toBootstrapSection returns empty for no skills", () => {
    const reg = new SkillRegistry();
    expect(reg.toBootstrapSection()).toBe("");
  });

  it("stores full instructions", () => {
    const content = "# Test Skill\n\nDescription here.\n\n## Steps\n\n1. Do X\n2. Do Y";
    createSkill("test", content);
    const reg = new SkillRegistry();
    reg.loadFromDirectory(tmpDir);
    expect(reg.get("test")!.instructions).toBe(content);
  });
});
