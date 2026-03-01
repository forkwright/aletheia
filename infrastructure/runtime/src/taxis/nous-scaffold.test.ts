import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mergeGitignore, scaffoldAgentWorkspaceDirs, scaffoldNousShared } from "./nous-scaffold.js";

let tmpDir: string;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "nous-scaffold-"));
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("scaffoldNousShared", () => {
  it("creates four subdirectories under workspace/", () => {
    scaffoldNousShared(tmpDir);
    expect(existsSync(join(tmpDir, "workspace", "plans"))).toBe(true);
    expect(existsSync(join(tmpDir, "workspace", "specs"))).toBe(true);
    expect(existsSync(join(tmpDir, "workspace", "standards"))).toBe(true);
    expect(existsSync(join(tmpDir, "workspace", "references"))).toBe(true);
  });

  it("returns created paths as segment strings", () => {
    const created = scaffoldNousShared(tmpDir);
    expect(created).toEqual([
      "workspace/plans",
      "workspace/specs",
      "workspace/standards",
      "workspace/references",
    ]);
  });

  it("returns empty array when all dirs already exist (idempotent)", () => {
    scaffoldNousShared(tmpDir);
    const second = scaffoldNousShared(tmpDir);
    expect(second).toEqual([]);
  });
});

describe("mergeGitignore", () => {
  const MANAGED_BLOCK_SNIPPET = "# BEGIN aletheia-managed";

  it("writes managed block to non-existent .gitignore", () => {
    mergeGitignore(tmpDir);
    const content = readFileSync(join(tmpDir, ".gitignore"), "utf-8");
    expect(content).toContain(MANAGED_BLOCK_SNIPPET);
    expect(content).toContain("# END aletheia-managed");
    expect(content).toContain("*/workspace/plans/");
  });

  it("appends managed block to file with content but no managed block", () => {
    writeFileSync(join(tmpDir, ".gitignore"), "node_modules/\ndist/\n", "utf-8");
    mergeGitignore(tmpDir);
    const content = readFileSync(join(tmpDir, ".gitignore"), "utf-8");
    expect(content).toContain("node_modules/");
    expect(content).toContain("dist/");
    expect(content).toContain(MANAGED_BLOCK_SNIPPET);
  });

  it("replaces managed block in file that already has one", () => {
    const initial = "# BEGIN aletheia-managed\nOLD_ENTRY/\n# END aletheia-managed\n";
    writeFileSync(join(tmpDir, ".gitignore"), initial, "utf-8");
    mergeGitignore(tmpDir);
    const content = readFileSync(join(tmpDir, ".gitignore"), "utf-8");
    expect(content).not.toContain("OLD_ENTRY/");
    expect(content).toContain("*/workspace/plans/");
  });

  it("preserves content above and below managed block on replacement", () => {
    const fixture = [
      "# User-authored entry",
      "node_modules/",
      "",
      "# BEGIN aletheia-managed",
      "OLD_ENTRY/",
      "# END aletheia-managed",
      "",
      "# More user content",
      "dist/",
      "",
    ].join("\n");
    writeFileSync(join(tmpDir, ".gitignore"), fixture, "utf-8");
    mergeGitignore(tmpDir);
    const content = readFileSync(join(tmpDir, ".gitignore"), "utf-8");
    expect(content).toContain("# User-authored entry");
    expect(content).toContain("node_modules/");
    expect(content).toContain("# More user content");
    expect(content).toContain("dist/");
    expect(content).not.toContain("OLD_ENTRY/");
    expect(content).toContain("*/workspace/plans/");
  });

  it("is idempotent — second call produces identical file content", () => {
    writeFileSync(join(tmpDir, ".gitignore"), "node_modules/\n", "utf-8");
    mergeGitignore(tmpDir);
    const first = readFileSync(join(tmpDir, ".gitignore"), "utf-8");
    mergeGitignore(tmpDir);
    const second = readFileSync(join(tmpDir, ".gitignore"), "utf-8");
    expect(second).toBe(first);
  });
});

describe("scaffoldAgentWorkspaceDirs", () => {
  it("creates scripts, drafts, data under workspace/", () => {
    scaffoldAgentWorkspaceDirs(tmpDir);
    expect(existsSync(join(tmpDir, "workspace", "scripts"))).toBe(true);
    expect(existsSync(join(tmpDir, "workspace", "drafts"))).toBe(true);
    expect(existsSync(join(tmpDir, "workspace", "data"))).toBe(true);
  });

  it("is idempotent — safe to call twice", () => {
    scaffoldAgentWorkspaceDirs(tmpDir);
    expect(() => scaffoldAgentWorkspaceDirs(tmpDir)).not.toThrow();
    expect(existsSync(join(tmpDir, "workspace", "scripts"))).toBe(true);
  });
});
