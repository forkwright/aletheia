// Structured logging with request tracing via AsyncLocalStorage
import { Logger } from "tslog";
import { AsyncLocalStorage } from "node:async_hooks";
import { appendFileSync } from "node:fs";

// --- Turn context (propagated via AsyncLocalStorage) ---

export interface TurnContext {
  turnId: string;
  nousId?: string;
  sessionId?: string;
  sessionKey?: string;
  channel?: string;
  sender?: string;
  [key: string]: unknown;
}

const turnStore = new AsyncLocalStorage<TurnContext>();
let turnCounter = 0;

/** Generate a short, monotonically increasing turn ID */
function generateTurnId(): string {
  const ts = Date.now().toString(36);
  const seq = (++turnCounter).toString(36).padStart(3, "0");
  return `t_${ts}_${seq}`;
}

/** Run a function with a new turn context. All log calls inside inherit the context. */
export function withTurn<T>(ctx: Partial<TurnContext>, fn: () => T): T {
  const turnId = ctx.turnId ?? generateTurnId();
  return turnStore.run({ ...ctx, turnId }, fn);
}

/** Run an async function with a new turn context. */
export async function withTurnAsync<T>(ctx: Partial<TurnContext>, fn: () => Promise<T>): Promise<T> {
  const turnId = ctx.turnId ?? generateTurnId();
  return turnStore.run({ ...ctx, turnId }, fn);
}

/** Get current turn context (or undefined if outside a turn) */
export function getTurnContext(): TurnContext | undefined {
  return turnStore.getStore();
}

/** Update current turn context in-place */
export function updateTurnContext(updates: Partial<TurnContext>): void {
  const ctx = turnStore.getStore();
  if (ctx) Object.assign(ctx, updates);
}

// --- Log level configuration ---

type LogLevel = "silly" | "trace" | "debug" | "info" | "warn" | "error" | "fatal";

const LEVEL_MAP: Record<LogLevel, number> = {
  silly: 0,
  trace: 1,
  debug: 2,
  info: 3,
  warn: 4,
  error: 5,
  fatal: 6,
};

/** Parse ALETHEIA_LOG_MODULES for per-module overrides.
 *  Format: "semeion:debug,nous:trace,hermeneus:info"
 */
function parseModuleLevels(): Map<string, number> {
  const env = process.env["ALETHEIA_LOG_MODULES"] ?? "";
  const map = new Map<string, number>();
  if (!env) return map;
  for (const pair of env.split(",")) {
    const [mod, level] = pair.trim().split(":") as [string, LogLevel | undefined];
    if (mod && level && LEVEL_MAP[level] !== undefined) {
      map.set(mod, LEVEL_MAP[level]);
    }
  }
  return map;
}

const globalLevel = parseGlobalLevel();
const moduleLevels = parseModuleLevels();

function parseGlobalLevel(): number {
  const env = (process.env["ALETHEIA_LOG_LEVEL"] ?? "info").toLowerCase() as LogLevel;
  return LEVEL_MAP[env] ?? 3;
}

// --- Root logger ---

export const log = new Logger({
  name: "aletheia",
  prettyLogTemplate:
    "{{dateIsoStr}} {{logLevelName}} [{{name}}] ",
  prettyErrorTemplate:
    "{{dateIsoStr}} {{logLevelName}} [{{name}}] {{errorName}} {{errorMessage}}\n{{errorStack}}",
  type: "pretty",
  minLevel: globalLevel,
});

// Attach JSON transport for structured output to file/pipe
const jsonLogPath = process.env["ALETHEIA_LOG_JSON"];
if (jsonLogPath) {
  log.attachTransport((logObj) => {
    const ctx = turnStore.getStore();
    const entry: Record<string, unknown> = {
      ts: new Date().toISOString(),
      level: logObj["_meta"]?.["logLevelName"],
      module: logObj["_meta"]?.["name"],
      ...(ctx?.turnId ? { turnId: ctx.turnId } : {}),
      ...(ctx?.nousId ? { nousId: ctx.nousId } : {}),
      ...(ctx?.sessionKey ? { sessionKey: ctx.sessionKey } : {}),
      ...(ctx?.channel ? { channel: ctx.channel } : {}),
    };
    // Extract message from the numbered keys (tslog stores args as 0, 1, 2...)
    const parts: unknown[] = [];
    for (let i = 0; i < 10; i++) {
      if (logObj[String(i)] !== undefined) parts.push(logObj[String(i)]);
      else break;
    }
    if (parts.length === 1 && typeof parts[0] === "string") {
      entry["msg"] = parts[0];
    } else if (parts.length > 0) {
      entry["msg"] = parts[0];
      if (parts.length > 1) entry["data"] = parts.slice(1);
    }
    try {
      appendFileSync(jsonLogPath, JSON.stringify(entry) + "\n");
    } catch { /* log write failed — cannot recurse */
      // Don't recurse on log failure
    }
  });
}

// --- Scoped logger factory ---

export function createLogger(name: string): Logger<unknown> {
  // Check for module-specific log level
  const moduleLevel = findModuleLevel(name);
  return log.getSubLogger({
    name,
    ...(moduleLevel !== undefined ? { minLevel: moduleLevel } : {}),
  });
}

function findModuleLevel(name: string): number | undefined {
  // Exact match first
  if (moduleLevels.has(name)) return moduleLevels.get(name);
  // Prefix match: "semeion" matches "semeion:listen", "semeion:daemon", etc.
  for (const [mod, level] of moduleLevels) {
    if (name.startsWith(mod + ":") || name.startsWith(mod + ".")) return level;
    if (name.includes(":" + mod) || name === mod) return level;
  }
  return undefined;
}

/**
 * Truncate long strings in a value before logging — prevents PII-heavy
 * payloads (message content, tool results) from appearing in log output.
 * Primitive types are returned as-is. Objects are shallow-cloned with
 * string fields longer than `maxLen` truncated to `[…N chars]`.
 */
export function sanitizeForLog(value: unknown, maxLen = 200): unknown {
  if (typeof value === "string") {
    return value.length > maxLen ? `${value.slice(0, maxLen)}…[${value.length} chars]` : value;
  }
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      out[k] = typeof v === "string" && v.length > maxLen
        ? `${v.slice(0, maxLen)}…[${v.length} chars]`
        : v;
    }
    return out;
  }
  return value;
}
