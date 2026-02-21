import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { loadCustomCommands, parseFrontmatter, substituteArgs } from "./custom-commands.js";

describe("parseFrontmatter", () => {
  it("parses valid frontmatter", () => {
    const content = `---
name: deploy
description: Deploy a service
arguments:
  - name: service
    required: true
  - name: branch
    default: main
allowed_tools: [exec, read]
---

Deploy \`$service\` from \`$branch\`.`;

    const { frontmatter, body } = parseFrontmatter(content);
    expect(frontmatter).not.toBeNull();
    expect(frontmatter!.name).toBe("deploy");
    expect(frontmatter!.description).toBe("Deploy a service");
    expect(frontmatter!.arguments).toHaveLength(2);
    expect(frontmatter!.arguments![0]!.name).toBe("service");
    expect(frontmatter!.arguments![0]!.required).toBe(true);
    expect(frontmatter!.arguments![1]!.name).toBe("branch");
    expect(frontmatter!.arguments![1]!.default).toBe("main");
    expect(frontmatter!.allowed_tools).toEqual(["exec", "read"]);
    expect(body).toContain("Deploy `$service`");
  });

  it("returns null frontmatter for no markers", () => {
    const { frontmatter, body } = parseFrontmatter("just plain text");
    expect(frontmatter).toBeNull();
    expect(body).toBe("just plain text");
  });

  it("returns null frontmatter for unclosed markers", () => {
    const { frontmatter } = parseFrontmatter("---\nname: test\nno closing marker");
    expect(frontmatter).toBeNull();
  });

  it("handles frontmatter without arguments", () => {
    const { frontmatter } = parseFrontmatter("---\nname: ping\ndescription: Simple ping\n---\nPong!");
    expect(frontmatter!.name).toBe("ping");
    expect(frontmatter!.arguments).toBeUndefined();
  });
});

describe("substituteArgs", () => {
  it("substitutes required args", () => {
    const result = substituteArgs(
      "Deploy $service from $branch",
      [{ name: "service", required: true }, { name: "branch", required: true }],
      "api main",
    );
    expect(result.prompt).toBe("Deploy api from main");
    expect(result.error).toBeUndefined();
  });

  it("applies defaults for missing optional args", () => {
    const result = substituteArgs(
      "Deploy $service from $branch",
      [{ name: "service", required: true }, { name: "branch", default: "main" }],
      "api",
    );
    expect(result.prompt).toBe("Deploy api from main");
  });

  it("returns error for missing required args", () => {
    const result = substituteArgs(
      "Deploy $service",
      [{ name: "service", required: true }],
      "",
    );
    expect(result.error).toContain("Missing required argument: service");
  });

  it("handles no arguments", () => {
    const result = substituteArgs("Just run this", [], "");
    expect(result.prompt).toBe("Just run this");
  });
});

describe("loadCustomCommands", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = join(tmpdir(), `custom-cmds-test-${Date.now()}`);
    mkdirSync(tmpDir, { recursive: true });
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("returns empty array for missing directory", () => {
    const cmds = loadCustomCommands("/nonexistent");
    expect(cmds).toEqual([]);
  });

  it("loads .md files with valid frontmatter", () => {
    writeFileSync(join(tmpDir, "test.md"), "---\nname: test\ndescription: A test command\n---\nDo the test.");
    const cmds = loadCustomCommands(tmpDir);
    expect(cmds).toHaveLength(1);
    expect(cmds[0]!.name).toBe("test");
    expect(cmds[0]!.description).toBe("A test command");
    expect(cmds[0]!.prompt).toBe("Do the test.");
  });

  it("skips files without frontmatter", () => {
    writeFileSync(join(tmpDir, "no-fm.md"), "Just a plain markdown file.");
    const cmds = loadCustomCommands(tmpDir);
    expect(cmds).toHaveLength(0);
  });

  it("skips files missing name or description", () => {
    writeFileSync(join(tmpDir, "no-desc.md"), "---\nname: broken\n---\nNo description.");
    const cmds = loadCustomCommands(tmpDir);
    expect(cmds).toHaveLength(0);
  });

  it("skips non-.md files", () => {
    writeFileSync(join(tmpDir, "readme.txt"), "---\nname: txt\ndescription: Not md\n---\nBody.");
    const cmds = loadCustomCommands(tmpDir);
    expect(cmds).toHaveLength(0);
  });
});
