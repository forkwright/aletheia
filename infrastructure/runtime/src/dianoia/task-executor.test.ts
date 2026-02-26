// TaskExecutor unit tests — verification, review loops, git commits, deviation rules, checkpoints
import { describe, expect, it } from "vitest";
import {
  buildCommitMessage,
  buildTaskPrompt,
  buildReviewPrompt,
  classifyDeviation,
  DEFAULT_DEVIATION_RULES,
  detectCheckpoint,
  verifyTaskCompletion,
  type VerificationResult,
  TaskExecutor,
  type TaskExecutorConfig,
} from "./task-executor.js";
import type { Task } from "./task-store.js";

// ─── Fixtures ────────────────────────────────────────────────

function makeTask(overrides: Partial<Task> = {}): Task {
  return {
    id: "uuid-1",
    projectId: "proj-1",
    phaseId: "phase-1",
    parentId: null,
    taskId: "PROJ-001",
    title: "Implement user authentication",
    description: "Add login/logout endpoints with JWT tokens",
    status: "pending",
    priority: "high",
    action: "Create auth middleware and login route",
    verify: "npx vitest run src/auth.test.ts",
    files: ["src/auth/middleware.ts", "src/auth/routes.ts"],
    mustHaves: ["JWT token generation", "Password hashing", "Rate limiting"],
    contextBudget: 8000,
    blockedBy: [],
    blocks: [],
    depth: 0,
    assignee: null,
    tags: [],
    completedAt: null,
    createdAt: "2026-02-26T00:00:00Z",
    updatedAt: "2026-02-26T00:00:00Z",
    ...overrides,
  };
}

// ─── buildTaskPrompt ─────────────────────────────────────────

describe("buildTaskPrompt", () => {
  it("includes task title and ID", () => {
    const task = makeTask();
    const prompt = buildTaskPrompt(task, "Build auth system", "Implement core auth");
    expect(prompt).toContain("Implement user authentication");
    expect(prompt).toContain("PROJ-001");
  });

  it("includes action, files, and must-haves", () => {
    const task = makeTask();
    const prompt = buildTaskPrompt(task, "goal", "phase");
    expect(prompt).toContain("Create auth middleware");
    expect(prompt).toContain("src/auth/middleware.ts");
    expect(prompt).toContain("JWT token generation");
  });

  it("includes verification command", () => {
    const task = makeTask();
    const prompt = buildTaskPrompt(task, "goal", "phase");
    expect(prompt).toContain("npx vitest run src/auth.test.ts");
  });

  it("includes output format requirement", () => {
    const task = makeTask();
    const prompt = buildTaskPrompt(task, "goal", "phase");
    expect(prompt).toContain('"status"');
    expect(prompt).toContain('"filesChanged"');
    expect(prompt).toContain('"mustHaveResults"');
  });

  it("includes rules about actually writing code", () => {
    const task = makeTask();
    const prompt = buildTaskPrompt(task, "goal", "phase");
    expect(prompt).toContain("Do the work");
    expect(prompt).toContain("Do NOT just describe");
  });

  it("handles task with no optional fields", () => {
    const task = makeTask({
      description: "",
      action: null,
      verify: null,
      files: [],
      mustHaves: [],
    });
    const prompt = buildTaskPrompt(task, "goal", "phase");
    expect(prompt).toContain("PROJ-001");
    expect(prompt).not.toContain("## Relevant Files");
    expect(prompt).not.toContain("## Must-Haves");
  });
});

// ─── verifyTaskCompletion ────────────────────────────────────

describe("verifyTaskCompletion", () => {
  it("fails truths level when no git changes and response claims changes", () => {
    const task = makeTask();
    // Workspace doesn't exist / no git — getGitDiff returns ""
    const result = verifyTaskCompletion(task, {
      filesChanged: ["src/auth/middleware.ts"],
      buildPassed: true,
      mustHaveResults: {},
    }, "/nonexistent/workspace");

    expect(result.passed).toBe(false);
    expect(result.level).toBe("truths");
    expect(result.checks.find(c => c.name === "git_diff_exists")?.passed).toBe(false);
  });

  it("checks must-have results", () => {
    const task = makeTask({ files: [], mustHaves: ["JWT token generation", "Password hashing"] });
    const result = verifyTaskCompletion(task, {
      buildPassed: true,
      mustHaveResults: { "JWT token generation": true, "Password hashing": false },
    }, "/nonexistent/workspace");

    const jwtCheck = result.checks.find(c => c.name === "must_have:JWT token generation");
    const hashCheck = result.checks.find(c => c.name === "must_have:Password hashing");
    expect(jwtCheck?.passed).toBe(true);
    expect(hashCheck?.passed).toBe(false);
  });

  it("checks build status", () => {
    const task = makeTask({ files: [], mustHaves: [] });
    const result = verifyTaskCompletion(task, { buildPassed: false }, "/nonexistent/workspace");
    const buildCheck = result.checks.find(c => c.name === "build_passed");
    expect(buildCheck?.passed).toBe(false);
  });

  it("checks verify command status when specified", () => {
    const task = makeTask({ files: [], mustHaves: [] });
    const result = verifyTaskCompletion(task, { verifyPassed: false }, "/nonexistent/workspace");
    const verifyCheck = result.checks.find(c => c.name === "verify_passed");
    expect(verifyCheck?.passed).toBe(false);
  });

  it("skips verify check when no verify command on task", () => {
    const task = makeTask({ verify: null, files: [], mustHaves: [] });
    const result = verifyTaskCompletion(task, {}, "/nonexistent/workspace");
    const verifyCheck = result.checks.find(c => c.name === "verify_passed");
    expect(verifyCheck).toBeUndefined();
  });
});

// ─── buildCommitMessage ──────────────────────────────────────

describe("buildCommitMessage", () => {
  it("generates feat type by default", () => {
    const task = makeTask({ title: "Add user authentication" });
    const message = buildCommitMessage(task, "Core Auth");
    expect(message).toBe("feat(core-auth): Add user authentication");
  });

  it("detects fix type", () => {
    const task = makeTask({ title: "Fix login bug with expired tokens" });
    const message = buildCommitMessage(task, "Auth");
    expect(message).toMatch(/^fix\(auth\)/);
  });

  it("detects test type", () => {
    const task = makeTask({ title: "Add test coverage for auth middleware" });
    const message = buildCommitMessage(task, "Auth");
    expect(message).toMatch(/^test\(auth\)/);
  });

  it("detects docs type", () => {
    const task = makeTask({ title: "Update README with setup instructions" });
    const message = buildCommitMessage(task, "Documentation");
    expect(message).toMatch(/^docs\(documentation\)/);
  });

  it("detects refactor type", () => {
    const task = makeTask({ title: "Refactor auth module for clarity" });
    const message = buildCommitMessage(task, "Auth");
    expect(message).toMatch(/^refactor\(auth\)/);
  });

  it("truncates long titles", () => {
    const task = makeTask({ title: "A".repeat(80) });
    const message = buildCommitMessage(task, "Phase");
    expect(message.length).toBeLessThanOrEqual(100);
    expect(message).toContain("...");
  });

  it("kebab-cases and truncates scope", () => {
    const task = makeTask({ title: "Add feature" });
    const message = buildCommitMessage(task, "Very Long Phase Name With Spaces");
    expect(message).toMatch(/^feat\(very-long-phase-name/);
    // Scope truncated at 20 chars
    const scope = message.match(/\(([^)]+)\)/)?.[1] ?? "";
    expect(scope.length).toBeLessThanOrEqual(20);
  });
});

// ─── classifyDeviation ───────────────────────────────────────

describe("classifyDeviation", () => {
  it("classifies lint issues as auto", () => {
    const { level } = classifyDeviation("Fix lint errors in auth module", DEFAULT_DEVIATION_RULES);
    expect(level).toBe("auto");
  });

  it("classifies format issues as auto", () => {
    const { level } = classifyDeviation("Format code with prettier", DEFAULT_DEVIATION_RULES);
    expect(level).toBe("auto");
  });

  it("classifies test failures as warn", () => {
    const { level } = classifyDeviation("Test fail in auth module", DEFAULT_DEVIATION_RULES);
    expect(level).toBe("warn");
  });

  it("classifies API changes as ask", () => {
    const { level } = classifyDeviation("API change to user endpoint", DEFAULT_DEVIATION_RULES);
    expect(level).toBe("ask");
  });

  it("classifies schema changes as ask", () => {
    const { level } = classifyDeviation("Schema migration for users table", DEFAULT_DEVIATION_RULES);
    expect(level).toBe("ask");
  });

  it("classifies architecture changes as block", () => {
    const { level } = classifyDeviation("Architecture redesign of auth system", DEFAULT_DEVIATION_RULES);
    expect(level).toBe("block");
  });

  it("classifies security changes as block", () => {
    const { level } = classifyDeviation("Security vulnerability in credential storage", DEFAULT_DEVIATION_RULES);
    expect(level).toBe("block");
  });

  it("defaults to warn for unmatched patterns", () => {
    const { level } = classifyDeviation("Something completely unrelated", DEFAULT_DEVIATION_RULES);
    expect(level).toBe("warn");
  });
});

// ─── detectCheckpoint ────────────────────────────────────────

describe("detectCheckpoint", () => {
  it("detects human-action for deploy tasks", () => {
    const task = makeTask({ title: "Deploy to production" });
    const checkpoint = detectCheckpoint(task);
    expect(checkpoint).not.toBeNull();
    expect(checkpoint!.type).toBe("human-action");
    expect(checkpoint!.blocking).toBe(true);
  });

  it("detects decision for architecture choices", () => {
    const task = makeTask({ title: "Choose which approach for auth" });
    const checkpoint = detectCheckpoint(task);
    expect(checkpoint).not.toBeNull();
    expect(checkpoint!.type).toBe("decision");
  });

  it("detects human-verify for security-sensitive tasks", () => {
    const task = makeTask({ title: "Update credential storage", description: "Migration of existing credentials" });
    const checkpoint = detectCheckpoint(task);
    expect(checkpoint).not.toBeNull();
    expect(checkpoint!.type).toBe("human-verify");
  });

  it("returns null for normal coding tasks", () => {
    const task = makeTask({ title: "Implement user login form" });
    const checkpoint = detectCheckpoint(task);
    expect(checkpoint).toBeNull();
  });

  it("detects human-action for merge to main", () => {
    const task = makeTask({ title: "Merge feature branch to main" });
    const checkpoint = detectCheckpoint(task);
    expect(checkpoint).not.toBeNull();
    expect(checkpoint!.type).toBe("human-action");
  });
});

// ─── buildReviewPrompt ───────────────────────────────────────

describe("buildReviewPrompt", () => {
  it("includes task context and diff", () => {
    const task = makeTask();
    const verification: VerificationResult = {
      passed: true,
      level: "wiring",
      checks: [{ name: "build_passed", passed: true, details: "Build OK" }],
      summary: "All checks passed",
    };
    const prompt = buildReviewPrompt(task, "diff content here", verification);
    expect(prompt).toContain("Code Review Request");
    expect(prompt).toContain("diff content here");
    expect(prompt).toContain("PROJ-001");
    expect(prompt).toContain("✅");
  });

  it("truncates large diffs", () => {
    const task = makeTask();
    const verification: VerificationResult = {
      passed: true, level: "wiring", checks: [], summary: "ok",
    };
    const largeDiff = "x".repeat(10000);
    const prompt = buildReviewPrompt(task, largeDiff, verification);
    // Diff capped at 8000 chars
    expect(prompt.length).toBeLessThan(largeDiff.length + 1000);
  });
});

// ─── TaskExecutor integration ────────────────────────────────

describe("TaskExecutor", () => {
  const config: TaskExecutorConfig = {
    workspaceRoot: "/nonexistent/test",
    maxReviewRounds: 3,
    enableGitCommits: false, // Don't actually commit in tests
    enableReview: false,
    deviationRules: DEFAULT_DEVIATION_RULES,
  };

  it("fails task when sub-agent returns no structured JSON", async () => {
    const executor = new TaskExecutor(config);
    const task = makeTask({ files: [], mustHaves: [] });

    const result = await executor.executeTask(
      task, "project goal", "phase goal", "Phase 1",
      async () => "I analyzed the requirements and here is my plan...", // No JSON!
    );

    expect(result.status).toBe("failed");
    expect(result.error).toContain("no structured JSON");
  });

  it("fails task when sub-agent claims success but no git changes", async () => {
    const executor = new TaskExecutor(config);
    const task = makeTask({ files: [], mustHaves: [] });

    const result = await executor.executeTask(
      task, "project goal", "phase goal", "Phase 1",
      async () => '```json\n{"status":"success","summary":"Done","filesChanged":["foo.ts"],"buildPassed":true,"confidence":0.9}\n```',
    );

    expect(result.status).toBe("failed");
    expect(result.verification?.level).toBe("truths");
    expect(result.error).toContain("did not write code");
  });

  it("skips tasks that require human-action checkpoints", async () => {
    const executor = new TaskExecutor(config);
    const task = makeTask({ title: "Deploy to production" });

    const result = await executor.executeTask(
      task, "goal", "phase", "Phase 1",
      async () => { throw new Error("should not be called"); },
    );

    expect(result.status).toBe("skipped");
    expect(result.error).toContain("checkpoint");
  });

  it("handles dispatch errors gracefully", async () => {
    const executor = new TaskExecutor(config);
    const task = makeTask({ files: [], mustHaves: [] });

    const result = await executor.executeTask(
      task, "goal", "phase", "Phase 1",
      async () => { throw new Error("Network timeout"); },
    );

    expect(result.status).toBe("failed");
    expect(result.error).toContain("Network timeout");
  });

  it("runs review loop when enabled", async () => {
    const reviewConfig = { ...config, enableReview: true };
    const executor = new TaskExecutor(reviewConfig);
    const task = makeTask({ files: [], mustHaves: [] });

    let reviewCallCount = 0;
    const result = await executor.executeTask(
      task, "goal", "phase", "Phase 1",
      // Coder returns success (but no git changes, so verification fails at truths)
      async () => '```json\n{"status":"success","summary":"Done","filesChanged":[],"buildPassed":true,"confidence":0.9}\n```',
      // Reviewer
      async () => {
        reviewCallCount++;
        return '```json\n{"passed":true,"issues":[],"summary":"LGTM"}\n```';
      },
    );

    // Task fails at truths level (no git changes), review should not run
    expect(result.status).toBe("failed");
    expect(reviewCallCount).toBe(0);
  });
});
