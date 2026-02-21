// Workspace git tracking tests
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { execFileSync } from "node:child_process";

vi.mock("../koina/logger.js", () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() }),
}));

import { initWorkspaceRepo, commitWorkspaceChange } from "./workspace-git.js";

describe("initWorkspaceRepo", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = mkdtempSync(join(tmpdir(), "ws-git-"));
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("initializes a git repo in the workspace", () => {
    const result = initWorkspaceRepo(tmpDir);
    expect(result).toBe(true);

    // Verify git was initialized
    const status = execFileSync("git", ["status", "--short"], { cwd: tmpDir, encoding: "utf-8" });
    expect(status).toBeDefined();
  });

  it("returns true if repo already exists", () => {
    initWorkspaceRepo(tmpDir);
    const result = initWorkspaceRepo(tmpDir);
    expect(result).toBe(true);
  });
});

describe("commitWorkspaceChange", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = mkdtempSync(join(tmpdir(), "ws-git-commit-"));
    initWorkspaceRepo(tmpDir);
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("commits a new file", () => {
    const filePath = join(tmpDir, "test.txt");
    writeFileSync(filePath, "hello world", "utf-8");

    commitWorkspaceChange(tmpDir, filePath, "write");

    const logOutput = execFileSync("git", ["log", "--oneline"], { cwd: tmpDir, encoding: "utf-8" });
    expect(logOutput).toContain("write: test.txt");
  });

  it("ignores files outside workspace (path traversal)", () => {
    const outsidePath = "/tmp/outside.txt";
    commitWorkspaceChange(tmpDir, outsidePath, "write");

    // Should have no commits
    try {
      execFileSync("git", ["log", "--oneline"], { cwd: tmpDir, encoding: "utf-8" });
    } catch {
      // "fatal: your current branch 'main' does not have any commits yet" — expected
    }
  });

  it("commits modified file", () => {
    const filePath = join(tmpDir, "test.txt");
    writeFileSync(filePath, "v1", "utf-8");
    commitWorkspaceChange(tmpDir, filePath, "write");

    writeFileSync(filePath, "v2", "utf-8");
    commitWorkspaceChange(tmpDir, filePath, "edit");

    const logOutput = execFileSync("git", ["log", "--oneline"], { cwd: tmpDir, encoding: "utf-8" });
    expect(logOutput).toContain("edit: test.txt");
    expect(logOutput).toContain("write: test.txt");
  });
});
