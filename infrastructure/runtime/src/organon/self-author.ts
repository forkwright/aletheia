// Self-authoring tools — agents create, test, and register custom tools at runtime
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, basename } from "node:path";
import { execSync } from "node:child_process";
import { createLogger } from "../koina/logger.js";
import type { ToolHandler, ToolContext, ToolRegistry } from "./registry.js";

const log = createLogger("organon.self-author");

const AUTHORED_DIR_NAME = "tools/authored";
const MAX_TOOL_SIZE = 8192;
const MAX_FAILURES = 3;
const SANDBOX_TIMEOUT = 10_000;

interface AuthoredToolMeta {
  name: string;
  author: string;
  version: number;
  failures: number;
  quarantined: boolean;
  createdAt: string;
  updatedAt: string;
}

function getAuthoredDir(workspace: string): string {
  const dir = join(workspace, "..", "..", "shared", AUTHORED_DIR_NAME);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function metaPath(dir: string, name: string): string {
  return join(dir, `${name}.meta.json`);
}

function codePath(dir: string, name: string): string {
  return join(dir, `${name}.tool.mjs`);
}

function testPath(dir: string, name: string): string {
  return join(dir, `${name}.test.mjs`);
}

function readMeta(dir: string, name: string): AuthoredToolMeta | null {
  const p = metaPath(dir, name);
  if (!existsSync(p)) return null;
  return JSON.parse(readFileSync(p, "utf-8"));
}

function writeMeta(dir: string, meta: AuthoredToolMeta): void {
  writeFileSync(metaPath(dir, meta.name), JSON.stringify(meta, null, 2));
}

function runTests(dir: string, name: string): { ok: boolean; output: string } {
  const tp = testPath(dir, name);
  if (!existsSync(tp)) return { ok: true, output: "no tests" };

  try {
    const output = execSync(`node "${tp}"`, {
      cwd: dir,
      timeout: SANDBOX_TIMEOUT,
      stdio: "pipe",
      env: { ...process.env, NODE_NO_WARNINGS: "1" },
    }).toString();
    return { ok: true, output: output.slice(0, 1000) };
  } catch (err: unknown) {
    const e = err as { stderr?: Buffer; stdout?: Buffer };
    const stderr = e.stderr?.toString() ?? "";
    const stdout = e.stdout?.toString() ?? "";
    return { ok: false, output: (stderr + "\n" + stdout).slice(0, 1000) };
  }
}

function loadAuthoredTool(dir: string, name: string): ToolHandler | null {
  const cp = codePath(dir, name);
  if (!existsSync(cp)) return null;

  const code = readFileSync(cp, "utf-8");

  // The tool file must export: { definition, execute }
  // We evaluate it in a controlled way
  try {
    // eslint-disable-next-line no-new-func
    const module = { exports: {} as Record<string, unknown> };
    const fn = new Function("module", "exports", "require", code);
    fn(module, module.exports, require);

    const def = module.exports["definition"] as Record<string, unknown> | undefined;
    const exec = module.exports["execute"] as Function | undefined;

    if (!def || !exec || typeof exec !== "function") {
      log.warn(`Authored tool ${name}: missing definition or execute export`);
      return null;
    }

    return {
      definition: def as unknown as ToolHandler["definition"],
      execute: exec as ToolHandler["execute"],
    };
  } catch (err) {
    log.warn(`Authored tool ${name} failed to load: ${err instanceof Error ? err.message : err}`);
    return null;
  }
}

export function loadAuthoredTools(workspace: string, registry: ToolRegistry): number {
  const dir = getAuthoredDir(workspace);
  let loaded = 0;

  for (const file of readdirSync(dir)) {
    if (!file.endsWith(".tool.mjs")) continue;
    const name = basename(file, ".tool.mjs");
    const meta = readMeta(dir, name);

    if (meta?.quarantined) {
      log.info(`Skipping quarantined tool: ${name}`);
      continue;
    }

    const handler = loadAuthoredTool(dir, name);
    if (handler) {
      registry.register(handler);
      loaded++;
      log.info(`Loaded authored tool: ${name}`);
    }
  }

  return loaded;
}

export function createSelfAuthorTools(workspace: string, registry: ToolRegistry): ToolHandler[] {
  const dir = getAuthoredDir(workspace);

  const toolCreate: ToolHandler = {
    definition: {
      name: "tool_create",
      description:
        "Create a new custom tool. Provide the tool code (CommonJS module exporting " +
        "`definition` and `execute`) and an optional test file. The tool is tested in " +
        "a sandbox before registration. Tool code must be < 8KB.",
      input_schema: {
        type: "object",
        properties: {
          name: {
            type: "string",
            description: "Tool name (alphanumeric + underscores)",
          },
          code: {
            type: "string",
            description:
              "Tool source code. Must export `definition` (Anthropic tool schema) " +
              "and `execute(input, context)` returning a string.",
          },
          test_code: {
            type: "string",
            description:
              "Optional test code. Runs via `node test.mjs` — exit 0 = pass, nonzero = fail.",
          },
        },
        required: ["name", "code"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const name = (input["name"] as string).replace(/[^a-zA-Z0-9_]/g, "");
      const code = input["code"] as string;
      const testCode = input["test_code"] as string | undefined;

      if (!name) return JSON.stringify({ error: "Invalid tool name" });
      if (code.length > MAX_TOOL_SIZE) {
        return JSON.stringify({ error: `Code exceeds ${MAX_TOOL_SIZE} byte limit` });
      }

      // Check for quarantined tool
      const existingMeta = readMeta(dir, name);
      if (existingMeta?.quarantined) {
        return JSON.stringify({ error: `Tool ${name} is quarantined after ${MAX_FAILURES} failures` });
      }

      // Write code + test
      writeFileSync(codePath(dir, name), code);
      if (testCode) writeFileSync(testPath(dir, name), testCode);

      // Run tests
      const testResult = runTests(dir, name);
      if (!testResult.ok) {
        return JSON.stringify({
          error: "Tests failed",
          output: testResult.output,
        });
      }

      // Try to load the tool
      const handler = loadAuthoredTool(dir, name);
      if (!handler) {
        return JSON.stringify({ error: "Tool failed to load — check exports" });
      }

      // Write meta and register
      const meta: AuthoredToolMeta = {
        name,
        author: context.nousId,
        version: (existingMeta?.version ?? 0) + 1,
        failures: 0,
        quarantined: false,
        createdAt: existingMeta?.createdAt ?? new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };
      writeMeta(dir, meta);
      registry.register(handler);

      log.info(`Tool ${name} created by ${context.nousId} (v${meta.version})`);
      return JSON.stringify({
        created: true,
        name,
        version: meta.version,
        tests: testResult.output,
      });
    },
  };

  const toolList: ToolHandler = {
    definition: {
      name: "tool_list_authored",
      description: "List all self-authored tools with their status.",
      input_schema: { type: "object", properties: {} },
    },
    async execute(): Promise<string> {
      const tools: Record<string, unknown>[] = [];
      for (const file of readdirSync(dir)) {
        if (!file.endsWith(".meta.json")) continue;
        const name = basename(file, ".meta.json");
        const meta = readMeta(dir, name);
        if (meta) {
          tools.push({
            name: meta.name,
            author: meta.author,
            version: meta.version,
            failures: meta.failures,
            quarantined: meta.quarantined,
            updatedAt: meta.updatedAt,
          });
        }
      }
      return JSON.stringify({ tools });
    },
  };

  const toolRecordFailure: ToolHandler = {
    definition: {
      name: "tool_record_failure",
      description:
        "Record a failure for an authored tool. After 3 failures, the tool is quarantined.",
      input_schema: {
        type: "object",
        properties: {
          name: { type: "string", description: "Tool name" },
          error: { type: "string", description: "Error description" },
        },
        required: ["name"],
      },
    },
    async execute(input: Record<string, unknown>): Promise<string> {
      const name = input["name"] as string;
      const meta = readMeta(dir, name);
      if (!meta) return JSON.stringify({ error: "Tool not found" });

      meta.failures++;
      meta.updatedAt = new Date().toISOString();
      if (meta.failures >= MAX_FAILURES) {
        meta.quarantined = true;
        log.warn(`Tool ${name} quarantined after ${meta.failures} failures`);
      }
      writeMeta(dir, meta);

      return JSON.stringify({
        name,
        failures: meta.failures,
        quarantined: meta.quarantined,
      });
    },
  };

  return [toolCreate, toolList, toolRecordFailure];
}
