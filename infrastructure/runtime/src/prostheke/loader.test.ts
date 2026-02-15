// Plugin loader tests
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { loadPlugins } from "./loader.js";

let tmpDir: string;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "loader-"));
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("loadPlugins", () => {
  it("returns empty array for empty paths", async () => {
    const plugins = await loadPlugins([]);
    expect(plugins).toEqual([]);
  });

  it("warns and skips non-existent path", async () => {
    const plugins = await loadPlugins(["/nonexistent/path"]);
    expect(plugins).toEqual([]);
  });

  it("loads manifest-only plugin", async () => {
    const pluginDir = join(tmpDir, "test-plugin");
    mkdirSync(pluginDir);
    writeFileSync(join(pluginDir, "manifest.json"), JSON.stringify({
      id: "test-plugin",
      name: "Test Plugin",
      version: "1.0.0",
    }));

    const plugins = await loadPlugins([pluginDir]);
    expect(plugins).toHaveLength(1);
    expect(plugins[0]!.manifest.id).toBe("test-plugin");
    expect(plugins[0]!.manifest.name).toBe("Test Plugin");
  });

  it("finds *.plugin.json manifest", async () => {
    const pluginDir = join(tmpDir, "my-plugin");
    mkdirSync(pluginDir);
    writeFileSync(join(pluginDir, "aletheia.plugin.json"), JSON.stringify({
      id: "my-plugin",
      name: "My Plugin",
      version: "0.1.0",
    }));

    const plugins = await loadPlugins([pluginDir]);
    expect(plugins).toHaveLength(1);
    expect(plugins[0]!.manifest.id).toBe("my-plugin");
  });

  it("skips plugin with no manifest", async () => {
    const pluginDir = join(tmpDir, "no-manifest");
    mkdirSync(pluginDir);
    writeFileSync(join(pluginDir, "readme.md"), "# No manifest");

    const plugins = await loadPlugins([pluginDir]);
    expect(plugins).toEqual([]);
  });

  it("skips plugin with invalid manifest", async () => {
    const pluginDir = join(tmpDir, "bad-manifest");
    mkdirSync(pluginDir);
    writeFileSync(join(pluginDir, "manifest.json"), "not json");

    const plugins = await loadPlugins([pluginDir]);
    expect(plugins).toEqual([]);
  });

  it("skips plugin with missing required fields", async () => {
    const pluginDir = join(tmpDir, "incomplete");
    mkdirSync(pluginDir);
    writeFileSync(join(pluginDir, "manifest.json"), JSON.stringify({
      id: "incomplete",
      // missing name and version
    }));

    const plugins = await loadPlugins([pluginDir]);
    expect(plugins).toEqual([]);
  });

  it("loads plugin with code entry", async () => {
    const pluginDir = join(tmpDir, "code-plugin");
    mkdirSync(pluginDir);
    writeFileSync(join(pluginDir, "manifest.json"), JSON.stringify({
      id: "code-plugin",
      name: "Code Plugin",
      version: "1.0.0",
    }));
    writeFileSync(join(pluginDir, "index.js"), `
      exports.hooks = { afterTurn: async () => {} };
      exports.tools = [];
    `);

    const plugins = await loadPlugins([pluginDir]);
    expect(plugins).toHaveLength(1);
    expect(plugins[0]!.hooks).toBeDefined();
  });

  it("loads multiple plugins", async () => {
    for (const name of ["p1", "p2"]) {
      const dir = join(tmpDir, name);
      mkdirSync(dir);
      writeFileSync(join(dir, "manifest.json"), JSON.stringify({
        id: name,
        name: `Plugin ${name}`,
        version: "1.0.0",
      }));
    }

    const plugins = await loadPlugins([join(tmpDir, "p1"), join(tmpDir, "p2")]);
    expect(plugins).toHaveLength(2);
  });
});
