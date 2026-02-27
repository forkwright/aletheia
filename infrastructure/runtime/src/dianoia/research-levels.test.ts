// Tests for ResearchLevels — right-sized research per phase (ENG-11)
import { describe, it, expect } from "vitest";
import {
  extractComplexitySignals,
  selectResearchLevel,
  getResearchConfig,
  determineResearchLevel,
  RESEARCH_LEVELS,
} from "./research-levels.js";
import type { PlanningPhase, PlanningRequirement } from "./types.js";

function makePhase(overrides?: Partial<PlanningPhase>): PlanningPhase {
  return {
    id: "phase_test",
    projectId: "proj_test",
    name: "Test Phase",
    goal: "Do something",
    requirements: [],
    successCriteria: [],
    dependencies: [],
    plan: null,
    status: "pending",
    phaseOrder: 0,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function makeReq(description: string): PlanningRequirement {
  return {
    id: "req_test",
    projectId: "proj_test",
    phaseId: null,
    reqId: "REQ-01",
    description,
    category: "Test",
    tier: "v1",
    status: "pending",
    rationale: null,
    dependsOn: [],
    blockedBy: [],
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
  };
}

describe("ResearchLevels", () => {
  describe("RESEARCH_LEVELS", () => {
    it("has 4 levels (0-3)", () => {
      expect(Object.keys(RESEARCH_LEVELS)).toHaveLength(4);
      expect(RESEARCH_LEVELS[0].name).toBe("Skip");
      expect(RESEARCH_LEVELS[1].name).toBe("Quick");
      expect(RESEARCH_LEVELS[2].name).toBe("Standard");
      expect(RESEARCH_LEVELS[3].name).toBe("Deep Dive");
    });

    it("L0 has no dimensions", () => {
      expect(RESEARCH_LEVELS[0].dimensions).toHaveLength(0);
      expect(RESEARCH_LEVELS[0].researcherCount).toBe(0);
    });

    it("L1 has 1 dimension (pitfalls)", () => {
      expect(RESEARCH_LEVELS[1].dimensions).toEqual(["pitfalls"]);
      expect(RESEARCH_LEVELS[1].researcherCount).toBe(1);
    });

    it("L2 and L3 have 4 dimensions", () => {
      expect(RESEARCH_LEVELS[2].dimensions).toHaveLength(4);
      expect(RESEARCH_LEVELS[3].dimensions).toHaveLength(4);
    });

    it("L2 and L3 need synthesis", () => {
      expect(RESEARCH_LEVELS[2].needsSynthesis).toBe(true);
      expect(RESEARCH_LEVELS[3].needsSynthesis).toBe(true);
    });
  });

  describe("extractComplexitySignals", () => {
    it("detects novel technology keywords", () => {
      const phase = makePhase({ goal: "Migrate to a new framework" });
      const signals = extractComplexitySignals(phase, []);
      expect(signals.hasNovelTechnology).toBe(true);
    });

    it("detects security concerns", () => {
      const phase = makePhase({ goal: "Implement user authentication" });
      const signals = extractComplexitySignals(phase, []);
      expect(signals.hasSecurityConcerns).toBe(true);
    });

    it("detects data migration", () => {
      const reqs = [makeReq("Database schema migration for users table")];
      const signals = extractComplexitySignals(makePhase(), reqs);
      expect(signals.hasDataMigration).toBe(true);
    });

    it("detects external integrations", () => {
      const reqs = [makeReq("Integrate with third-party payment SDK")];
      const signals = extractComplexitySignals(makePhase(), reqs);
      expect(signals.hasExternalIntegrations).toBe(true);
    });

    it("detects architectural decisions", () => {
      const phase = makePhase({ goal: "Design the distributed message queue architecture" });
      const signals = extractComplexitySignals(phase, []);
      expect(signals.hasArchitecturalDecisions).toBe(true);
    });

    it("counts requirements", () => {
      const reqs = Array.from({ length: 8 }, (_, i) => makeReq(`Req ${i}`));
      const signals = extractComplexitySignals(makePhase(), reqs);
      expect(signals.requirementCount).toBe(8);
    });

    it("respects user override", () => {
      const signals = extractComplexitySignals(makePhase(), [], { userOverride: 3 });
      expect(signals.userOverride).toBe(3);
    });

    it("respects existing patterns flag", () => {
      const signals = extractComplexitySignals(makePhase(), [], { existingPatterns: true });
      expect(signals.hasExistingPatterns).toBe(true);
    });
  });

  describe("selectResearchLevel", () => {
    it("returns L0 for simple phase with existing patterns", () => {
      const level = selectResearchLevel({
        requirementCount: 1,
        hasNovelTechnology: false,
        hasSecurityConcerns: false,
        hasDataMigration: false,
        hasExternalIntegrations: false,
        hasArchitecturalDecisions: false,
        hasExistingPatterns: true,
        userOverride: null,
      });
      expect(level).toBe(0);
    });

    it("returns L1 for few requirements, no special signals", () => {
      const level = selectResearchLevel({
        requirementCount: 3,
        hasNovelTechnology: false,
        hasSecurityConcerns: false,
        hasDataMigration: false,
        hasExternalIntegrations: false,
        hasArchitecturalDecisions: false,
        hasExistingPatterns: false,
        userOverride: null,
      });
      expect(level).toBe(1);
    });

    it("returns L2 for moderate complexity", () => {
      const level = selectResearchLevel({
        requirementCount: 5,
        hasNovelTechnology: false,
        hasSecurityConcerns: true,
        hasDataMigration: false,
        hasExternalIntegrations: false,
        hasArchitecturalDecisions: false,
        hasExistingPatterns: false,
        userOverride: null,
      });
      expect(level).toBe(2);
    });

    it("returns L3 for high complexity", () => {
      const level = selectResearchLevel({
        requirementCount: 12,
        hasNovelTechnology: true,
        hasSecurityConcerns: true,
        hasDataMigration: true,
        hasExternalIntegrations: false,
        hasArchitecturalDecisions: true,
        hasExistingPatterns: false,
        userOverride: null,
      });
      expect(level).toBe(3);
    });

    it("respects user override regardless of signals", () => {
      const level = selectResearchLevel({
        requirementCount: 1,
        hasNovelTechnology: false,
        hasSecurityConcerns: false,
        hasDataMigration: false,
        hasExternalIntegrations: false,
        hasArchitecturalDecisions: false,
        hasExistingPatterns: true,
        userOverride: 3,
      });
      expect(level).toBe(3);
    });
  });

  describe("getResearchConfig", () => {
    it("returns config for each level", () => {
      for (const level of [0, 1, 2, 3] as const) {
        const config = getResearchConfig(level);
        expect(config.level).toBe(level);
        expect(config.name).toBeTruthy();
      }
    });
  });

  describe("determineResearchLevel", () => {
    it("combines extraction and selection", () => {
      const phase = makePhase({ goal: "Implement OAuth authentication with external provider" });
      const reqs = [
        makeReq("OIDC integration"),
        makeReq("Token refresh"),
        makeReq("Permission model"),
      ];

      const result = determineResearchLevel(phase, reqs);

      expect(result.level).toBeGreaterThanOrEqual(1);
      expect(result.config).toBeDefined();
      expect(result.signals.hasSecurityConcerns).toBe(true);
    });

    it("returns L0 for trivial phase", () => {
      const phase = makePhase({ goal: "Add a readme file" });
      const result = determineResearchLevel(phase, [], { existingPatterns: true });
      expect(result.level).toBe(0);
    });
  });
});
