// Bootstrap diff detection tests
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { detectBootstrapDiff, logBootstrapDiff } from "./bootstrap-diff.js";
import { mkdtempSync, rmSync, mkdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

let tmpDir: string;
let workspace: string;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "bootstrap-diff-"));
  // Create workspace structure: tmpDir/nous/syn (workspace must have ../../shared)
  workspace = join(tmpDir, "nous", "syn");
  mkdirSync(workspace, { recursive: true });
  mkdirSync(join(tmpDir, "shared", "status"), { recursive: true });
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("detectBootstrapDiff", () => {
  it("returns null on first call (no previous data)", () => {
    const diff = detectBootstrapDiff("syn", { "SOUL.md": "abc123" }, workspace);
    expect(diff).toBeNull();
  });

  it("detects changed files on second call", () => {
    detectBootstrapDiff("syn", { "SOUL.md": "abc123", "USER.md": "def456" }, workspace);
    const diff = detectBootstrapDiff("syn", { "SOUL.md": "abc123", "USER.md": "changed" }, workspace);
    expect(diff).not.toBeNull();
    expect(diff!.changed).toContain("USER.md");
  });

  it("detects added files", () => {
    detectBootstrapDiff("syn", { "SOUL.md": "abc" }, workspace);
    const diff = detectBootstrapDiff("syn", { "SOUL.md": "abc", "NEW.md": "xyz" }, workspace);
    expect(diff).not.toBeNull();
    expect(diff!.added).toContain("NEW.md");
  });

  it("detects removed files", () => {
    detectBootstrapDiff("syn", { "SOUL.md": "abc", "OLD.md": "xyz" }, workspace);
    const diff = detectBootstrapDiff("syn", { "SOUL.md": "abc" }, workspace);
    expect(diff).not.toBeNull();
    expect(diff!.removed).toContain("OLD.md");
  });

  it("returns null when no changes", () => {
    const hashes = { "SOUL.md": "abc", "USER.md": "def" };
    detectBootstrapDiff("syn", hashes, workspace);
    const diff = detectBootstrapDiff("syn", hashes, workspace);
    expect(diff).toBeNull();
  });
});

describe("logBootstrapDiff", () => {
  it("writes to bootstrap-changes.jsonl", () => {
    const diff = {
      timestamp: new Date().toISOString(),
      nousId: "syn",
      added: ["NEW.md"],
      removed: [],
      changed: ["SOUL.md"],
    };
    logBootstrapDiff(diff, workspace);
    const logFile = join(tmpDir, "shared", "status", "bootstrap-changes.jsonl");
    expect(existsSync(logFile)).toBe(true);
  });
});
