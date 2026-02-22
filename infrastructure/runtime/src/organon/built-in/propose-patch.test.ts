// Tests for runtime code patching tools
import { describe, expect, it } from "vitest";
import { checkRateLimit, isPathAllowed, type PatchRecord } from "./propose-patch.js";

describe("propose-patch path validation", () => {
  it("allows paths in patchable directories", () => {
    expect(isPathAllowed("organon/built-in/note.ts").allowed).toBe(true);
    expect(isPathAllowed("nous/recall.ts").allowed).toBe(true);
    expect(isPathAllowed("distillation/extract.ts").allowed).toBe(true);
    expect(isPathAllowed("daemon/reflection-cron.ts").allowed).toBe(true);
  });

  it("rejects paths in forbidden directories", () => {
    expect(isPathAllowed("pylon/server.ts").allowed).toBe(false);
    expect(isPathAllowed("koina/logger.ts").allowed).toBe(false);
    expect(isPathAllowed("semeion/commands.ts").allowed).toBe(false);
    expect(isPathAllowed("taxis/schema.ts").allowed).toBe(false);
  });

  it("rejects paths not in any patchable directory", () => {
    expect(isPathAllowed("entry.ts").allowed).toBe(false);
    expect(isPathAllowed("aletheia.ts").allowed).toBe(false);
    expect(isPathAllowed("hermeneus/router.ts").allowed).toBe(false);
  });

  it("includes reason on rejection", () => {
    const result = isPathAllowed("pylon/server.ts");
    expect(result.reason).toContain("forbidden");
  });
});

describe("propose-patch rate limiting", () => {
  const basePatch: PatchRecord = {
    id: "patch-1",
    nousId: "syn",
    filePath: "nous/recall.ts",
    description: "test",
    oldText: "old",
    newText: "new",
    status: "applied",
    appliedAt: new Date().toISOString(),
    backupContent: "backup",
  };

  it("allows first patch", () => {
    expect(checkRateLimit({ patches: [] }, "syn").allowed).toBe(true);
  });

  it("blocks second patch within 1 hour for same agent", () => {
    const result = checkRateLimit({ patches: [basePatch] }, "syn");
    expect(result.allowed).toBe(false);
    expect(result.reason).toContain("1 patch per hour");
  });

  it("allows patch from different agent", () => {
    expect(checkRateLimit({ patches: [basePatch] }, "chiron").allowed).toBe(true);
  });

  it("blocks when daily limit reached", () => {
    const patches: PatchRecord[] = [
      { ...basePatch, id: "p1", nousId: "syn", appliedAt: new Date(Date.now() - 7200_000).toISOString() },
      { ...basePatch, id: "p2", nousId: "chiron", appliedAt: new Date(Date.now() - 3700_000).toISOString() },
      { ...basePatch, id: "p3", nousId: "syl", appliedAt: new Date(Date.now() - 3700_000).toISOString() },
    ];
    const result = checkRateLimit({ patches }, "akron");
    expect(result.allowed).toBe(false);
    expect(result.reason).toContain("patches per day");
  });

  it("allows patch after old ones expire", () => {
    const oldPatch = {
      ...basePatch,
      appliedAt: new Date(Date.now() - 86_500_000).toISOString(),
    };
    expect(checkRateLimit({ patches: [oldPatch] }, "syn").allowed).toBe(true);
  });
});
