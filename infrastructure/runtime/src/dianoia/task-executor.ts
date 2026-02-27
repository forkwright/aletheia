// TaskExecutor — task-level execution with verification, review loops, and git commits
//
// Replaces the phase-level "dispatch and hope" pattern with:
// 1. Break phase plan into tasks (already done by Phase 4)
// 2. Execute each task with enriched context (files, action, verify, must_haves)
// 3. Verify work was done: check git diff, check artifacts, validate must_haves
// 4. Optional reviewer loop (max 3 rounds) for quality gate
// 5. Git commit per task with conventional prefix

import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createLogger } from "../koina/logger.js";
import type { Task } from "./task-store.js";

const log = createLogger("dianoia:task-executor");

// ─── Types ───────────────────────────────────────────────────

export type DeviationLevel = "auto" | "warn" | "ask" | "block";

export interface DeviationRule {
  pattern: RegExp;
  level: DeviationLevel;
  description: string;
}

export interface VerificationResult {
  passed: boolean;
  level: "truths" | "artifacts" | "wiring";
  checks: VerificationCheck[];
  summary: string;
}

export interface VerificationCheck {
  name: string;
  passed: boolean;
  details: string;
}

export interface ReviewRound {
  round: number;
  issues: ReviewIssue[];
  passed: boolean;
}

export interface ReviewIssue {
  severity: "error" | "warning" | "info";
  file?: string;
  message: string;
  suggestion?: string;
}

export interface TaskExecutionResult {
  taskId: string;
  status: "success" | "partial" | "failed" | "skipped";
  verification: VerificationResult | null;
  reviewRounds: ReviewRound[];
  commitHash: string | null;
  duration: number;
  error?: string;
}

export interface TaskExecutorConfig {
  workspaceRoot: string;
  maxReviewRounds: number;
  enableGitCommits: boolean;
  enableReview: boolean;
  deviationRules: DeviationRule[];
}

// ─── Default Deviation Rules ─────────────────────────────────

export const DEFAULT_DEVIATION_RULES: DeviationRule[] = [
  // Auto-fixable: formatting, imports, lint issues
  { pattern: /\b(lint|format|import|typo|whitespace|trailing)\b/i, level: "auto", description: "Auto-fix: style/formatting issues" },
  // Warn: test failures, minor refactors
  { pattern: /\b(test fail|refactor|rename|move file)\b/i, level: "warn", description: "Warn: minor structural changes" },
  // Ask: API changes, schema changes, dependency updates
  { pattern: /\b(api change|schema|migration|breaking|dependency|package)\b/i, level: "ask", description: "Ask: breaking changes need approval" },
  // Block: architecture changes, security, credentials
  { pattern: /\b(architect|security|credential|secret|permission|auth)\b/i, level: "block", description: "Block: architecture/security changes require human" },
];

// ─── Task Execution Prompt Builder ───────────────────────────

export function buildTaskPrompt(task: Task, projectGoal: string, phaseGoal: string): string {
  const sections: string[] = [];

  sections.push(`# Task: ${task.title}`);
  sections.push("");
  sections.push(`**Task ID:** ${task.taskId}`);
  sections.push(`**Priority:** ${task.priority}`);
  sections.push("");

  if (task.description) {
    sections.push("## Description");
    sections.push(task.description);
    sections.push("");
  }

  if (task.action) {
    sections.push("## Action Required");
    sections.push(task.action);
    sections.push("");
  }

  if (task.files.length > 0) {
    sections.push("## Relevant Files");
    for (const f of task.files) {
      sections.push(`- \`${f}\``);
    }
    sections.push("");
  }

  if (task.mustHaves.length > 0) {
    sections.push("## Must-Haves (Verification Criteria)");
    for (const m of task.mustHaves) {
      sections.push(`- ${m}`);
    }
    sections.push("");
  }

  if (task.verify) {
    sections.push("## Verification Command");
    sections.push(`Run this to verify your work: \`${task.verify}\``);
    sections.push("");
  }

  sections.push("## Context");
  sections.push(`**Project goal:** ${projectGoal}`);
  sections.push(`**Phase goal:** ${phaseGoal}`);
  sections.push("");

  sections.push("## Rules");
  sections.push("1. **Do the work.** Write code, create files, make edits. Do NOT just describe what should be done.");
  sections.push("2. **Stay in scope.** Only change files relevant to this task.");
  sections.push("3. **Build must pass.** Run the build after changes and fix any errors.");
  sections.push("4. **Run verification** if a verify command is specified.");
  sections.push("5. **Git add your changes** but do NOT commit — the orchestrator handles commits.");
  sections.push("");

  sections.push("## Output Format (REQUIRED)");
  sections.push("End your response with:");
  sections.push("```json");
  sections.push("{");
  sections.push('  "status": "success" | "partial" | "failed",');
  sections.push('  "summary": "What you did",');
  sections.push('  "filesChanged": ["path/to/file.ts"],');
  sections.push('  "mustHaveResults": { "criterion": true/false },');
  sections.push('  "buildPassed": true,');
  sections.push('  "verifyPassed": true,');
  sections.push('  "issues": [],');
  sections.push('  "confidence": 0.95');
  sections.push("}");
  sections.push("```");

  return sections.join("\n");
}

// ─── Verification Engine ─────────────────────────────────────

export function verifyTaskCompletion(
  task: Task,
  executionResult: Record<string, unknown>,
  workspaceRoot: string,
): VerificationResult {
  const checks: VerificationCheck[] = [];

  // Level 1: Truths — did anything actually change?
  const gitDiff = getGitDiff(workspaceRoot);
  const hasChanges = gitDiff.trim().length > 0;
  checks.push({
    name: "git_diff_exists",
    passed: hasChanges,
    details: hasChanges
      ? `Git shows changes: ${gitDiff.split("\n").length} lines`
      : "No git changes detected — sub-agent may not have written anything",
  });

  // Check that claimed filesChanged actually exist in diff
  const claimedFiles = (executionResult["filesChanged"] as string[]) ?? [];
  if (claimedFiles.length > 0) {
    const diffFiles = getDiffFiles(workspaceRoot);
    const missingFromDiff = claimedFiles.filter(f => !diffFiles.some(d => d.includes(f) || f.includes(d)));
    checks.push({
      name: "claimed_files_in_diff",
      passed: missingFromDiff.length === 0,
      details: missingFromDiff.length === 0
        ? `All ${claimedFiles.length} claimed files found in git diff`
        : `Files claimed but not in diff: ${missingFromDiff.join(", ")}`,
    });
  }

  // Level 2: Artifacts — do the files exist?
  for (const file of task.files) {
    const fullPath = `${workspaceRoot}/${file}`;
    const exists = existsSync(fullPath);
    checks.push({
      name: `file_exists:${file}`,
      passed: exists,
      details: exists ? `File exists: ${file}` : `Expected file missing: ${file}`,
    });
  }

  // Level 3: Wiring — do must_haves pass?
  const mustHaveResults = (executionResult["mustHaveResults"] as Record<string, boolean>) ?? {};
  for (const mh of task.mustHaves) {
    const passed = mustHaveResults[mh] === true;
    checks.push({
      name: `must_have:${mh}`,
      passed,
      details: passed ? `Must-have satisfied: ${mh}` : `Must-have NOT met: ${mh}`,
    });
  }

  // Check build passed
  const buildPassed = executionResult["buildPassed"] !== false;
  checks.push({
    name: "build_passed",
    passed: buildPassed,
    details: buildPassed ? "Build reported as passed" : "Build failed",
  });

  // Check verify command passed (if specified)
  if (task.verify) {
    const verifyPassed = executionResult["verifyPassed"] !== false;
    checks.push({
      name: "verify_passed",
      passed: verifyPassed,
      details: verifyPassed ? "Verification command passed" : "Verification command failed",
    });
  }

  const allPassed = checks.every(c => c.passed);
  const failedChecks = checks.filter(c => !c.passed);

  // Determine highest level that failed
  const level: "truths" | "artifacts" | "wiring" = failedChecks.some(c => c.name === "git_diff_exists")
    ? "truths"
    : failedChecks.some(c => c.name.startsWith("file_exists"))
      ? "artifacts"
      : "wiring";

  return {
    passed: allPassed,
    level: allPassed ? "wiring" : level,
    checks,
    summary: allPassed
      ? `All ${checks.length} checks passed`
      : `${failedChecks.length}/${checks.length} checks failed at ${level} level: ${failedChecks.map(c => c.name).join(", ")}`,
  };
}

// ─── Review Prompt Builder ───────────────────────────────────

export function buildReviewPrompt(task: Task, diff: string, verification: VerificationResult): string {
  const sections: string[] = [];

  sections.push("# Code Review Request");
  sections.push("");
  sections.push(`## Task: ${task.title} (${task.taskId})`);
  if (task.description) sections.push(task.description);
  sections.push("");

  sections.push("## Git Diff");
  sections.push("```diff");
  sections.push(diff.slice(0, 8000)); // Cap diff size for review context
  sections.push("```");
  sections.push("");

  if (task.mustHaves.length > 0) {
    sections.push("## Must-Haves to Verify");
    for (const m of task.mustHaves) {
      sections.push(`- ${m}`);
    }
    sections.push("");
  }

  sections.push("## Verification Results");
  for (const check of verification.checks) {
    sections.push(`- ${check.passed ? "✅" : "❌"} **${check.name}**: ${check.details}`);
  }
  sections.push("");

  sections.push("## Your Job");
  sections.push("1. Review the diff for correctness, style, and completeness");
  sections.push("2. Verify it meets the task requirements and must-haves");
  sections.push("3. Check for bugs, security issues, and missed edge cases");
  sections.push("");

  sections.push("## Output Format (REQUIRED)");
  sections.push("```json");
  sections.push("{");
  sections.push('  "passed": true | false,');
  sections.push('  "issues": [');
  sections.push('    { "severity": "error|warning|info", "file": "path", "message": "what", "suggestion": "fix" }');
  sections.push("  ],");
  sections.push('  "summary": "Overall assessment"');
  sections.push("}");
  sections.push("```");

  return sections.join("\n");
}

// ─── Git Operations ──────────────────────────────────────────

export function getGitDiff(workspaceRoot: string): string {
  try {
    // Show both staged and unstaged changes
    const diff = execSync("git diff HEAD --stat", { cwd: workspaceRoot, encoding: "utf-8", timeout: 10000 });
    return diff;
  } catch {
    return "";
  }
}

export function getGitDiffFull(workspaceRoot: string): string {
  try {
    return execSync("git diff HEAD", { cwd: workspaceRoot, encoding: "utf-8", timeout: 10000 });
  } catch {
    return "";
  }
}

export function getDiffFiles(workspaceRoot: string): string[] {
  try {
    const output = execSync("git diff HEAD --name-only", { cwd: workspaceRoot, encoding: "utf-8", timeout: 10000 });
    return output.trim().split("\n").filter(Boolean);
  } catch {
    return [];
  }
}

export function gitStageAll(workspaceRoot: string): void {
  execSync("git add -A", { cwd: workspaceRoot, timeout: 10000 });
}

export function gitCommit(workspaceRoot: string, message: string): string | null {
  try {
    gitStageAll(workspaceRoot);
    // Check if there's anything to commit
    const status = execSync("git status --porcelain", { cwd: workspaceRoot, encoding: "utf-8", timeout: 10000 });
    if (!status.trim()) return null;

    execSync(`git commit -m ${JSON.stringify(message)}`, { cwd: workspaceRoot, timeout: 30000 });
    const hash = execSync("git rev-parse --short HEAD", { cwd: workspaceRoot, encoding: "utf-8", timeout: 10000 }).trim();
    return hash;
  } catch (error) {
    log.warn(`Git commit failed: ${error instanceof Error ? error.message : String(error)}`);
    return null;
  }
}

/**
 * Generate conventional commit message from task metadata.
 * Format: type(scope): description
 */
export function buildCommitMessage(task: Task, phaseName: string): string {
  // Determine commit type from task content
  const titleLower = task.title.toLowerCase();
  let type = "feat";
  if (titleLower.match(/\b(fix|bug|patch|hotfix)\b/)) type = "fix";
  else if (titleLower.match(/\b(test|spec|coverage)\b/)) type = "test";
  else if (titleLower.match(/\b(doc|readme|comment)\b/)) type = "docs";
  else if (titleLower.match(/\b(refactor|clean|reorganize)\b/)) type = "refactor";
  else if (titleLower.match(/\b(build|ci|deploy|docker)\b/)) type = "chore";
  else if (titleLower.match(/\b(style|format|lint)\b/)) type = "style";
  else if (titleLower.match(/\b(perf|optimize|speed)\b/)) type = "perf";

  // Scope from phase name (kebab-case, truncated)
  const scope = phaseName.toLowerCase().replace(/[^a-z0-9]+/g, "-").slice(0, 20);

  // Description from task title (max 72 chars for commit subject line)
  const desc = task.title.length > 60 ? task.title.slice(0, 57) + "..." : task.title;

  return `${type}(${scope}): ${desc}`;
}

// ─── Deviation Classifier ────────────────────────────────────

export function classifyDeviation(
  description: string,
  rules: DeviationRule[],
): { level: DeviationLevel; matchedRule: DeviationRule | null } {
  for (const rule of rules) {
    if (rule.pattern.test(description)) {
      return { level: rule.level, matchedRule: rule };
    }
  }
  return { level: "warn", matchedRule: null };
}

// ─── Checkpoint Types ────────────────────────────────────────

export type CheckpointType = "human-verify" | "decision" | "human-action";

export interface Checkpoint {
  type: CheckpointType;
  taskId: string;
  description: string;
  /** What the human needs to verify/decide/do */
  prompt: string;
  /** Whether execution should block until resolved */
  blocking: boolean;
}

/**
 * Detect if a task requires a checkpoint before/after execution.
 * Returns null if no checkpoint needed.
 */
export function detectCheckpoint(task: Task): Checkpoint | null {
  const titleLower = task.title.toLowerCase();
  const descLower = (task.description ?? "").toLowerCase();
  const combined = `${titleLower} ${descLower}`;

  // Human-action: physical things only humans can do
  if (combined.match(/\b(deploy|release|publish|push to prod)\b/) || combined.match(/merge\b.*\bto main\b/) || combined.match(/merge\b.*\bmain\b/)) {
    return {
      type: "human-action",
      taskId: task.taskId,
      description: "This task requires human action",
      prompt: `Task "${task.title}" requires you to: ${task.action ?? task.description ?? task.title}`,
      blocking: true,
    };
  }

  // Decision: architecture/design choices that need human judgment
  if (combined.match(/\b(choose|decide|which approach|tradeoff|architecture|design decision)\b/)) {
    return {
      type: "decision",
      taskId: task.taskId,
      description: "This task requires a design decision",
      prompt: `Task "${task.title}" needs your input: ${task.description ?? task.title}`,
      blocking: true,
    };
  }

  // Human-verify: security, data migrations, breaking changes
  if (combined.match(/\b(migration|security|credential|breaking change|data loss|irreversible)\b/)) {
    return {
      type: "human-verify",
      taskId: task.taskId,
      description: "This task output needs human verification",
      prompt: `Please verify the output of "${task.title}" before proceeding`,
      blocking: true,
    };
  }

  return null;
}

// ─── Task Executor ───────────────────────────────────────────

export class TaskExecutor {
  constructor(private config: TaskExecutorConfig) {}

  /**
   * Execute a single task through the full pipeline:
   * 1. Check for pre-execution checkpoint
   * 2. Dispatch to coder sub-agent
   * 3. Verify work was done (three-level verification)
   * 4. Optional reviewer loop (max rounds)
   * 5. Git commit if verification passes
   */
  async executeTask(
    task: Task,
    projectGoal: string,
    phaseGoal: string,
    phaseName: string,
    dispatchFn: (prompt: string, role: string, timeoutSeconds: number) => Promise<string>,
    reviewFn?: (prompt: string) => Promise<string>,
  ): Promise<TaskExecutionResult> {
    const start = Date.now();
    const reviewRounds: ReviewRound[] = [];
    let commitHash: string | null = null;

    try {
      // Step 0: Check for checkpoint
      const checkpoint = detectCheckpoint(task);
      if (checkpoint?.blocking) {
        log.info(`Task ${task.taskId} requires ${checkpoint.type} checkpoint — skipping for now`);
        return {
          taskId: task.taskId,
          status: "skipped",
          verification: null,
          reviewRounds: [],
          commitHash: null,
          duration: Date.now() - start,
          error: `Blocked: ${checkpoint.type} checkpoint — ${checkpoint.description}`,
        };
      }

      // Step 1: Build and dispatch task
      const prompt = buildTaskPrompt(task, projectGoal, phaseGoal);
      const role = selectTaskRole(task);
      const timeout = task.contextBudget ? Math.max(180, Math.ceil(task.contextBudget / 100)) : 300;

      log.info(`Executing task ${task.taskId}: "${task.title}" (role=${role}, timeout=${timeout}s)`);
      const response = await dispatchFn(prompt, role, timeout);

      // Step 2: Parse response
      const executionResult = parseExecutionResponse(response);
      if (!executionResult) {
        return {
          taskId: task.taskId,
          status: "failed",
          verification: null,
          reviewRounds: [],
          commitHash: null,
          duration: Date.now() - start,
          error: "Failed to parse sub-agent response — no structured JSON found",
        };
      }

      // Step 3: Verify (three-level)
      const verification = verifyTaskCompletion(task, executionResult, this.config.workspaceRoot);

      // If truths-level failure (no git changes), fail immediately
      if (!verification.passed && verification.level === "truths") {
        log.warn(`Task ${task.taskId} failed truths-level verification: no actual changes detected`);
        return {
          taskId: task.taskId,
          status: "failed",
          verification,
          reviewRounds: [],
          commitHash: null,
          duration: Date.now() - start,
          error: "No actual changes detected — sub-agent did not write code",
        };
      }

      // Step 4: Review loop (if enabled and there's something to review)
      if (this.config.enableReview && reviewFn && verification.level !== "truths") {
        const diff = getGitDiffFull(this.config.workspaceRoot);
        if (diff.trim()) {
          for (let round = 0; round < this.config.maxReviewRounds; round++) {
            const reviewPrompt = buildReviewPrompt(task, diff, verification);
            const reviewResponse = await reviewFn(reviewPrompt);
            const reviewResult = parseReviewResponse(reviewResponse);

            reviewRounds.push({
              round: round + 1,
              issues: reviewResult?.issues ?? [],
              passed: reviewResult?.passed ?? false,
            });

            if (reviewResult?.passed) {
              log.info(`Task ${task.taskId} passed review on round ${round + 1}`);
              break;
            }

            // If review failed with errors, dispatch fix attempt
            const errors = (reviewResult?.issues ?? []).filter(i => i.severity === "error");
            if (errors.length > 0 && round < this.config.maxReviewRounds - 1) {
              log.info(`Task ${task.taskId} has ${errors.length} review errors — dispatching fix (round ${round + 2})`);
              const fixPrompt = buildFixPrompt(task, errors, diff);
              await dispatchFn(fixPrompt, role, timeout);
            }
          }
        }
      }

      // Step 5: Git commit
      if (this.config.enableGitCommits && (verification.passed || executionResult["status"] === "success")) {
        const message = buildCommitMessage(task, phaseName);
        commitHash = gitCommit(this.config.workspaceRoot, message);
        if (commitHash) {
          log.info(`Task ${task.taskId} committed: ${commitHash} — ${message}`);
        }
      }

      const status = verification.passed ? "success" : (executionResult["status"] === "partial" ? "partial" : "failed");

      return {
        taskId: task.taskId,
        status,
        verification,
        reviewRounds,
        commitHash,
        duration: Date.now() - start,
      };
    } catch (error) {
      return {
        taskId: task.taskId,
        status: "failed",
        verification: null,
        reviewRounds,
        commitHash: null,
        duration: Date.now() - start,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }
}

// ─── Helpers ─────────────────────────────────────────────────

function selectTaskRole(task: Task): string {
  const combined = `${task.title} ${task.description ?? ""} ${task.action ?? ""}`.toLowerCase();

  if (combined.match(/\b(test|spec|coverage)\b/)) return "coder"; // Tests need write access
  if (combined.match(/\b(research|investigate|find out|look up)\b/)) return "researcher";
  if (combined.match(/\b(explore|trace|grep|find where)\b/)) return "explorer";
  if (combined.match(/\b(run|execute|deploy|build|check)\b/) && !combined.match(/\b(implement|create|write)\b/)) return "runner";

  return "coder"; // Default: write code
}

function parseExecutionResponse(response: string): Record<string, unknown> | null {
  // Try fenced JSON blocks
  const jsonBlocks = [...response.matchAll(/```json\s*\n([\s\S]*?)\n```/g)];
  if (jsonBlocks.length > 0) {
    const lastBlock = jsonBlocks[jsonBlocks.length - 1];
    if (lastBlock?.[1]) {
      try {
        return JSON.parse(lastBlock[1].trim()) as Record<string, unknown>;
      } catch { /* fall through */ }
    }
  }

  // Try raw JSON at end
  const trimmed = response.trim();
  const lastBrace = trimmed.lastIndexOf("}");
  if (lastBrace > 0) {
    let depth = 0;
    for (let i = lastBrace; i >= 0; i--) {
      if (trimmed[i] === "}") depth++;
      if (trimmed[i] === "{") {
        depth--;
        if (depth === 0) {
          try {
            return JSON.parse(trimmed.slice(i, lastBrace + 1)) as Record<string, unknown>;
          } catch { /* fall through */ }
        }
      }
    }
  }

  return null;
}

function parseReviewResponse(response: string): { passed: boolean; issues: ReviewIssue[]; summary: string } | null {
  const parsed = parseExecutionResponse(response);
  if (!parsed) return null;

  return {
    passed: parsed["passed"] === true,
    issues: (parsed["issues"] as ReviewIssue[]) ?? [],
    summary: (parsed["summary"] as string) ?? "",
  };
}

function buildFixPrompt(task: Task, errors: ReviewIssue[], _diff: string): string {
  const sections: string[] = [];

  sections.push("# Fix Review Issues");
  sections.push("");
  sections.push(`## Task: ${task.title} (${task.taskId})`);
  sections.push("");
  sections.push("The reviewer found the following errors in your previous implementation:");
  sections.push("");

  for (const error of errors) {
    sections.push(`### ${error.file ?? "General"}`);
    sections.push(`**Issue:** ${error.message}`);
    if (error.suggestion) sections.push(`**Suggestion:** ${error.suggestion}`);
    sections.push("");
  }

  sections.push("## Rules");
  sections.push("1. Fix ONLY the issues listed above");
  sections.push("2. Do NOT introduce new features or refactors");
  sections.push("3. Build must pass after fixes");
  sections.push("4. Git add your changes");
  sections.push("");

  sections.push("## Output Format (REQUIRED)");
  sections.push("```json");
  sections.push("{");
  sections.push('  "status": "success" | "partial" | "failed",');
  sections.push('  "summary": "What you fixed",');
  sections.push('  "filesChanged": ["path/to/file.ts"],');
  sections.push('  "buildPassed": true,');
  sections.push('  "confidence": 0.9');
  sections.push("}");
  sections.push("```");

  return sections.join("\n");
}
