// Nous workspace scaffolding utilities
import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { readText, writeText } from "../koina/fs.js";
import { createLogger } from "../koina/logger.js";

const log = createLogger("taxis:nous-scaffold");

/**
 * Theke directories — the single shared working tree for all agents and the
 * operator.  Organized by subject, not by agent.
 *
 * See docs/WORKSPACE_FILES.md and instance.example/README.md for the full
 * rationale.  The rule: nous/{id}/ holds identity + session memory ONLY.
 * All research, plans, drafts, specs, and work products live in theke/.
 */
const THEKE_DIRS = [
  ["projects"],
  ["research"],
  ["reference"],
  ["nous"],        // per-agent scratch within theke (theke/nous/{id}/)
  ["archive"],
] as const;

/**
 * Scaffold the theke/ working tree inside the instance directory.
 * Called during `aletheia init` and at runtime startup.
 *
 * @returns list of directories that were newly created (empty on repeat calls)
 */
export function scaffoldTheke(thekeDir: string): string[] {
  const created: string[] = [];
  for (const segments of THEKE_DIRS) {
    const dir = join(thekeDir, ...segments);
    if (!existsSync(dir)) {
      mkdirSync(dir, { recursive: true });
      created.push(segments.join("/"));
    }
  }
  log.debug("scaffoldTheke complete", { thekeDir, created });
  return created;
}

const MANAGED_BEGIN = "# BEGIN aletheia-managed";
const MANAGED_END = "# END aletheia-managed";
const MANAGED_ENTRIES = [
  "# Agent session memory",
  "memory/",
  "",
  "# Runtime index",
  ".aletheia-index/",
  "",
  "# Secrets and credentials",
  ".env",
  "*.log",
  "*.key",
  "*.pem",
  "*.secret",
  "secrets/",
  "credentials/",
].join("\n");

export function mergeGitignore(nousDir: string): void {
  const gitignorePath = join(nousDir, ".gitignore");
  const existing = readText(gitignorePath) ?? "";
  const lines = existing.split("\n");

  const beginIdx = lines.findIndex((l) => l.trim() === MANAGED_BEGIN);
  const endIdx = lines.findIndex((l) => l.trim() === MANAGED_END);

  const block = [MANAGED_BEGIN, MANAGED_ENTRIES, MANAGED_END].join("\n");

  let updated: string;
  if (beginIdx !== -1 && endIdx !== -1 && endIdx > beginIdx) {
    const before = lines.slice(0, beginIdx).join("\n").trimEnd();
    const after = lines.slice(endIdx + 1).join("\n").trimStart();
    updated = [before, block, after].filter(Boolean).join("\n\n") + "\n";
  } else {
    const base = existing.trimEnd();
    updated = (base ? base + "\n\n" : "") + block + "\n";
  }

  writeText(gitignorePath, updated);
  log.debug("mergeGitignore complete", { gitignorePath });
}

/**
 * Scaffold the minimal agent workspace: just a memory/ directory for session
 * logs.  All other working files belong in theke/.
 */
export function scaffoldAgentWorkspace(agentWorkspace: string): void {
  const memDir = join(agentWorkspace, "memory");
  if (!existsSync(memDir)) {
    mkdirSync(memDir, { recursive: true });
  }
  log.debug("scaffoldAgentWorkspace complete", { agentWorkspace });
}

// ── Deprecated aliases ──────────────────────────────────────────────────────

/** @deprecated Use scaffoldTheke instead. Will be removed in a future version. */
export const scaffoldNousShared = scaffoldTheke;

/** @deprecated Use scaffoldAgentWorkspace instead. Will be removed in a future version. */
export function scaffoldAgentWorkspaceDirs(agentWorkspace: string): void {
  log.warn("scaffoldAgentWorkspaceDirs is deprecated — use scaffoldAgentWorkspace");
  scaffoldAgentWorkspace(agentWorkspace);
}
