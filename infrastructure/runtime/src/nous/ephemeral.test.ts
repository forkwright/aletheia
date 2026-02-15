// Ephemeral agent tests
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import {
  spawnEphemeral,
  recordEphemeralTurn,
  teardownEphemeral,
  getEphemeral,
  listEphemerals,
  harvestOutput,
} from "./ephemeral.js";
import { mkdtempSync, rmSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

let tmpDir: string;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "ephemeral-"));
  // Clear any leftover ephemerals from previous tests
  for (const eph of listEphemerals()) {
    teardownEphemeral(eph.id);
  }
});

afterEach(() => {
  for (const eph of listEphemerals()) {
    teardownEphemeral(eph.id);
  }
  rmSync(tmpDir, { recursive: true, force: true });
});

const baseSpec = {
  name: "test-agent",
  soul: "You are a test agent.",
  maxTurns: 5,
  maxDurationMs: 60_000,
};

describe("spawnEphemeral", () => {
  it("creates an ephemeral agent with workspace", () => {
    const eph = spawnEphemeral(baseSpec, tmpDir);
    expect(eph.id).toMatch(/^eph_/);
    expect(eph.spec.name).toBe("test-agent");
    expect(existsSync(eph.workspace)).toBe(true);
    expect(existsSync(join(eph.workspace, "SOUL.md"))).toBe(true);
  });

  it("enforces max concurrent limit (3)", () => {
    spawnEphemeral({ ...baseSpec, name: "a" }, tmpDir);
    spawnEphemeral({ ...baseSpec, name: "b" }, tmpDir);
    spawnEphemeral({ ...baseSpec, name: "c" }, tmpDir);
    expect(() => spawnEphemeral({ ...baseSpec, name: "d" }, tmpDir)).toThrow();
  });
});

describe("recordEphemeralTurn", () => {
  it("increments turn count and stores output", () => {
    const eph = spawnEphemeral(baseSpec, tmpDir);
    const ok = recordEphemeralTurn(eph.id, "response 1");
    expect(ok).toBe(true);
    expect(getEphemeral(eph.id)!.turnCount).toBe(1);
    expect(getEphemeral(eph.id)!.output).toContain("response 1");
  });

  it("returns false when turn limit reached", () => {
    const eph = spawnEphemeral({ ...baseSpec, maxTurns: 2 }, tmpDir);
    recordEphemeralTurn(eph.id, "r1");
    const ok = recordEphemeralTurn(eph.id, "r2");
    expect(ok).toBe(false);
  });
});

describe("teardownEphemeral", () => {
  it("removes agent and returns it", () => {
    const eph = spawnEphemeral(baseSpec, tmpDir);
    const removed = teardownEphemeral(eph.id);
    expect(removed).not.toBeNull();
    expect(removed!.id).toBe(eph.id);
    expect(getEphemeral(eph.id)).toBeUndefined();
  });

  it("returns null for unknown id", () => {
    expect(teardownEphemeral("eph_nonexistent")).toBeNull();
  });
});

describe("listEphemerals", () => {
  it("returns all active ephemerals", () => {
    spawnEphemeral({ ...baseSpec, name: "a" }, tmpDir);
    spawnEphemeral({ ...baseSpec, name: "b" }, tmpDir);
    expect(listEphemerals()).toHaveLength(2);
  });
});

describe("harvestOutput", () => {
  it("joins outputs with separator", () => {
    const eph = spawnEphemeral(baseSpec, tmpDir);
    recordEphemeralTurn(eph.id, "part1");
    recordEphemeralTurn(eph.id, "part2");
    const output = harvestOutput(eph.id);
    expect(output).toContain("part1");
    expect(output).toContain("part2");
  });
});
