// Retrospective — project learning system (Spec 32 Phase 4)
//
// After a project completes (or is abandoned), extract patterns:
// - What phases succeeded/failed and why
// - Discussion decisions that proved correct/incorrect
// - Verification gaps that recurred
// - Execution timing data
//
// Persisted as RETRO.md in the project directory and available for
// future projects via context packet assembly.

import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { paths } from "../taxis/paths.js";
import { PlanningStore } from "./store.js";
import { ensureProjectDir, getProjectDir } from "./project-files.js";
import type Database from "better-sqlite3";
import type { PlanningPhase, VerificationResult } from "./types.js";

const log = createLogger("dianoia:retrospective");

export interface RetrospectiveEntry {
  projectId: string;
  goal: string;
  outcome: "complete" | "abandoned" | "partial";
  phases: PhaseRetrospective[];
  patterns: Pattern[];
  generatedAt: string;
}

export interface PhaseRetrospective {
  name: string;
  goal: string;
  status: string;
  discussionCount: number;
  verificationStatus: string | null;
  gapCount: number;
  duration: string | null; // ISO duration or null
}

export interface Pattern {
  type: "success" | "failure" | "antipattern" | "lesson";
  summary: string;
  context: string;
}

export class RetrospectiveGenerator {
  private store: PlanningStore;

  constructor(db: Database.Database) {
    this.store = new PlanningStore(db);
  }

  /**
   * Generate a retrospective for a completed/abandoned project.
   * Analyzes phases, discussions, verification results, and spawn records
   * to extract patterns and lessons.
   */
  generate(projectId: string): RetrospectiveEntry {
    const project = this.store.getProjectOrThrow(projectId);
    const phases = this.store.listPhases(projectId);
    const spawnRecords = this.store.listSpawnRecords(projectId);

    const outcome: RetrospectiveEntry["outcome"] =
      project.state === "complete" ? "complete" :
      project.state === "abandoned" ? "abandoned" : "partial";

    const phaseRetros = phases.map((phase): PhaseRetrospective => {
      const discussions = this.store.listDiscussionQuestions(projectId, phase.id);
      const verification = phase.verificationResult as VerificationResult | null;
      const phaseSpawns = spawnRecords.filter((r) => r.phaseId === phase.id);

      // Calculate duration from spawn records
      let duration: string | null = null;
      if (phaseSpawns.length > 0) {
        const start = phaseSpawns[0]?.startedAt;
        const end = phaseSpawns[phaseSpawns.length - 1]?.completedAt;
        if (start && end) {
          const ms = new Date(end).getTime() - new Date(start).getTime();
          duration = `${Math.round(ms / 1000)}s`;
        }
      }

      return {
        name: phase.name,
        goal: phase.goal,
        status: phase.status,
        discussionCount: discussions.length,
        verificationStatus: verification?.status ?? null,
        gapCount: verification?.gaps?.length ?? 0,
        duration,
      };
    });

    const patterns = this.extractPatterns(phases, phaseRetros, spawnRecords);

    const retro: RetrospectiveEntry = {
      projectId,
      goal: project.goal,
      outcome,
      phases: phaseRetros,
      patterns,
      generatedAt: new Date().toISOString(),
    };

    log.info(`Generated retrospective for project ${projectId}: ${patterns.length} patterns extracted`);
    return retro;
  }

  /**
   * Write RETRO.md to the project directory.
   * @param projectDirValue — project.projectDir from DB (slug for new projects, absolute path for legacy)
   */
  writeRetroFile(projectDirValue: string, retro: RetrospectiveEntry): void {
    const dir = ensureProjectDir(projectDirValue);
    const lines = [
      `# Retrospective: ${retro.goal}`,
      "",
      `| Field | Value |`,
      `|-------|-------|`,
      `| Outcome | ${retro.outcome} |`,
      `| Phases | ${retro.phases.length} |`,
      `| Patterns | ${retro.patterns.length} |`,
      `| Generated | ${retro.generatedAt} |`,
      "",
      "## Phases",
      "",
    ];

    for (const phase of retro.phases) {
      const icon = phase.status === "complete" ? "✅" :
        phase.status === "failed" ? "❌" :
        phase.status === "skipped" ? "⏭" : "⬜";
      lines.push(`### ${icon} ${phase.name}`);
      lines.push(`- **Goal:** ${phase.goal}`);
      lines.push(`- **Status:** ${phase.status}`);
      if (phase.duration) lines.push(`- **Duration:** ${phase.duration}`);
      if (phase.discussionCount > 0) lines.push(`- **Discussions:** ${phase.discussionCount}`);
      if (phase.verificationStatus) lines.push(`- **Verification:** ${phase.verificationStatus}`);
      if (phase.gapCount > 0) lines.push(`- **Gaps found:** ${phase.gapCount}`);
      lines.push("");
    }

    if (retro.patterns.length > 0) {
      lines.push("## Patterns", "");
      for (const pattern of retro.patterns) {
        const icon = pattern.type === "success" ? "✅" :
          pattern.type === "failure" ? "❌" :
          pattern.type === "antipattern" ? "⚠️" : "💡";
        lines.push(`### ${icon} ${pattern.summary}`);
        lines.push(`**Type:** ${pattern.type}`);
        lines.push(`${pattern.context}`);
        lines.push("");
      }
    }

    writeFileSync(join(dir, "RETRO.md"), lines.join("\n"), "utf-8");
    log.debug(`Wrote RETRO.md for ${retro.projectId}`);
  }

  /**
   * Read all retrospectives from past projects (for feeding into new project context).
   * Scans instance/data/plans/ for retro.json files.
   */
  readPastRetros(): RetrospectiveEntry[] {
    const plansDir = paths.plansDir();

    if (!existsSync(plansDir)) return [];

    const retros: RetrospectiveEntry[] = [];
    try {
      const dirs = readdirSync(plansDir);
      for (const dir of dirs) {
        const retroPath = join(plansDir, dir, "RETRO.md");
        if (existsSync(retroPath)) {
          // We store structured data too for quick access
          const jsonPath = join(plansDir, dir, "retro.json");
          if (existsSync(jsonPath)) {
            try {
              const data = JSON.parse(readFileSync(jsonPath, "utf-8")) as RetrospectiveEntry;
              retros.push(data);
            } catch {
              // Skip corrupt entries
            }
          }
        }
      }
    } catch {
      // Plans dir not readable
    }

    return retros;
  }

  /**
   * Write structured JSON alongside RETRO.md for programmatic access.
   * @param projectDirValue — project.projectDir from DB (slug for new projects, absolute path for legacy)
   */
  writeRetroJson(projectDirValue: string, retro: RetrospectiveEntry): void {
    const dir = getProjectDir(projectDirValue);
    writeFileSync(join(dir, "retro.json"), JSON.stringify(retro, null, 2), "utf-8");
  }

  // --- Pattern extraction ---

  private extractPatterns(
    _phases: PlanningPhase[],
    phaseRetros: PhaseRetrospective[],
    spawnRecords: Array<{ status: string; phaseId: string }>,
  ): Pattern[] {
    const patterns: Pattern[] = [];

    // Pattern: All phases succeeded
    const allComplete = phaseRetros.every((p) => p.status === "complete");
    if (allComplete && phaseRetros.length > 0) {
      patterns.push({
        type: "success",
        summary: "All phases completed successfully",
        context: `${phaseRetros.length} phases executed without failures.`,
      });
    }

    // Pattern: Failed phases
    const failedPhases = phaseRetros.filter((p) => p.status === "failed");
    for (const fp of failedPhases) {
      patterns.push({
        type: "failure",
        summary: `Phase "${fp.name}" failed`,
        context: `Goal: ${fp.goal}. Verification: ${fp.verificationStatus ?? "not run"}. Gaps: ${fp.gapCount}.`,
      });
    }

    // Pattern: Phases with verification gaps that were eventually resolved
    const gapPhases = phaseRetros.filter((p) => p.gapCount > 0 && p.status === "complete");
    if (gapPhases.length > 0) {
      patterns.push({
        type: "lesson",
        summary: "Verification gaps were recoverable",
        context: `${gapPhases.length} phase(s) had verification gaps but completed after gap closure: ${gapPhases.map((p) => p.name).join(", ")}.`,
      });
    }

    // Pattern: Skipped phases (cascade from failures)
    const skippedPhases = phaseRetros.filter((p) => p.status === "skipped");
    if (skippedPhases.length > 0) {
      patterns.push({
        type: "antipattern",
        summary: "Cascade skip from dependency failure",
        context: `${skippedPhases.length} phase(s) were skipped due to upstream failures: ${skippedPhases.map((p) => p.name).join(", ")}.`,
      });
    }

    // Pattern: Phases with no discussion (fast-tracked)
    const noDiscussion = phaseRetros.filter((p) => p.discussionCount === 0);
    if (noDiscussion.some((p) => p.status === "failed")) {
      patterns.push({
        type: "antipattern",
        summary: "Phases without discussion had higher failure rate",
        context: `${noDiscussion.filter((p) => p.status === "failed").length}/${noDiscussion.length} undiscussed phases failed vs ${failedPhases.length}/${phaseRetros.length} overall.`,
      });
    }

    // Pattern: Zombie detection
    const zombieCount = spawnRecords.filter((r) => r.status === "zombie").length;
    if (zombieCount > 0) {
      patterns.push({
        type: "antipattern",
        summary: "Zombie spawn records detected",
        context: `${zombieCount} sub-agent spawn(s) timed out and were reaped as zombies. Consider increasing timeout or reducing task scope.`,
      });
    }

    return patterns;
  }
}
