// Tests for DiscussionArtifacts — structured gray-area documentation (ENG-02)
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, existsSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  writeStructuredDiscussFile,
  readStructuredDiscussFile,
  extractDecisionsFromQuestions,
  createEmptyArtifact,
  isDiscussionLocked,
  acquireDiscussionLock,
  releaseDiscussionLock,
  type DiscussionArtifact,
  type BoundaryItem,
  type ImplementationDecision,
  type DiscretionItem,
  type DeferredIdea,
} from "./discussion-artifacts.js";
import { ensurePhaseDir, getPhaseDir } from "./project-files.js";
import type { DiscussionQuestion } from "./types.js";

function createTempWorkspace(): string {
  const dir = join(tmpdir(), `dianoia-discuss-test-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

const TEST_PROJECT_ID = "proj_test123";
const TEST_PHASE_ID = "phase_abc";

function createFullArtifact(): DiscussionArtifact {
  return {
    phaseId: TEST_PHASE_ID,
    projectId: TEST_PROJECT_ID,
    boundaries: [
      { item: "OAuth integration", scope: "in-scope", rationale: "Required for auth phase" },
      { item: "2FA", scope: "other-phase", targetPhase: "Phase 3", rationale: "Nice-to-have, not MVP" },
      { item: "LDAP", scope: "out-of-scope", rationale: "Enterprise only" },
    ],
    decisions: [
      {
        decision: "Use JWT for session tokens",
        alternatives: ["Session cookies", "Opaque tokens"],
        rationale: "Stateless, works with CDN",
        impact: "high",
        source: "human",
      },
      {
        decision: "Store refresh tokens in HttpOnly cookies",
        alternatives: ["localStorage", "IndexedDB"],
        rationale: "XSS protection",
        impact: "medium",
        source: "discussion",
      },
    ],
    discretion: [
      {
        item: "Token expiry duration",
        constraints: ["Between 15min and 1hr for access tokens", "Between 7d and 30d for refresh tokens"],
        escalationTrigger: "If changing refresh token to < 7 days",
      },
    ],
    deferred: [
      {
        idea: "Magic link authentication",
        targetPhase: "Phase 4",
        rationale: "Nice UX but not required for MVP",
        priority: "medium",
      },
      {
        idea: "Social login (Google, GitHub)",
        targetPhase: "v2",
        rationale: "Integration complexity not justified yet",
        priority: "low",
      },
    ],
    questions: [
      {
        id: "q1",
        projectId: TEST_PROJECT_ID,
        phaseId: TEST_PHASE_ID,
        question: "Which token format?",
        options: [
          { label: "JWT", rationale: "Stateless" },
          { label: "Opaque", rationale: "More secure" },
        ],
        recommendation: "JWT",
        decision: "JWT",
        userNote: "Aligns with CDN strategy",
        status: "answered",
        createdAt: "2026-02-26T00:00:00Z",
        updatedAt: "2026-02-26T00:00:00Z",
      },
    ],
    updatedAt: "2026-02-26T22:00:00.000Z",
  };
}

describe("DiscussionArtifacts", () => {
  let workspace: string;
  let projectDirValue: string;

  beforeEach(() => {
    workspace = createTempWorkspace();
    projectDirValue = join(workspace, ".dianoia", "projects", TEST_PROJECT_ID);
  });

  afterEach(() => {
    try { rmSync(workspace, { recursive: true, force: true }); } catch { /* ignore */ }
  });

  describe("writeStructuredDiscussFile", () => {
    it("creates DISCUSS.md with all four sections", () => {
      const artifact = createFullArtifact();
      writeStructuredDiscussFile(projectDirValue, artifact);

      const phaseDir = getPhaseDir(projectDirValue, TEST_PHASE_ID);
      const filePath = join(phaseDir, "DISCUSS.md");
      expect(existsSync(filePath)).toBe(true);

      const content = readFileSync(filePath, "utf-8");
      expect(content).toContain("# Phase Discussion Artifacts");
      expect(content).toContain("## Phase Boundary");
      expect(content).toContain("## Implementation Decisions");
      expect(content).toContain("## Claude's Discretion");
      expect(content).toContain("## Deferred Ideas");
    });

    it("includes boundary items in table format", () => {
      const artifact = createFullArtifact();
      writeStructuredDiscussFile(projectDirValue, artifact);

      const phaseDir = getPhaseDir(projectDirValue, TEST_PHASE_ID);
      const content = readFileSync(join(phaseDir, "DISCUSS.md"), "utf-8");

      expect(content).toContain("OAuth integration");
      expect(content).toContain("in-scope");
      expect(content).toContain("2FA");
      expect(content).toContain("Phase 3");
      expect(content).toContain("LDAP");
      expect(content).toContain("out-of-scope");
    });

    it("includes implementation decisions with alternatives", () => {
      const artifact = createFullArtifact();
      writeStructuredDiscussFile(projectDirValue, artifact);

      const phaseDir = getPhaseDir(projectDirValue, TEST_PHASE_ID);
      const content = readFileSync(join(phaseDir, "DISCUSS.md"), "utf-8");

      expect(content).toContain("Use JWT for session tokens");
      expect(content).toContain("Session cookies");
      expect(content).toContain("Stateless, works with CDN");
      expect(content).toContain("🔴"); // high impact
      expect(content).toContain("🟡"); // medium impact
    });

    it("includes discretion items with constraints", () => {
      const artifact = createFullArtifact();
      writeStructuredDiscussFile(projectDirValue, artifact);

      const phaseDir = getPhaseDir(projectDirValue, TEST_PHASE_ID);
      const content = readFileSync(join(phaseDir, "DISCUSS.md"), "utf-8");

      expect(content).toContain("Token expiry duration");
      expect(content).toContain("15min and 1hr");
      expect(content).toContain("Escalate if");
    });

    it("includes deferred ideas with priority", () => {
      const artifact = createFullArtifact();
      writeStructuredDiscussFile(projectDirValue, artifact);

      const phaseDir = getPhaseDir(projectDirValue, TEST_PHASE_ID);
      const content = readFileSync(join(phaseDir, "DISCUSS.md"), "utf-8");

      expect(content).toContain("Magic link authentication");
      expect(content).toContain("Phase 4");
      expect(content).toContain("Social login");
    });

    it("preserves raw discussion questions", () => {
      const artifact = createFullArtifact();
      writeStructuredDiscussFile(projectDirValue, artifact);

      const phaseDir = getPhaseDir(projectDirValue, TEST_PHASE_ID);
      const content = readFileSync(join(phaseDir, "DISCUSS.md"), "utf-8");

      expect(content).toContain("Which token format?");
      expect(content).toContain("✅");
      expect(content).toContain("Aligns with CDN strategy");
    });

    it("embeds JSON trailer for machine parsing", () => {
      const artifact = createFullArtifact();
      writeStructuredDiscussFile(projectDirValue, artifact);

      const phaseDir = getPhaseDir(projectDirValue, TEST_PHASE_ID);
      const content = readFileSync(join(phaseDir, "DISCUSS.md"), "utf-8");

      const jsonMatch = content.match(/```json\n([\s\S]+?)\n```/);
      expect(jsonMatch).not.toBeNull();

      const parsed = JSON.parse(jsonMatch![1]!);
      expect(parsed.boundaries).toHaveLength(3);
      expect(parsed.decisions).toHaveLength(2);
      expect(parsed.discretion).toHaveLength(1);
      expect(parsed.deferred).toHaveLength(2);
    });

    it("handles empty artifact gracefully", () => {
      const artifact = createEmptyArtifact(TEST_PROJECT_ID, TEST_PHASE_ID);
      writeStructuredDiscussFile(projectDirValue, artifact);

      const phaseDir = getPhaseDir(projectDirValue, TEST_PHASE_ID);
      const content = readFileSync(join(phaseDir, "DISCUSS.md"), "utf-8");

      expect(content).toContain("No explicit boundaries");
      expect(content).toContain("No implementation decisions");
      expect(content).toContain("No discretion items");
      expect(content).toContain("No deferred ideas");
    });
  });

  describe("readStructuredDiscussFile", () => {
    it("round-trips structured data through write/read", () => {
      const artifact = createFullArtifact();
      writeStructuredDiscussFile(projectDirValue, artifact);

      const result = readStructuredDiscussFile(projectDirValue, TEST_PHASE_ID);

      expect(result).not.toBeNull();
      expect(result!.boundaries).toHaveLength(3);
      expect(result!.boundaries[0]!.item).toBe("OAuth integration");
      expect(result!.decisions).toHaveLength(2);
      expect(result!.decisions[0]!.decision).toBe("Use JWT for session tokens");
      expect(result!.discretion).toHaveLength(1);
      expect(result!.deferred).toHaveLength(2);
    });

    it("returns null when file doesn't exist", () => {
      const result = readStructuredDiscussFile(projectDirValue, "nonexistent");
      expect(result).toBeNull();
    });
  });

  describe("extractDecisionsFromQuestions", () => {
    it("converts answered questions to implementation decisions", () => {
      const questions: DiscussionQuestion[] = [
        {
          id: "q1",
          projectId: TEST_PROJECT_ID,
          phaseId: TEST_PHASE_ID,
          question: "Which database?",
          options: [
            { label: "SQLite", rationale: "Simple and embedded" },
            { label: "PostgreSQL", rationale: "Scalable" },
          ],
          recommendation: "SQLite",
          decision: "SQLite",
          userNote: "Embedded is simpler for our use case",
          status: "answered",
          createdAt: "2026-02-26T00:00:00Z",
          updatedAt: "2026-02-26T00:00:00Z",
        },
        {
          id: "q2",
          projectId: TEST_PROJECT_ID,
          phaseId: TEST_PHASE_ID,
          question: "Skipped question",
          options: [],
          recommendation: null,
          decision: null,
          userNote: null,
          status: "skipped",
          createdAt: "2026-02-26T00:00:00Z",
          updatedAt: "2026-02-26T00:00:00Z",
        },
      ];

      const decisions = extractDecisionsFromQuestions(questions);

      expect(decisions).toHaveLength(1);
      expect(decisions[0]!.decision).toContain("Which database?");
      expect(decisions[0]!.decision).toContain("SQLite");
      expect(decisions[0]!.alternatives).toContain("PostgreSQL");
      expect(decisions[0]!.rationale).toContain("Embedded is simpler");
      expect(decisions[0]!.source).toBe("human");
    });

    it("returns empty array for no answered questions", () => {
      const decisions = extractDecisionsFromQuestions([]);
      expect(decisions).toHaveLength(0);
    });

    it("uses discussion source when no userNote", () => {
      const questions: DiscussionQuestion[] = [
        {
          id: "q1",
          projectId: TEST_PROJECT_ID,
          phaseId: TEST_PHASE_ID,
          question: "Auto-answered?",
          options: [
            { label: "Yes", rationale: "Makes sense" },
            { label: "No", rationale: "Doesn't" },
          ],
          recommendation: "Yes",
          decision: "Yes",
          userNote: null,
          status: "answered",
          createdAt: "2026-02-26T00:00:00Z",
          updatedAt: "2026-02-26T00:00:00Z",
        },
      ];

      const decisions = extractDecisionsFromQuestions(questions);
      expect(decisions[0]!.source).toBe("discussion");
    });
  });

  describe("createEmptyArtifact", () => {
    it("creates artifact with correct IDs and empty arrays", () => {
      const artifact = createEmptyArtifact("proj_x", "phase_y");

      expect(artifact.projectId).toBe("proj_x");
      expect(artifact.phaseId).toBe("phase_y");
      expect(artifact.boundaries).toEqual([]);
      expect(artifact.decisions).toEqual([]);
      expect(artifact.discretion).toEqual([]);
      expect(artifact.deferred).toEqual([]);
      expect(artifact.questions).toEqual([]);
      expect(artifact.updatedAt).toBeTruthy();
    });
  });

  describe("lock semantics", () => {
    it("reports unlocked when no lock exists", () => {
      ensurePhaseDir(projectDirValue, TEST_PHASE_ID);
      const status = isDiscussionLocked(projectDirValue, TEST_PHASE_ID);
      expect(status.locked).toBe(false);
    });

    it("acquires lock successfully", () => {
      const acquired = acquireDiscussionLock(projectDirValue, TEST_PHASE_ID, "session-1");
      expect(acquired).toBe(true);

      const status = isDiscussionLocked(projectDirValue, TEST_PHASE_ID);
      expect(status.locked).toBe(true);
      expect(status.lockedBy).toBe("session-1");
    });

    it("prevents other sessions from acquiring lock", () => {
      acquireDiscussionLock(projectDirValue, TEST_PHASE_ID, "session-1");

      const acquired = acquireDiscussionLock(projectDirValue, TEST_PHASE_ID, "session-2");
      expect(acquired).toBe(false);
    });

    it("allows same session to re-acquire lock", () => {
      acquireDiscussionLock(projectDirValue, TEST_PHASE_ID, "session-1");

      const acquired = acquireDiscussionLock(projectDirValue, TEST_PHASE_ID, "session-1");
      expect(acquired).toBe(true);
    });

    it("releases lock successfully", () => {
      acquireDiscussionLock(projectDirValue, TEST_PHASE_ID, "session-1");

      const released = releaseDiscussionLock(projectDirValue, TEST_PHASE_ID, "session-1");
      expect(released).toBe(true);

      const status = isDiscussionLocked(projectDirValue, TEST_PHASE_ID);
      expect(status.locked).toBe(false);
    });

    it("prevents other sessions from releasing lock", () => {
      acquireDiscussionLock(projectDirValue, TEST_PHASE_ID, "session-1");

      const released = releaseDiscussionLock(projectDirValue, TEST_PHASE_ID, "session-2");
      expect(released).toBe(false);

      // Lock still held
      const status = isDiscussionLocked(projectDirValue, TEST_PHASE_ID);
      expect(status.locked).toBe(true);
    });

    it("treats stale locks as unlocked (5 min expiry)", () => {
      // Manually write a stale lock
      ensurePhaseDir(projectDirValue, TEST_PHASE_ID);
      const phaseDir = getPhaseDir(projectDirValue, TEST_PHASE_ID);
      const lockPath = join(phaseDir, ".discuss.lock");
      const staleTime = new Date(Date.now() - 6 * 60 * 1000).toISOString(); // 6 minutes ago
      const { writeFileSync } = require("node:fs");
      writeFileSync(lockPath, JSON.stringify({ lockedBy: "stale-session", lockedAt: staleTime }));

      const status = isDiscussionLocked(projectDirValue, TEST_PHASE_ID);
      expect(status.locked).toBe(false);

      // Can acquire over stale lock
      const acquired = acquireDiscussionLock(projectDirValue, TEST_PHASE_ID, "new-session");
      expect(acquired).toBe(true);
    });
  });
});
