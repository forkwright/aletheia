// Nous workspace scaffolding utilities
import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { readText, writeText } from "../koina/fs.js";
import { createLogger } from "../koina/logger.js";

const log = createLogger("taxis:nous-scaffold");

const SHARED_WORKSPACE_DIRS = [
  ["workspace", "plans"],
  ["workspace", "specs"],
  ["workspace", "standards"],
  ["workspace", "references"],
] as const;

export function scaffoldNousShared(sharedDir: string): string[] {
  const created: string[] = [];
  for (const segments of SHARED_WORKSPACE_DIRS) {
    const dir = join(sharedDir, ...segments);
    if (!existsSync(dir)) {
      mkdirSync(dir, { recursive: true });
      created.push(segments.join("/"));
    }
  }
  log.debug("scaffoldNousShared complete", { sharedDir, created });
  return created;
}

const MANAGED_BEGIN = "# BEGIN aletheia-managed";
const MANAGED_END = "# END aletheia-managed";
const MANAGED_ENTRIES = [
  "# Workspace ephemeral content",
  "*/workspace/plans/",
  "*/workspace/data/",
  "",
  "# Memory and index",
  "memory/",
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

const AGENT_WORKSPACE_SUBDIRS = ["scripts", "drafts", "data"] as const;

export function scaffoldAgentWorkspaceDirs(agentWorkspace: string): void {
  for (const subdir of AGENT_WORKSPACE_SUBDIRS) {
    mkdirSync(join(agentWorkspace, "workspace", subdir), { recursive: true });
  }
  log.debug("scaffoldAgentWorkspaceDirs complete", { agentWorkspace });
}
