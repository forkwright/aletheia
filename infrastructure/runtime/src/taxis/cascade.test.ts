import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { cascadeDiscover, cascadeResolve, cascadeResolveAll } from "./cascade.js";

// We need to override paths for testing — mock the paths module
import { vi } from "vitest";

let testRoot: string;
let mockPaths: { nousDir: (id: string) => string; shared: string; theke: string };

vi.mock("./paths.js", () => ({
  get paths() {
    return {
      nousDir: (id: string) => join(testRoot, "nous", id),
      shared: join(testRoot, "shared"),
      theke: join(testRoot, "theke"),
    };
  },
}));

function mkfile(relPath: string, content = "") {
  const full = join(testRoot, relPath);
  mkdirSync(join(full, ".."), { recursive: true });
  writeFileSync(full, content || `content of ${relPath}`);
}

beforeEach(() => {
  testRoot = join(tmpdir(), `cascade-test-${Date.now()}-${Math.random().toString(36).slice(2)}`);
  mkdirSync(testRoot, { recursive: true });
});

afterEach(() => {
  rmSync(testRoot, { recursive: true, force: true });
});

describe("cascadeDiscover", () => {
  it("discovers files from all three tiers", () => {
    mkfile("nous/syn/tools/agent-only.md");
    mkfile("shared/tools/shared-tool.md");
    mkfile("theke/tools/theke-tool.md");

    const results = cascadeDiscover("syn", "tools", ".md");
    expect(results).toHaveLength(3);
    expect(results.map(r => r.name).sort()).toEqual(["agent-only.md", "shared-tool.md", "theke-tool.md"]);
    expect(results.find(r => r.name === "agent-only.md")?.tier).toBe("nous");
    expect(results.find(r => r.name === "shared-tool.md")?.tier).toBe("shared");
    expect(results.find(r => r.name === "theke-tool.md")?.tier).toBe("theke");
  });

  it("most-specific tier wins on name collision", () => {
    mkfile("nous/syn/tools/override.md", "agent version");
    mkfile("shared/tools/override.md", "shared version");
    mkfile("theke/tools/override.md", "theke version");

    const results = cascadeDiscover("syn", "tools", ".md");
    expect(results).toHaveLength(1);
    expect(results[0]!.tier).toBe("nous");
    expect(results[0]!.path).toContain("nous/syn/tools/override.md");
  });

  it("shared overrides theke but not nous", () => {
    mkfile("shared/hooks/common.md", "shared version");
    mkfile("theke/hooks/common.md", "theke version");

    const results = cascadeDiscover("syn", "hooks", ".md");
    expect(results).toHaveLength(1);
    expect(results[0]!.tier).toBe("shared");
  });

  it("filters by extension", () => {
    mkfile("shared/tools/tool.md");
    mkfile("shared/tools/tool.yaml");
    mkfile("shared/tools/tool.txt");

    const mdResults = cascadeDiscover("syn", "tools", ".md");
    expect(mdResults).toHaveLength(1);
    expect(mdResults[0]!.name).toBe("tool.md");

    const yamlResults = cascadeDiscover("syn", "tools", ".yaml");
    expect(yamlResults).toHaveLength(1);
    expect(yamlResults[0]!.name).toBe("tool.yaml");
  });

  it("returns all files when no extension filter", () => {
    mkfile("shared/tools/tool.md");
    mkfile("shared/tools/tool.yaml");

    const results = cascadeDiscover("syn", "tools");
    expect(results).toHaveLength(2);
  });

  it("returns empty array for missing directories", () => {
    const results = cascadeDiscover("syn", "nonexistent");
    expect(results).toEqual([]);
  });

  it("skips hidden files", () => {
    mkfile("shared/tools/.hidden.md");
    mkfile("shared/tools/visible.md");

    const results = cascadeDiscover("syn", "tools", ".md");
    expect(results).toHaveLength(1);
    expect(results[0]!.name).toBe("visible.md");
  });

  it("handles different agents independently", () => {
    mkfile("nous/syn/tools/syn-tool.md");
    mkfile("nous/demiurge/tools/demi-tool.md");
    mkfile("shared/tools/common.md");

    const synResults = cascadeDiscover("syn", "tools", ".md");
    expect(synResults.map(r => r.name).sort()).toEqual(["common.md", "syn-tool.md"]);

    const demiResults = cascadeDiscover("demiurge", "tools", ".md");
    expect(demiResults.map(r => r.name).sort()).toEqual(["common.md", "demi-tool.md"]);
  });
});

describe("cascadeResolve", () => {
  it("returns most-specific path", () => {
    mkfile("nous/syn/USER.md", "agent copy");
    mkfile("theke/USER.md", "canonical");

    const result = cascadeResolve("syn", "USER.md");
    expect(result).toContain("nous/syn/USER.md");
  });

  it("falls through to theke when not in nous or shared", () => {
    mkfile("theke/USER.md", "canonical");

    const result = cascadeResolve("syn", "USER.md");
    expect(result).toContain("theke/USER.md");
  });

  it("returns null when file not found anywhere", () => {
    const result = cascadeResolve("syn", "NONEXISTENT.md");
    expect(result).toBeNull();
  });

  it("resolves with subdirectory", () => {
    mkfile("shared/tools/my-tool.md");

    const result = cascadeResolve("syn", "my-tool.md", "tools");
    expect(result).toContain("shared/tools/my-tool.md");
  });
});

describe("cascadeResolveAll", () => {
  it("returns all instances ordered most-specific first", () => {
    mkfile("nous/syn/config.yaml", "nous");
    mkfile("shared/config.yaml", "shared");
    mkfile("theke/config.yaml", "theke");

    const results = cascadeResolveAll("syn", "config.yaml");
    expect(results).toHaveLength(3);
    expect(results[0]!.tier).toBe("nous");
    expect(results[1]!.tier).toBe("shared");
    expect(results[2]!.tier).toBe("theke");
  });

  it("returns only tiers where file exists", () => {
    mkfile("theke/USER.md");

    const results = cascadeResolveAll("syn", "USER.md");
    expect(results).toHaveLength(1);
    expect(results[0]!.tier).toBe("theke");
  });

  it("returns empty for nonexistent file", () => {
    const results = cascadeResolveAll("syn", "nope.md");
    expect(results).toEqual([]);
  });
});
