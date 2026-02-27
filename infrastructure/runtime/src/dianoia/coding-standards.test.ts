// Tests for CodingStandards — layered coding standard system (ENG-15)
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, existsSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  getLanguageRules,
  buildStandards,
  writeStandardsFile,
  readStandardsFile,
  createUserPreferenceRule,
} from "./coding-standards.js";
import { ensureProjectDir, getProjectDir } from "./project-files.js";

function createTempWorkspace(): string {
  const dir = join(tmpdir(), `dianoia-standards-test-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

describe("CodingStandards", () => {
  let workspace: string;

  beforeEach(() => { workspace = createTempWorkspace(); });
  afterEach(() => { try { rmSync(workspace, { recursive: true, force: true }); } catch { /* */ } });

  describe("getLanguageRules", () => {
    it("returns TypeScript rules", () => {
      const rules = getLanguageRules("typescript");
      expect(rules.length).toBeGreaterThan(0);
      expect(rules.every(r => r.level === 1)).toBe(true);
      expect(rules.some(r => r.id.includes("TS"))).toBe(true);
    });

    it("returns Python rules", () => {
      const rules = getLanguageRules("python");
      expect(rules.length).toBeGreaterThan(0);
      expect(rules.some(r => r.id.includes("PY"))).toBe(true);
    });

    it("returns C# rules", () => {
      const rules = getLanguageRules("csharp");
      expect(rules.length).toBeGreaterThan(0);
      expect(rules.some(r => r.id.includes("CS"))).toBe(true);
    });

    it("returns empty for unknown language", () => {
      const rules = getLanguageRules("brainfuck");
      expect(rules).toHaveLength(0);
    });
  });

  describe("buildStandards", () => {
    it("builds 4-layer standards stack", () => {
      const standards = buildStandards({ primaryLanguage: "typescript" });
      expect(standards.layers).toHaveLength(4);
      expect(standards.layers[0]!.level).toBe(0);
      expect(standards.layers[1]!.level).toBe(1);
      expect(standards.layers[2]!.level).toBe(2);
      expect(standards.layers[3]!.level).toBe(3);
    });

    it("L0 has universal rules", () => {
      const standards = buildStandards({ primaryLanguage: "typescript" });
      const l0 = standards.layers[0]!;
      expect(l0.rules.length).toBeGreaterThan(0);
      expect(l0.rules.every(r => r.level === 0)).toBe(true);
    });

    it("L1 has language-specific rules", () => {
      const standards = buildStandards({ primaryLanguage: "typescript" });
      const l1 = standards.layers[1]!;
      expect(l1.rules.length).toBeGreaterThan(0);
      expect(l1.name).toContain("typescript");
    });

    it("effective rules include L0 + L1", () => {
      const standards = buildStandards({ primaryLanguage: "typescript" });
      const l0Count = standards.layers[0]!.rules.length;
      const l1Count = standards.layers[1]!.rules.length;
      expect(standards.effectiveRules.length).toBe(l0Count + l1Count);
    });

    it("includes custom L2 project rules", () => {
      const projectRules = [{
        id: "L2-PROJ-001", level: 2 as const, name: "Custom rule",
        description: "Test", check: "review" as const, severity: "warn" as const,
        enabled: true,
      }];

      const standards = buildStandards({
        primaryLanguage: "typescript",
        projectRules,
      });

      expect(standards.effectiveRules.some(r => r.id === "L2-PROJ-001")).toBe(true);
    });

    it("includes L3 user preference rules", () => {
      const userRules = [createUserPreferenceRule("Always use early returns")];

      const standards = buildStandards({
        primaryLanguage: "typescript",
        userRules,
      });

      expect(standards.effectiveRules.some(r => r.level === 3)).toBe(true);
    });

    it("sets updatedAt timestamp", () => {
      const standards = buildStandards({ primaryLanguage: "typescript" });
      expect(standards.updatedAt).toBeTruthy();
    });
  });

  describe("writeStandardsFile / readStandardsFile", () => {
    it("writes and reads STANDARDS.md", () => {
      const projectId = "proj_test123";
      ensureProjectDir(workspace, projectId);

      const standards = buildStandards({ primaryLanguage: "typescript", projectId });
      writeStandardsFile(workspace, projectId, standards);

      const filePath = join(getProjectDir(workspace, projectId), "STANDARDS.md");
      expect(existsSync(filePath)).toBe(true);

      const content = readFileSync(filePath, "utf-8");
      expect(content).toContain("# Coding Standards");
      expect(content).toContain("L0: Universal");
      expect(content).toContain("L1: Language: typescript");
    });

    it("round-trips effective rules through JSON trailer", () => {
      const projectId = "proj_roundtrip";
      ensureProjectDir(workspace, projectId);

      const standards = buildStandards({ primaryLanguage: "python", projectId });
      writeStandardsFile(workspace, projectId, standards);

      const result = readStandardsFile(workspace, projectId);
      expect(result).not.toBeNull();
      expect(result!.effectiveRules.length).toBe(standards.effectiveRules.length);
      expect(result!.effectiveRules[0]!.id).toBeTruthy();
    });

    it("returns null when file doesn't exist", () => {
      const result = readStandardsFile(workspace, "proj_nonexistent");
      expect(result).toBeNull();
    });
  });

  describe("createUserPreferenceRule", () => {
    it("creates L3 rule from correction text", () => {
      const rule = createUserPreferenceRule("Always validate input at function boundary");
      expect(rule.level).toBe(3);
      expect(rule.id).toMatch(/^L3-USR-/);
      expect(rule.description).toContain("validate input");
      expect(rule.enabled).toBe(true);
      expect(rule.severity).toBe("warn"); // default
    });

    it("respects severity override", () => {
      const rule = createUserPreferenceRule("Never use any", { severity: "error" });
      expect(rule.severity).toBe("error");
    });

    it("respects check override", () => {
      const rule = createUserPreferenceRule("Run lint", { check: "lint" });
      expect(rule.check).toBe("lint");
    });

    it("truncates long names to 80 chars", () => {
      const longText = "A".repeat(200);
      const rule = createUserPreferenceRule(longText);
      expect(rule.name.length).toBe(80);
      expect(rule.description.length).toBe(200); // Full text in description
    });
  });
});
