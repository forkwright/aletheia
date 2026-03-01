// Project file generators — markdown files as source of truth for Dianoia projects (Spec 32)
import { existsSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { paths } from "../taxis/paths.js";
import { createLogger } from "../koina/logger.js";
import { PlanningError } from "../koina/errors.js";
import type {
  DiscussionQuestion,
  PlanningPhase,
  PlanningProject,
  PlanningRequirement,
  PlanningResearch,
  ProjectContext,
} from "./types.js";

const log = createLogger("dianoia:files");

// --- Atomic file writing utility ---

function atomicWriteFile(filePath: string, content: string, encoding: BufferEncoding = "utf-8"): void {
  const tmpPath = `${filePath}.${process.pid}.${Date.now()}.tmp`;
  try {
    writeFileSync(tmpPath, content, encoding);
    renameSync(tmpPath, filePath);
  } catch (error) {
    // Clean up tmp file on error
    try {
      if (existsSync(tmpPath)) {
        unlinkSync(tmpPath);
      }
    } catch {
      // Ignore cleanup errors
    }
    log.error(`Failed to write file atomically: ${filePath}`, error);
    throw error;
  }
}

/**
 * Validate that a file exists and is non-empty after write.
 * Throws if file is missing or empty (fail-fast).
 */
function validateFileWritten(filePath: string, operation: string): void {
  if (!existsSync(filePath)) {
    throw new PlanningError(`${operation}: File not found after write: ${filePath}`, { code: "PLANNING_FILE_WRITE_FAILED", context: { operation, filePath } });
  }
  const content = readFileSync(filePath, "utf-8");
  if (content.trim().length === 0) {
    throw new PlanningError(`${operation}: File is empty after write: ${filePath}`, { code: "PLANNING_FILE_WRITE_FAILED", context: { operation, filePath } });
  }
}

// --- Directory management ---

/**
 * Resolve the absolute path for a project directory.
 * Backward compatible: absolute paths (pre-migration) are returned as-is.
 * New-style: slug resolves relative to instance/data/plans/{slug}.
 */
export function getProjectDir(projectDirValue: string): string {
  if (projectDirValue.startsWith("/")) return projectDirValue;
  return join(paths.plansDir(), projectDirValue);
}

export function getPhaseDir(projectDirValue: string, phaseId: string): string {
  return join(getProjectDir(projectDirValue), "phases", phaseId);
}

export function ensureProjectDir(projectDirValue: string): string {
  const dir = getProjectDir(projectDirValue);
  mkdirSync(dir, { recursive: true });
  mkdirSync(join(dir, "phases"), { recursive: true });
  return dir;
}

export function ensurePhaseDir(projectDirValue: string, phaseId: string): string {
  const dir = getPhaseDir(projectDirValue, phaseId);
  mkdirSync(dir, { recursive: true });
  return dir;
}

// --- File writers ---

export function writeProjectFile(
  project: PlanningProject,
  context?: ProjectContext | null,
): void {
  const dir = ensureProjectDir(project.projectDir!);
  const ctx = context ?? project.projectContext;

  const lines = [
    `# ${project.goal || "Untitled Project"}`,
    "",
    `| Field | Value |`,
    `|-------|-------|`,
    `| ID | \`${project.id}\` |`,
    `| State | ${project.state} |`,
    `| Created | ${project.createdAt} |`,
    `| Updated | ${project.updatedAt} |`,
    "",
  ];

  if (ctx) {
    lines.push("## Context", "");
    if (ctx.goal) lines.push(`**Goal:** ${ctx.goal}`, "");
    if (ctx.coreValue) lines.push(`**Core Value:** ${ctx.coreValue}`, "");
    if (ctx.constraints?.length) {
      lines.push("**Constraints:**");
      for (const c of ctx.constraints) lines.push(`- ${c}`);
      lines.push("");
    }
    if (ctx.keyDecisions?.length) {
      lines.push("**Key Decisions:**");
      for (const d of ctx.keyDecisions) lines.push(`- ${d}`);
      lines.push("");
    }
    if (ctx.rawTranscript?.length) {
      lines.push("## Discovery Transcript", "");
      for (const t of ctx.rawTranscript) {
        lines.push(`**Q${t.turn}:** ${t.text}`, "");
      }
    }
  }

  const filePath = join(dir, "PROJECT.md");
  atomicWriteFile(filePath, lines.join("\n"), "utf-8");
  validateFileWritten(filePath, "writeProjectFile");
  log.debug(`Wrote PROJECT.md for ${project.id}`);
}

export function writeRequirementsFile(
  projectDirValue: string,
  requirements: PlanningRequirement[],
): void {
  const dir = ensureProjectDir(projectDirValue);
  const lines = ["# Requirements", ""];

  // Group by category
  const byCategory = new Map<string, PlanningRequirement[]>();
  for (const req of requirements) {
    const list = byCategory.get(req.category) ?? [];
    list.push(req);
    byCategory.set(req.category, list);
  }

  // Group by tier within each category
  for (const [category, reqs] of byCategory) {
    lines.push(`## ${category}`, "");
    lines.push("| ID | Description | Tier | Status | Rationale |");
    lines.push("|-----|-------------|------|--------|-----------|");
    for (const req of reqs) {
      lines.push(
        `| ${req.reqId} | ${req.description} | ${req.tier} | ${req.status} | ${req.rationale ?? "—"} |`,
      );
    }
    lines.push("");
  }

  const filePath = join(dir, "REQUIREMENTS.md");
  atomicWriteFile(filePath, lines.join("\n"), "utf-8");
  validateFileWritten(filePath, "writeRequirementsFile");
  log.debug(`Wrote REQUIREMENTS.md for ${projectDirValue}`);
}

export function writeResearchFile(
  projectDirValue: string,
  research: PlanningResearch[],
): void {
  const dir = ensureProjectDir(projectDirValue);
  const lines = ["# Research", ""];

  for (const r of research) {
    lines.push(`## ${r.dimension} (${r.status})`, "");
    lines.push(r.content, "");
  }

  const filePath = join(dir, "RESEARCH.md");
  atomicWriteFile(filePath, lines.join("\n"), "utf-8");
  validateFileWritten(filePath, "writeResearchFile");
  log.debug(`Wrote RESEARCH.md for ${projectDirValue}`);
}

export function writeRoadmapFile(
  projectDirValue: string,
  phases: PlanningPhase[],
): void {
  const dir = ensureProjectDir(projectDirValue);
  const lines = ["# Roadmap", ""];

  for (const phase of phases) {
    const status = phase.status === "complete" ? "✅" :
      phase.status === "executing" ? "🔄" :
      phase.status === "failed" ? "❌" :
      phase.status === "skipped" ? "⏭" : "⬜";
    lines.push(`## ${status} Phase ${phase.phaseOrder + 1}: ${phase.name}`, "");
    lines.push(`**Goal:** ${phase.goal}`, "");
    if (phase.requirements.length > 0) {
      lines.push("**Requirements:**");
      for (const r of phase.requirements) lines.push(`- ${r}`);
      lines.push("");
    }
    if (phase.successCriteria.length > 0) {
      lines.push("**Success Criteria:**");
      for (const c of phase.successCriteria) lines.push(`- ${c}`);
      lines.push("");
    }
  }

  const filePath = join(dir, "ROADMAP.md");
  atomicWriteFile(filePath, lines.join("\n"), "utf-8");
  validateFileWritten(filePath, "writeRoadmapFile");
  log.debug(`Wrote ROADMAP.md for ${projectDirValue}`);
}

export function writeDiscussFile(
  projectDirValue: string,
  phaseId: string,
  questions: DiscussionQuestion[],
): void {
  const dir = ensurePhaseDir(projectDirValue, phaseId);
  const lines = ["# Phase Discussion", ""];

  for (const q of questions) {
    const statusIcon = q.status === "answered" ? "✅" :
      q.status === "skipped" ? "⏭" : "❓";
    lines.push(`## ${statusIcon} ${q.question}`, "");

    if (q.options.length > 0) {
      lines.push("**Options:**");
      for (const opt of q.options) {
        const selected = q.decision === opt.label ? " ← **selected**" : "";
        lines.push(`- **${opt.label}:** ${opt.rationale}${selected}`);
      }
      lines.push("");
    }

    if (q.recommendation) {
      lines.push(`**Recommended:** ${q.recommendation}`, "");
    }

    if (q.decision) {
      lines.push(`**Decision:** ${q.decision}`, "");
    }

    if (q.userNote) {
      lines.push(`**Note:** ${q.userNote}`, "");
    }
  }

  const filePath = join(dir, "DISCUSS.md");
  atomicWriteFile(filePath, lines.join("\n"), "utf-8");
  validateFileWritten(filePath, "writeDiscussFile");
  log.debug(`Wrote DISCUSS.md for phase ${phaseId}`);
}

export function writePlanFile(
  projectDirValue: string,
  phaseId: string,
  plan: unknown,
): void {
  const dir = ensurePhaseDir(projectDirValue, phaseId);
  const content = typeof plan === "string" ? plan : JSON.stringify(plan, null, 2);
  const filePath = join(dir, "PLAN.md");
  atomicWriteFile(filePath, `# Execution Plan\n\n\`\`\`json\n${content}\n\`\`\`\n`, "utf-8");
  validateFileWritten(filePath, "writePlanFile");
  log.debug(`Wrote PLAN.md for phase ${phaseId}`);
}

export function writeStateFile(
  projectDirValue: string,
  phaseId: string,
  state: Record<string, unknown>,
): void {
  const dir = ensurePhaseDir(projectDirValue, phaseId);
  const filePath = join(dir, "STATE.md");
  atomicWriteFile(
    filePath,
    `# Phase State\n\n\`\`\`json\n${JSON.stringify(state, null, 2)}\n\`\`\`\n`,
    "utf-8",
  );
  validateFileWritten(filePath, "writeStateFile");
  log.debug(`Wrote STATE.md for phase ${phaseId}`);
}

export function writeVerifyFile(
  projectDirValue: string,
  phaseId: string,
  verification: Record<string, unknown>,
): void {
  const dir = ensurePhaseDir(projectDirValue, phaseId);
  const v = verification;
  const lines = [
    "# Verification Results",
    "",
    `**Status:** ${v["overallStatus"] ?? v["status"] ?? "unknown"}`,
    `**Verified:** ${v["verifiedAt"] ?? "—"}`,
    "",
  ];

  if (v["summary"]) {
    lines.push("## Summary", "", v["summary"] as string, "");
  }

  const gaps = v["gaps"] as Array<Record<string, unknown>> | undefined;
  if (gaps?.length) {
    lines.push("## Gaps", "");
    for (const gap of gaps) {
      lines.push(`- **${gap["status"]}:** ${gap["detail"] ?? gap["criterion"] ?? gap["requirement"]}`);
      if (gap["proposedFix"]) lines.push(`  - Fix: ${gap["proposedFix"]}`);
    }
    lines.push("");
  }

  const filePath = join(dir, "VERIFY.md");
  atomicWriteFile(filePath, lines.join("\n"), "utf-8");
  validateFileWritten(filePath, "writeVerifyFile");
  log.debug(`Wrote VERIFY.md for phase ${phaseId}`);
}

// --- File readers (for context packets) ---

export function readProjectFile(projectDirValue: string): string | null {
  const path = join(getProjectDir(projectDirValue), "PROJECT.md");
  return existsSync(path) ? readFileSync(path, "utf-8") : null;
}

export function readRequirementsFile(projectDirValue: string): string | null {
  const path = join(getProjectDir(projectDirValue), "REQUIREMENTS.md");
  return existsSync(path) ? readFileSync(path, "utf-8") : null;
}

export function readRoadmapFile(projectDirValue: string): string | null {
  const path = join(getProjectDir(projectDirValue), "ROADMAP.md");
  return existsSync(path) ? readFileSync(path, "utf-8") : null;
}

export function readResearchFile(projectDirValue: string): string | null {
  const path = join(getProjectDir(projectDirValue), "RESEARCH.md");
  return existsSync(path) ? readFileSync(path, "utf-8") : null;
}

export function readDiscussFile(projectDirValue: string, phaseId: string): string | null {
  const path = join(getPhaseDir(projectDirValue, phaseId), "DISCUSS.md");
  return existsSync(path) ? readFileSync(path, "utf-8") : null;
}

export function readPlanFile(projectDirValue: string, phaseId: string): string | null {
  const path = join(getPhaseDir(projectDirValue, phaseId), "PLAN.md");
  return existsSync(path) ? readFileSync(path, "utf-8") : null;
}
