// Runtime code patching — agents propose changes to their own source, gated by tsc + vitest
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";
import { join, dirname } from "node:path";
import { createLogger } from "../../koina/logger.js";
import type { ToolContext, ToolHandler } from "../registry.js";

const log = createLogger("organon.propose-patch");

const PATCHABLE_DIRS = ["organon/", "nous/", "distillation/", "daemon/"];
const FORBIDDEN_DIRS = ["pylon/", "koina/", "semeion/", "taxis/"];
const RATE_LIMIT_MS = 3600_000;
const DAILY_LIMIT = 3;
const TSC_TIMEOUT = 60_000;
const TEST_TIMEOUT = 60_000;
const BUILD_TIMEOUT = 120_000;

export interface PatchRecord {
  id: string;
  nousId: string;
  filePath: string;
  description: string;
  oldText: string;
  newText: string;
  status: "applied" | "failed_tsc" | "failed_test" | "failed_review" | "rolled_back";
  tscOutput?: string;
  testOutput?: string;
  reviewedBy?: string;
  appliedAt: string;
  backupContent: string;
}

interface PatchHistory {
  patches: PatchRecord[];
}

function getRuntimeDir(): string {
  return join(dirname(new URL(import.meta.url).pathname), "..", "..");
}

function getSrcDir(): string {
  return join(getRuntimeDir(), "src");
}

function getHistoryPath(workspace: string): string {
  const dir = join(workspace, "..", "..", "shared", "patches");
  mkdirSync(dir, { recursive: true });
  return join(dir, "history.json");
}

function loadHistory(workspace: string): PatchHistory {
  const path = getHistoryPath(workspace);
  if (!existsSync(path)) return { patches: [] };
  try {
    return JSON.parse(readFileSync(path, "utf-8")) as PatchHistory;
  } catch { /* git check failed */
    return { patches: [] };
  }
}

function saveHistory(workspace: string, history: PatchHistory): void {
  writeFileSync(getHistoryPath(workspace), JSON.stringify(history, null, 2) + "\n");
}

export function isPathAllowed(filePath: string): { allowed: boolean; reason?: string } {
  const normalized = filePath.replace(/\\/g, "/");
  for (const forbidden of FORBIDDEN_DIRS) {
    if (normalized.startsWith(forbidden)) {
      return { allowed: false, reason: `Path in forbidden directory: ${forbidden}` };
    }
  }
  for (const allowed of PATCHABLE_DIRS) {
    if (normalized.startsWith(allowed)) return { allowed: true };
  }
  return { allowed: false, reason: `Path not in patchable directories: ${PATCHABLE_DIRS.join(", ")}` };
}

export function checkRateLimit(
  history: PatchHistory,
  nousId: string,
): { allowed: boolean; reason?: string } {
  const now = Date.now();
  const dayStart = now - 86_400_000;

  const recentByAgent = history.patches.filter(
    (p) => p.nousId === nousId && new Date(p.appliedAt).getTime() > now - RATE_LIMIT_MS,
  );
  if (recentByAgent.length > 0) {
    return { allowed: false, reason: "Rate limited: 1 patch per hour per agent" };
  }

  const dailyTotal = history.patches.filter(
    (p) => p.status === "applied" && new Date(p.appliedAt).getTime() > dayStart,
  );
  if (dailyTotal.length >= DAILY_LIMIT) {
    return { allowed: false, reason: `Rate limited: ${DAILY_LIMIT} patches per day` };
  }

  return { allowed: true };
}

function runTsc(runtimeDir: string): { ok: boolean; output: string } {
  try {
    execSync("npx tsc --noEmit", {
      cwd: runtimeDir,
      timeout: TSC_TIMEOUT,
      stdio: "pipe",
      env: { ...process.env, NODE_NO_WARNINGS: "1" },
    });
    return { ok: true, output: "TypeScript compilation passed" };
  } catch (err: unknown) {
    const e = err as { stderr?: Buffer; stdout?: Buffer };
    const output = ((e.stdout?.toString() ?? "") + "\n" + (e.stderr?.toString() ?? "")).trim();
    return { ok: false, output: output.slice(0, 2000) };
  }
}

function runTests(runtimeDir: string, testFile: string): { ok: boolean; output: string } {
  try {
    const output = execSync(`npx vitest run ${testFile}`, {
      cwd: runtimeDir,
      timeout: TEST_TIMEOUT,
      stdio: "pipe",
      env: { ...process.env, NODE_NO_WARNINGS: "1" },
    }).toString();
    return { ok: true, output: output.slice(0, 2000) };
  } catch (err: unknown) {
    const e = err as { stderr?: Buffer; stdout?: Buffer };
    const output = ((e.stdout?.toString() ?? "") + "\n" + (e.stderr?.toString() ?? "")).trim();
    return { ok: false, output: output.slice(0, 2000) };
  }
}

export function createPatchTools(): ToolHandler[] {
  const proposePatch: ToolHandler = {
    definition: {
      name: "propose_patch",
      description:
        "Propose a code patch to the runtime source. The patch is validated by TypeScript " +
        "compilation and the test suite before being applied.\n\n" +
        "PATCHABLE: organon/, nous/, distillation/, daemon/\n" +
        "FORBIDDEN: pylon/, koina/, semeion/, taxis/\n" +
        "RATE LIMIT: 1 patch/hour/agent, 3 patches/day total\n\n" +
        "Provide the file path relative to src/, the old text to replace, and the new text.",
      input_schema: {
        type: "object",
        properties: {
          file: {
            type: "string",
            description: "File path relative to infrastructure/runtime/src/ (e.g. 'nous/recall.ts')",
          },
          description: {
            type: "string",
            description: "What the change does and why",
          },
          old_text: {
            type: "string",
            description: "Exact text to find in the file",
          },
          new_text: {
            type: "string",
            description: "Replacement text",
          },
        },
        required: ["file", "description", "old_text", "new_text"],
      },
    },
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      const filePath = input["file"] as string;
      const description = input["description"] as string;
      const oldText = input["old_text"] as string;
      const newText = input["new_text"] as string;

      // Validate path
      const pathCheck = isPathAllowed(filePath);
      if (!pathCheck.allowed) {
        return JSON.stringify({ error: pathCheck.reason });
      }

      // Check rate limit
      const history = loadHistory(context.workspace);
      const rateCheck = checkRateLimit(history, context.nousId);
      if (!rateCheck.allowed) {
        return JSON.stringify({ error: rateCheck.reason });
      }

      const runtimeDir = getRuntimeDir();
      const srcDir = getSrcDir();
      const absPath = join(srcDir, filePath);

      if (!existsSync(absPath)) {
        return JSON.stringify({ error: `File not found: ${filePath}` });
      }

      const originalContent = readFileSync(absPath, "utf-8");
      if (!originalContent.includes(oldText)) {
        return JSON.stringify({ error: "old_text not found in file" });
      }

      const patchedContent = originalContent.replace(oldText, newText);
      const patchId = `patch-${Date.now().toString(36)}`;

      // Apply patch temporarily
      writeFileSync(absPath, patchedContent, "utf-8");

      // Run TypeScript compiler
      const tscResult = runTsc(runtimeDir);
      if (!tscResult.ok) {
        writeFileSync(absPath, originalContent, "utf-8");
        const record: PatchRecord = {
          id: patchId, nousId: context.nousId, filePath, description,
          oldText, newText, status: "failed_tsc",
          tscOutput: tscResult.output, appliedAt: new Date().toISOString(),
          backupContent: originalContent,
        };
        history.patches.push(record);
        saveHistory(context.workspace, history);
        log.warn(`Patch ${patchId} rejected: tsc failed`);
        return JSON.stringify({ error: "TypeScript compilation failed", output: tscResult.output });
      }

      // Run colocated tests if they exist
      const testFile = `src/${filePath.replace(".ts", ".test.ts")}`;
      const absTestFile = join(runtimeDir, testFile);
      let testResult = { ok: true, output: "no colocated tests" };
      if (existsSync(absTestFile)) {
        testResult = runTests(runtimeDir, testFile);
        if (!testResult.ok) {
          writeFileSync(absPath, originalContent, "utf-8");
          const record: PatchRecord = {
            id: patchId, nousId: context.nousId, filePath, description,
            oldText, newText, status: "failed_test",
            tscOutput: tscResult.output, testOutput: testResult.output,
            appliedAt: new Date().toISOString(), backupContent: originalContent,
          };
          history.patches.push(record);
          saveHistory(context.workspace, history);
          log.warn(`Patch ${patchId} rejected: tests failed`);
          return JSON.stringify({ error: "Tests failed after patch", output: testResult.output });
        }
      }

      // Rebuild
      try {
        execSync("npx tsdown", {
          cwd: runtimeDir,
          timeout: BUILD_TIMEOUT,
          stdio: "pipe",
        });
      } catch (err) {
        writeFileSync(absPath, originalContent, "utf-8");
        const msg = err instanceof Error ? err.message : String(err);
        log.warn(`Patch ${patchId} rejected: build failed — ${msg}`);
        return JSON.stringify({ error: "Build failed after patch", output: msg.slice(0, 1000) });
      }

      // Record success
      const record: PatchRecord = {
        id: patchId, nousId: context.nousId, filePath, description,
        oldText, newText, status: "applied",
        tscOutput: tscResult.output, testOutput: testResult.output,
        appliedAt: new Date().toISOString(), backupContent: originalContent,
      };
      history.patches.push(record);
      saveHistory(context.workspace, history);

      log.info(`Patch ${patchId} applied by ${context.nousId}: ${description}`);

      // Signal the process to restart gracefully
      setTimeout(() => {
        log.info("Triggering graceful restart after patch");
        process.kill(process.pid, "SIGTERM");
      }, 2000);

      return JSON.stringify({
        applied: true,
        patchId,
        file: filePath,
        description,
        tsc: "passed",
        tests: testResult.output.slice(0, 200),
        note: "Process will restart in ~2s to apply changes",
      });
    },
  };

  const rollbackPatch: ToolHandler = {
    definition: {
      name: "rollback_patch",
      description:
        "Rollback a previously applied code patch by restoring the original file content.",
      input_schema: {
        type: "object",
        properties: {
          patch_id: {
            type: "string",
            description: "Patch ID to rollback (from propose_patch result)",
          },
        },
        required: ["patch_id"],
      },
    },
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      const patchId = input["patch_id"] as string;
      const history = loadHistory(context.workspace);
      const patch = history.patches.find((p) => p.id === patchId);

      if (!patch) {
        return JSON.stringify({ error: "Patch not found" });
      }
      if (patch.status !== "applied") {
        return JSON.stringify({ error: `Cannot rollback patch with status: ${patch.status}` });
      }

      const srcDir = getSrcDir();
      const absPath = join(srcDir, patch.filePath);

      writeFileSync(absPath, patch.backupContent, "utf-8");

      // Rebuild
      const runtimeDir = getRuntimeDir();
      try {
        execSync("npx tsdown", {
          cwd: runtimeDir,
          timeout: BUILD_TIMEOUT,
          stdio: "pipe",
        });
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        log.warn(`Rollback rebuild failed: ${msg}`);
        return JSON.stringify({ error: "Rollback applied but rebuild failed", output: msg.slice(0, 1000) });
      }

      patch.status = "rolled_back";
      saveHistory(context.workspace, history);
      log.info(`Patch ${patchId} rolled back by ${context.nousId}`);

      setTimeout(() => {
        log.info("Triggering graceful restart after rollback");
        process.kill(process.pid, "SIGTERM");
      }, 2000);

      return JSON.stringify({
        rolledBack: true,
        patchId,
        file: patch.filePath,
        note: "Process will restart in ~2s to apply rollback",
      });
    },
  };

  return [proposePatch, rollbackPatch];
}
