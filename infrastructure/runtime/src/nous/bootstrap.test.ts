// Bootstrap assembly tests
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { assembleBootstrap } from "./bootstrap.js";
import { mkdtempSync, writeFileSync, rmSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

let workspace: string;

beforeEach(() => {
  workspace = mkdtempSync(join(tmpdir(), "bootstrap-"));
});

afterEach(() => {
  rmSync(workspace, { recursive: true, force: true });
});

function writeWsFile(name: string, content: string) {
  writeFileSync(join(workspace, name), content);
}

describe("assembleBootstrap", () => {
  it("returns empty result for empty workspace", () => {
    const result = assembleBootstrap(workspace);
    expect(result.fileCount).toBe(0);
    expect(result.totalTokens).toBe(0);
    expect(result.staticBlocks).toHaveLength(0);
    expect(result.dynamicBlocks).toHaveLength(0);
  });

  it("loads SOUL.md as static block", () => {
    writeWsFile("SOUL.md", "You are Syn, the orchestrator.");
    const result = assembleBootstrap(workspace);
    expect(result.fileCount).toBe(1);
    expect(result.staticBlocks).toHaveLength(1);
    expect(result.staticBlocks[0]!.text).toContain("You are Syn");
    expect(result.staticBlocks[0]!.cache_control).toEqual({ type: "ephemeral" });
  });

  it("groups static files together", () => {
    writeWsFile("SOUL.md", "Soul content");
    writeWsFile("USER.md", "User preferences");
    writeWsFile("AGENTS.md", "Agent descriptions");
    const result = assembleBootstrap(workspace);
    expect(result.staticBlocks).toHaveLength(1);
    expect(result.staticBlocks[0]!.text).toContain("Soul content");
    expect(result.staticBlocks[0]!.text).toContain("User preferences");
    expect(result.staticBlocks[0]!.text).toContain("Agent descriptions");
  });

  it("creates semi-static block for TOOLS.md and MEMORY.md", () => {
    writeWsFile("TOOLS.md", "Available tools");
    writeWsFile("MEMORY.md", "Memory context");
    const result = assembleBootstrap(workspace);
    expect(result.staticBlocks).toHaveLength(1);
    expect(result.staticBlocks[0]!.text).toContain("Available tools");
    expect(result.staticBlocks[0]!.text).toContain("Memory context");
  });

  it("creates dynamic blocks for PROSOCHE.md and CONTEXT.md", () => {
    writeWsFile("PROSOCHE.md", "Attention data");
    writeWsFile("CONTEXT.md", "Current context");
    const result = assembleBootstrap(workspace);
    expect(result.dynamicBlocks).toHaveLength(2);
    expect(result.dynamicBlocks[0]!.text).toContain("Attention data");
    expect(result.dynamicBlocks[1]!.text).toContain("Current context");
    expect(result.dynamicBlocks[0]!.cache_control).toBeUndefined();
  });

  it("computes content hash", () => {
    writeWsFile("SOUL.md", "content");
    const result = assembleBootstrap(workspace);
    expect(result.contentHash).toMatch(/^[0-9a-f]{32}$/);
    expect(result.fileHashes["SOUL.md"]).toMatch(/^[0-9a-f]{16}$/);
  });

  it("hash changes when content changes", () => {
    writeWsFile("SOUL.md", "version 1");
    const r1 = assembleBootstrap(workspace);
    writeWsFile("SOUL.md", "version 2");
    const r2 = assembleBootstrap(workspace);
    expect(r1.contentHash).not.toBe(r2.contentHash);
  });

  it("respects maxTokens budget", () => {
    writeWsFile("SOUL.md", "x".repeat(50000));
    writeWsFile("CONTEXT.md", "y".repeat(50000));
    const result = assembleBootstrap(workspace, { maxTokens: 5000 });
    expect(result.totalTokens).toBeLessThanOrEqual(5500); // some margin for truncation
  });

  it("drops lowest-priority files when budget exceeded", () => {
    writeWsFile("SOUL.md", "x".repeat(100000));
    writeWsFile("CONTEXT.md", "y".repeat(100000));
    const result = assembleBootstrap(workspace, { maxTokens: 15000 });
    expect(result.droppedFiles.length).toBeGreaterThan(0);
  });

  it("includes skillsSection in semi-static block", () => {
    writeWsFile("TOOLS.md", "Tools here");
    const result = assembleBootstrap(workspace, { skillsSection: "## Skills\n- skill1" });
    const semiStatic = result.staticBlocks.find((b) => b.text.includes("Skills"));
    expect(semiStatic).toBeDefined();
    expect(semiStatic!.text).toContain("skill1");
  });

  it("injects degradation guidance for down services", () => {
    writeWsFile("SOUL.md", "Identity");
    const result = assembleBootstrap(workspace, {
      degradedServices: ["neo4j", "qdrant"],
    });
    const infraBlock = result.dynamicBlocks.find((b) => b.text.includes("Infrastructure Status"));
    expect(infraBlock).toBeDefined();
    expect(infraBlock!.text).toContain("neo4j");
    expect(infraBlock!.text).toContain("qdrant");
  });

  it("handles extraFiles", () => {
    writeWsFile("CUSTOM.md", "Custom content");
    const result = assembleBootstrap(workspace, { extraFiles: ["CUSTOM.md"] });
    expect(result.fileCount).toBe(1);
    expect(result.dynamicBlocks.some((b) => b.text.includes("Custom content"))).toBe(true);
  });

  it("skips empty files", () => {
    writeWsFile("SOUL.md", "");
    const result = assembleBootstrap(workspace);
    expect(result.fileCount).toBe(0);
  });

  it("tracks totalTokens", () => {
    writeWsFile("SOUL.md", "Hello world this is a test");
    const result = assembleBootstrap(workspace);
    expect(result.totalTokens).toBeGreaterThan(0);
  });
});
