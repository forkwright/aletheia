// Declarative hook system — YAML-defined shell hooks wired to the event bus
//
// Hook definitions live in shared/hooks/*.yaml. Each hook specifies:
//   - event: which EventName to listen on
//   - handler.command: shell command to execute
//   - handler.args: template variables substituted from event payload
//   - handler.timeout: max execution time (default 30s)
//   - handler.failAction: warn | block | silent (default: warn)
//   - nousFilter: optional list of nous IDs to restrict to
//
// Shell handlers receive the full event payload as JSON on stdin.
// Exit codes: 0 = success, 1 = warning, 2+ = error.
// Same protocol as Claude Code's hook system for ecosystem compatibility.

import { execFile } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { extname, join } from "node:path";
import { z } from "zod";
import { createLogger } from "./logger.js";
import { eventBus, type EventName, type EventPayload } from "./event-bus.js";

const log = createLogger("hooks");

// --- Schema ---

const ALLOWED_EXTENSIONS = new Set([".sh", ".py", ".js", ".ts", ".rb", ".pl"]);

const HandlerSchema = z.object({
  type: z.literal("shell").default("shell"),
  command: z.string().min(1),
  args: z.array(z.string()).default([]),
  timeout: z.string().default("30s"),
  failAction: z.enum(["warn", "block", "silent"]).default("warn"),
  env: z.record(z.string(), z.string()).default({}),
  cwd: z.string().optional(),
});

const HookDefinitionSchema = z.object({
  name: z.string().min(1),
  event: z.string().min(1),
  handler: HandlerSchema,
  nousFilter: z.array(z.string()).optional(),
  enabled: z.boolean().default(true),
  description: z.string().optional(),
});

export type HookDefinition = z.infer<typeof HookDefinitionSchema>;

// --- Template substitution ---

/**
 * Replace {{variable}} placeholders with values from the event payload.
 * Nested access via dot notation: {{session.id}} → payload.session.id
 * Missing variables resolve to empty string.
 */
export function substituteTemplateVars(
  template: string,
  payload: EventPayload,
): string {
  return template.replace(/\{\{(\w+(?:\.\w+)*)\}\}/g, (_match, path: string) => {
    const parts = path.split(".");
    let value: unknown = payload;
    for (const part of parts) {
      if (value === null || value === undefined || typeof value !== "object") return "";
      value = (value as Record<string, unknown>)[part];
    }
    if (value === null || value === undefined) return "";
    if (typeof value === "object") return JSON.stringify(value);
    return String(value);
  });
}

// --- Timeout parsing ---

function parseTimeout(timeout: string): number {
  const match = timeout.match(/^(\d+)(ms|s|m)?$/);
  if (!match) return 30_000;
  const value = parseInt(match[1]!, 10);
  const unit = match[2] ?? "s";
  switch (unit) {
    case "ms": return value;
    case "s":  return value * 1000;
    case "m":  return value * 60_000;
    default:   return value * 1000;
  }
}

// --- Shell execution ---

export interface HookResult {
  hookName: string;
  exitCode: number | null;
  stdout: string;
  stderr: string;
  timedOut: boolean;
  durationMs: number;
}

/**
 * Execute a shell hook. The command receives event data as JSON on stdin.
 * Returns a structured result regardless of success/failure.
 */
export async function executeShellHook(
  hook: HookDefinition,
  payload: EventPayload,
): Promise<HookResult> {
  const timeoutMs = parseTimeout(hook.handler.timeout);
  const args = hook.handler.args.map((arg) =>
    substituteTemplateVars(arg, payload),
  );

  const command = substituteTemplateVars(hook.handler.command, payload);
  const stdinData = JSON.stringify(payload);
  const start = Date.now();

  return new Promise<HookResult>((resolveResult) => {
    const child = execFile(
      command,
      args,
      {
        timeout: timeoutMs,
        maxBuffer: 1024 * 1024, // 1MB
        env: {
          ...process.env,
          ...hook.handler.env,
          ALETHEIA_HOOK_NAME: hook.name,
          ALETHEIA_HOOK_EVENT: hook.event,
        },
        cwd: hook.handler.cwd,
      },
      (error, stdout, stderr) => {
        const durationMs = Date.now() - start;

        // Node's execFile sets `killed: true` and `signal: 'SIGTERM'` on timeout
        const errAny = error as NodeJS.ErrnoException & { killed?: boolean; signal?: string } | null;
        const timedOut = errAny?.killed === true || errAny?.signal === "SIGTERM";

        // exitCode: null if killed/timed out, otherwise from error or 0
        let exitCode: number | null = null;
        if (timedOut) {
          exitCode = null;
        } else if (error && "code" in error && typeof error.code === "number") {
          exitCode = error.code;
        } else if (!error) {
          exitCode = 0;
        }

        resolveResult({
          hookName: hook.name,
          exitCode,
          stdout: stdout.trim(),
          stderr: stderr.trim(),
          timedOut,
          durationMs,
        });
      },
    );

    // Pipe event payload as JSON to stdin — ignore EPIPE if command doesn't read stdin
    if (child.stdin) {
      child.stdin.on("error", () => { /* EPIPE is expected for commands that don't read stdin */ });
      child.stdin.write(stdinData);
      child.stdin.end();
    }
  });
}

// --- YAML loading ---

/**
 * Load hook definitions from a directory of YAML files.
 * Invalid files are logged and skipped, never crash boot.
 */
export function loadHookDefinitions(hooksDir: string): HookDefinition[] {
  if (!existsSync(hooksDir)) {
    log.debug(`Hooks directory not found: ${hooksDir}`);
    return [];
  }

  const hooks: HookDefinition[] = [];
  let files: string[];

  try {
    files = readdirSync(hooksDir).filter(
      (f) => f.endsWith(".yaml") || f.endsWith(".yml"),
    );
  } catch (err) {
    log.warn(`Cannot read hooks directory ${hooksDir}: ${err instanceof Error ? err.message : err}`);
    return [];
  }

  for (const file of files) {
    const filePath = join(hooksDir, file);
    try {
      const content = readFileSync(filePath, "utf-8");
      // Simple YAML parsing — we use a minimal approach to avoid adding js-yaml dependency.
      // Hook YAML is structured enough for JSON-compatible parsing after YAML→JSON conversion.
      const parsed = parseSimpleYaml(content);
      if (!parsed) {
        log.warn(`Empty or unparseable hook file: ${file}`);
        continue;
      }

      const result = HookDefinitionSchema.safeParse(parsed);
      if (!result.success) {
        log.warn(`Invalid hook definition in ${file}: ${result.error.message}`);
        continue;
      }

      const hook = result.data;

      // Validate command extension
      const cmdExt = extname(hook.handler.command);
      if (cmdExt && !ALLOWED_EXTENSIONS.has(cmdExt)) {
        log.warn(`Hook ${hook.name}: command extension ${cmdExt} not in allowlist, skipping`);
        continue;
      }

      // Validate event name is a known event
      if (!isValidEventName(hook.event)) {
        log.warn(`Hook ${hook.name}: unknown event '${hook.event}', skipping`);
        continue;
      }

      if (!hook.enabled) {
        log.info(`Hook ${hook.name}: disabled, skipping`);
        continue;
      }

      hooks.push(hook);
      log.info(`Loaded hook: ${hook.name} → ${hook.event}`);
    } catch (err) {
      log.warn(`Error loading hook file ${file}: ${err instanceof Error ? err.message : err}`);
    }
  }

  return hooks;
}

// Known event names from the event bus
const VALID_EVENTS: Set<string> = new Set([
  "turn:before", "turn:after",
  "tool:called", "tool:failed",
  "distill:before", "distill:stage", "distill:after",
  "session:created", "session:archived",
  "memory:added", "memory:retracted",
  "signal:received",
  "boot:start", "boot:ready",
  "config:reloaded",
  "exec:denied",
  "pipeline:error",
  "history:orphan_repair",
]);

function isValidEventName(event: string): event is EventName {
  return VALID_EVENTS.has(event);
}

// --- Minimal YAML parser ---
// Handles the flat structure of hook definitions without requiring js-yaml.
// Supports: scalars, arrays (inline and block), nested objects (one level),
// and comments. NOT a full YAML parser — just enough for hook definitions.

export function parseSimpleYaml(content: string): Record<string, unknown> | null {
  const lines = content.split("\n");
  const result: Record<string, unknown> = {};
  let currentIndent = 0;
  let nestedObj: Record<string, unknown> | null = null;
  let nestedKey: string | null = null;
  let arrayKey: string | null = null;
  let arrayItems: Array<string | number | boolean> | null = null;

  for (const rawLine of lines) {
    const line = rawLine.replace(/#.*$/, ""); // strip comments
    if (line.trim() === "") continue;

    const indent = line.length - line.trimStart().length;
    const trimmed = line.trim();

    // Flush pending array if indent returns to base level
    if (arrayKey && indent <= currentIndent && !trimmed.startsWith("-")) {
      if (nestedObj && nestedKey) {
        nestedObj[arrayKey] = arrayItems;
      } else {
        result[arrayKey] = arrayItems;
      }
      arrayKey = null;
      arrayItems = null;
    }

    // Array item (block style)
    if (trimmed.startsWith("- ") && arrayKey) {
      arrayItems!.push(parseYamlValue(trimmed.slice(2).trim()));
      continue;
    }

    // key: value pair
    const kvMatch = trimmed.match(/^(\w+)\s*:\s*(.*)$/);
    if (!kvMatch) continue;

    const key = kvMatch[1]!;
    const rawValue = kvMatch[2]!.trim();

    // Nested object detection: key with no value, next lines indented
    if (rawValue === "" || rawValue === "|" || rawValue === ">") {
      // Close any pending nested object at same or lower indent
      if (nestedObj && nestedKey && indent <= currentIndent) {
        result[nestedKey] = nestedObj;
        nestedObj = null;
        nestedKey = null;
      }

      if (indent === 0) {
        // Top-level nested object
        nestedKey = key;
        nestedObj = {};
        currentIndent = indent;
      }
      continue;
    }

    // Inline array: [a, b, c]
    if (rawValue.startsWith("[") && rawValue.endsWith("]")) {
      const items = rawValue
        .slice(1, -1)
        .split(",")
        .map((s) => parseYamlValue(s.trim()))
        .filter((s) => s !== "");

      if (nestedObj && indent > 0) {
        nestedObj[key] = items;
      } else {
        if (nestedObj && nestedKey) {
          result[nestedKey] = nestedObj;
          nestedObj = null;
          nestedKey = null;
        }
        result[key] = items;
      }
      continue;
    }

    // Block array start: key followed by - items on next lines
    // We'll detect this when we see a "- " on the next iteration

    // Regular value
    const parsedValue = parseYamlValue(rawValue);

    if (nestedObj && indent > 0) {
      nestedObj[key] = parsedValue;
    } else {
      // Close nested object if returning to top level
      if (nestedObj && nestedKey) {
        result[nestedKey] = nestedObj;
        nestedObj = null;
        nestedKey = null;
      }
      result[key] = parsedValue;
      currentIndent = indent;
    }
  }

  // Flush any pending nested object or array
  if (arrayKey) {
    if (nestedObj && nestedKey) {
      nestedObj[arrayKey] = arrayItems;
    } else {
      result[arrayKey] = arrayItems;
    }
  }
  if (nestedObj && nestedKey) {
    result[nestedKey] = nestedObj;
  }

  return Object.keys(result).length > 0 ? result : null;
}

function parseYamlValue(raw: string): string | number | boolean {
  // Remove surrounding quotes
  if ((raw.startsWith('"') && raw.endsWith('"')) || (raw.startsWith("'") && raw.endsWith("'"))) {
    return raw.slice(1, -1);
  }
  if (raw === "true") return true;
  if (raw === "false") return false;
  if (/^-?\d+$/.test(raw)) return parseInt(raw, 10);
  if (/^-?\d+\.\d+$/.test(raw)) return parseFloat(raw);
  return raw;
}

// --- Registry ---

export interface HookRegistry {
  hooks: HookDefinition[];
  teardown: () => void;
}

/**
 * Register all hooks from a directory onto the event bus.
 * Returns a teardown function to remove all listeners.
 */
export function registerHooks(hooksDir: string): HookRegistry {
  const hooks = loadHookDefinitions(hooksDir);
  const teardowns: Array<() => void> = [];

  for (const hook of hooks) {
    const handler = createHookHandler(hook);
    const eventName = hook.event as EventName;
    eventBus.on(eventName, handler);
    teardowns.push(() => eventBus.off(eventName, handler));
  }

  if (hooks.length > 0) {
    log.info(`Registered ${hooks.length} hook(s) from ${hooksDir}`);
  }

  return {
    hooks,
    teardown: () => {
      for (const fn of teardowns) fn();
      log.info("All hooks unregistered");
    },
  };
}

/**
 * Create an event handler function for a hook definition.
 * Handles nous filtering, execution, and error handling per failAction.
 */
function createHookHandler(hook: HookDefinition): (payload: EventPayload) => void {
  return (payload: EventPayload) => {
    // Nous filter — skip if payload.nousId doesn't match
    if (hook.nousFilter && hook.nousFilter.length > 0) {
      const nousId = payload["nousId"] as string | undefined;
      if (nousId && !hook.nousFilter.includes(nousId)) {
        return;
      }
    }

    // Fire-and-forget — don't block the event bus
    executeShellHook(hook, payload)
      .then((result) => {
        if (result.timedOut) {
          if (hook.handler.failAction !== "silent") {
            log.warn(`Hook ${hook.name} timed out after ${result.durationMs}ms`);
          }
          return;
        }

        if (result.exitCode === 0) {
          log.debug(`Hook ${hook.name} completed in ${result.durationMs}ms`);
          if (result.stdout) {
            log.debug(`Hook ${hook.name} stdout: ${result.stdout.slice(0, 500)}`);
          }
          return;
        }

        // Non-zero exit
        switch (hook.handler.failAction) {
          case "silent":
            break;
          case "warn":
            log.warn(
              `Hook ${hook.name} exited ${result.exitCode}: ${result.stderr || result.stdout}`.slice(0, 500),
            );
            break;
          case "block":
            // In fire-and-forget mode, block can only log — blocking requires sync execution
            // which the spec notes as a future enhancement. For now, treat as warn + error level.
            log.error(
              `Hook ${hook.name} BLOCKED (exit ${result.exitCode}): ${result.stderr || result.stdout}`.slice(0, 500),
            );
            break;
        }
      })
      .catch((err) => {
        if (hook.handler.failAction !== "silent") {
          log.warn(`Hook ${hook.name} execution error: ${err instanceof Error ? err.message : err}`);
        }
      });
  };
}
