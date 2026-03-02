import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync, existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

// These must be hoisted-safe: vi.mock factories can't reference outer variables
vi.mock("node:os", async () => {
  const actual = await vi.importActual<typeof import("node:os")>("node:os");
  return {
    ...actual,
    homedir: () => (globalThis as Record<string, string>).__TEST_HOME ?? actual.homedir(),
  };
});

vi.mock("./paths.js", () => ({
  paths: {
    get root() { return (globalThis as Record<string, string>).__TEST_ROOT ?? "/tmp/fallback"; },
  },
}));

import { migrateLegacyPaths } from "./migrate-legacy.js";

describe("migrateLegacyPaths", () => {
  let testRoot: string;
  let testHome: string;

  beforeEach(() => {
    testRoot = mkdtempSync(join(tmpdir(), "migrate-root-"));
    testHome = mkdtempSync(join(tmpdir(), "migrate-home-"));
    (globalThis as Record<string, string>).__TEST_ROOT = testRoot;
    (globalThis as Record<string, string>).__TEST_HOME = testHome;
  });

  afterEach(() => {
    try { rmSync(testRoot, { recursive: true, force: true }); } catch {}
    try { rmSync(testHome, { recursive: true, force: true }); } catch {}
    delete (globalThis as Record<string, string>).__TEST_ROOT;
    delete (globalThis as Record<string, string>).__TEST_HOME;
  });

  it("no-ops when ~/.aletheia does not exist", () => {
    expect(() => migrateLegacyPaths()).not.toThrow();
  });

  it("copies .setup-complete to config/", () => {
    mkdirSync(join(testHome, ".aletheia"), { recursive: true });
    writeFileSync(join(testHome, ".aletheia", ".setup-complete"), "1");

    migrateLegacyPaths();

    expect(existsSync(join(testRoot, "config", ".setup-complete"))).toBe(true);
    expect(readFileSync(join(testRoot, "config", ".setup-complete"), "utf-8")).toBe("1");
  });

  it("copies session.key to config/", () => {
    mkdirSync(join(testHome, ".aletheia"), { recursive: true });
    writeFileSync(join(testHome, ".aletheia", "session.key"), "secret-key");

    migrateLegacyPaths();

    expect(readFileSync(join(testRoot, "config", "session.key"), "utf-8")).toBe("secret-key");
  });

  it("copies credentials directory", () => {
    mkdirSync(join(testHome, ".aletheia", "credentials"), { recursive: true });
    writeFileSync(join(testHome, ".aletheia", "credentials", "anthropic.json"), '{"key":"sk-test"}');

    migrateLegacyPaths();

    expect(readFileSync(join(testRoot, "config", "credentials", "anthropic.json"), "utf-8")).toBe('{"key":"sk-test"}');
  });

  it("does not overwrite existing files", () => {
    mkdirSync(join(testHome, ".aletheia"), { recursive: true });
    writeFileSync(join(testHome, ".aletheia", "session.key"), "old");

    mkdirSync(join(testRoot, "config"), { recursive: true });
    writeFileSync(join(testRoot, "config", "session.key"), "new");

    migrateLegacyPaths();

    expect(readFileSync(join(testRoot, "config", "session.key"), "utf-8")).toBe("new");
  });

  it("copies sessions.db to data/", () => {
    mkdirSync(join(testHome, ".aletheia"), { recursive: true });
    writeFileSync(join(testHome, ".aletheia", "sessions.db"), "db-content");

    migrateLegacyPaths();

    expect(readFileSync(join(testRoot, "data", "sessions.db"), "utf-8")).toBe("db-content");
  });
});
