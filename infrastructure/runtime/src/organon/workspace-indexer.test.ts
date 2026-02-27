// Tests for workspace-indexer: gitignore filter, symlink skip, staleWarning
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { indexWorkspace, queryIndex, type WorkspaceIndex } from "./workspace-indexer.js";

let tmpDir: string;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "indx-test-"));
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("gitignore filtering", () => {
  it("excludes files matched by .gitignore", async () => {
    writeFileSync(join(tmpDir, ".gitignore"), "secret.txt\n");
    writeFileSync(join(tmpDir, "public.md"), "public content");
    writeFileSync(join(tmpDir, "secret.txt"), "private content");

    const index = await indexWorkspace(tmpDir);
    const paths = index.files.map((f) => f.path);
    expect(paths).toContain("public.md");
    expect(paths).not.toContain("secret.txt");
  });

  it("excludes directories matched by .gitignore", async () => {
    writeFileSync(join(tmpDir, ".gitignore"), "private/\n");
    mkdirSync(join(tmpDir, "private"));
    writeFileSync(join(tmpDir, "private", "data.md"), "private data");
    writeFileSync(join(tmpDir, "visible.md"), "visible");

    const index = await indexWorkspace(tmpDir);
    const paths = index.files.map((f) => f.path);
    expect(paths).toContain("visible.md");
    expect(paths).not.toContain(join("private", "data.md"));
  });

  it("indexes all files when no .gitignore exists", async () => {
    writeFileSync(join(tmpDir, "a.md"), "content a");
    writeFileSync(join(tmpDir, "b.md"), "content b");

    const index = await indexWorkspace(tmpDir);
    expect(index.files.length).toBe(2);
  });
});

describe("symlink protection", () => {
  it("does not traverse symlinked directories", async () => {
    const outsideDir = mkdtempSync(join(tmpdir(), "outside-"));
    writeFileSync(join(outsideDir, "outside.md"), "outside content");
    symlinkSync(outsideDir, join(tmpDir, "link-to-outside"));
    writeFileSync(join(tmpDir, "inside.md"), "inside content");

    const index = await indexWorkspace(tmpDir);
    const paths = index.files.map((f) => f.path);
    expect(paths).toContain("inside.md");
    expect(paths.some((p) => p.includes("outside"))).toBe(false);
    expect(paths.some((p) => p.includes("link-to-outside"))).toBe(false);

    rmSync(outsideDir, { recursive: true, force: true });
  });

  it("does not index symlinked files", async () => {
    const realFile = join(tmpDir, "real.md");
    writeFileSync(realFile, "real content");
    symlinkSync(realFile, join(tmpDir, "link.md"));

    const index = await indexWorkspace(tmpDir);
    const paths = index.files.map((f) => f.path);
    expect(paths).toContain("real.md");
    expect(paths).not.toContain("link.md");
  });
});

describe("staleWarning field", () => {
  it("returns staleWarning: false on freshly built index", async () => {
    writeFileSync(join(tmpDir, "note.md"), "content");
    const index = await indexWorkspace(tmpDir);
    expect(index.staleWarning).toBe(false);
  });

  it("WorkspaceIndex interface includes staleWarning", async () => {
    writeFileSync(join(tmpDir, "x.md"), "x");
    const index: WorkspaceIndex = await indexWorkspace(tmpDir);
    expect(typeof index.staleWarning).toBe("boolean");
  });
});

describe("queryIndex", () => {
  it("returns matching files ranked by score", async () => {
    writeFileSync(join(tmpDir, "auth.md"), "authentication token");
    writeFileSync(join(tmpDir, "notes.md"), "meeting notes");
    const index = await indexWorkspace(tmpDir);
    const results = queryIndex(index, "auth token", 5);
    expect(results[0].path).toBe("auth.md");
  });

  it("returns empty array for query with no matches", async () => {
    writeFileSync(join(tmpDir, "readme.md"), "readme content");
    const index = await indexWorkspace(tmpDir);
    const results = queryIndex(index, "zzzyyyxxx", 5);
    expect(results).toHaveLength(0);
  });
});
