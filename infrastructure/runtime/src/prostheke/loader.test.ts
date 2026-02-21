// Plugin loader tests
import { describe, expect, it, vi, afterAll, beforeEach } from "vitest";
import { mkdirSync, rmSync, writeFileSync, symlinkSync } from "node:fs";
import { join } from "node:path";

vi.mock("../koina/logger.js", () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

import { discoverPlugins, validatePluginPath } from "./loader.js";

const TEST_ROOT = "/tmp/aletheia-plugin-loader-test";

function makePlugin(dir: string, id: string, version = "1.0.0") {
  const pluginDir = join(dir, id);
  mkdirSync(pluginDir, { recursive: true });
  writeFileSync(
    join(pluginDir, "manifest.json"),
    JSON.stringify({ id, name: id, version }),
  );
}

describe("validatePluginPath", () => {
  beforeEach(() => {
    rmSync(TEST_ROOT, { recursive: true, force: true });
    mkdirSync(TEST_ROOT, { recursive: true });
  });

  afterAll(() => {
    rmSync(TEST_ROOT, { recursive: true, force: true });
  });

  it("accepts path within root", () => {
    const pluginDir = join(TEST_ROOT, "my-plugin");
    mkdirSync(pluginDir);
    expect(validatePluginPath(pluginDir, TEST_ROOT)).toBe(true);
  });

  it("rejects path outside root", () => {
    expect(validatePluginPath("/tmp/somewhere-else", TEST_ROOT)).toBe(false);
  });

  it("rejects traversal via ..", () => {
    const malicious = join(TEST_ROOT, "legit", "..", "..", "etc");
    expect(validatePluginPath(malicious, TEST_ROOT)).toBe(false);
  });

  it("rejects symlink escaping root", () => {
    const linkPath = join(TEST_ROOT, "escape-link");
    symlinkSync("/tmp", linkPath);
    expect(validatePluginPath(linkPath, TEST_ROOT)).toBe(false);
  });
});

describe("discoverPlugins", () => {
  beforeEach(() => {
    rmSync(TEST_ROOT, { recursive: true, force: true });
    mkdirSync(TEST_ROOT, { recursive: true });
  });

  afterAll(() => {
    rmSync(TEST_ROOT, { recursive: true, force: true });
  });

  it("returns empty array for missing directory", async () => {
    const result = await discoverPlugins("/tmp/nonexistent-plugin-dir-xxx");
    expect(result).toEqual([]);
  });

  it("returns empty array for empty directory", async () => {
    const result = await discoverPlugins(TEST_ROOT);
    expect(result).toEqual([]);
  });

  it("discovers plugins with manifests", async () => {
    makePlugin(TEST_ROOT, "plugin-a");
    makePlugin(TEST_ROOT, "plugin-b", "2.0.0");

    const result = await discoverPlugins(TEST_ROOT);
    expect(result).toHaveLength(2);

    const ids = result.map((p) => p.manifest.id).sort();
    expect(ids).toEqual(["plugin-a", "plugin-b"]);
  });

  it("skips directories starting with _ or .", async () => {
    makePlugin(TEST_ROOT, "_hidden");
    makePlugin(TEST_ROOT, ".dotdir");
    makePlugin(TEST_ROOT, "visible");

    const result = await discoverPlugins(TEST_ROOT);
    expect(result).toHaveLength(1);
    expect(result[0]!.manifest.id).toBe("visible");
  });

  it("skips files (non-directories)", async () => {
    writeFileSync(join(TEST_ROOT, "not-a-dir.json"), "{}");
    makePlugin(TEST_ROOT, "real-plugin");

    const result = await discoverPlugins(TEST_ROOT);
    expect(result).toHaveLength(1);
  });

  it("skips directories without manifests", async () => {
    mkdirSync(join(TEST_ROOT, "no-manifest"), { recursive: true });
    writeFileSync(join(TEST_ROOT, "no-manifest", "README.md"), "# No manifest");

    const result = await discoverPlugins(TEST_ROOT);
    expect(result).toHaveLength(0);
  });
});
