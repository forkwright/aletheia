// Token-aware context assembly with cache boundary optimization
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { createLogger } from "../koina/logger.js";
import { estimateTokens } from "../hermeneus/token-counter.js";

const log = createLogger("nous.bootstrap");

interface BootstrapFile {
  name: string;
  path: string;
  priority: number;
  cacheGroup: "static" | "semi-static" | "dynamic";
  content?: string;
  tokens?: number;
  hash?: string;
}

// Cache groups for Anthropic prefix caching:
// Group 1 (static): rarely changes — cached, breakpoint after group
// Group 2 (semi-static): changes occasionally — cached, breakpoint after group
// Group 3 (dynamic): changes every turn — never cached
const WORKSPACE_FILES: Omit<BootstrapFile, "path">[] = [
  { name: "SOUL.md", priority: 1, cacheGroup: "static" },
  { name: "USER.md", priority: 2, cacheGroup: "static" },
  { name: "AGENTS.md", priority: 3, cacheGroup: "static" },
  { name: "IDENTITY.md", priority: 4, cacheGroup: "static" },
  { name: "GOALS.md", priority: 4.5, cacheGroup: "semi-static" },
  { name: "TOOLS.md", priority: 5, cacheGroup: "semi-static" },
  { name: "MEMORY.md", priority: 6, cacheGroup: "semi-static" },
  { name: "STRATEGY.md", priority: 6.3, cacheGroup: "semi-static" },
  { name: "EVAL_FEEDBACK.md", priority: 6.5, cacheGroup: "semi-static" },
  { name: "PROSOCHE.md", priority: 7, cacheGroup: "dynamic" },
  { name: "CONTEXT.md", priority: 8, cacheGroup: "dynamic" },
];

type SystemBlock = { type: "text"; text: string; cache_control?: { type: "ephemeral" } };

// Degradation guidance per service — injected when services are down
const DEGRADATION_GUIDANCE: Record<string, string> = {
  "mem0-sidecar": "Long-term memory unavailable. Rely on workspace files and session history only. Do not attempt memory searches.",
  "neo4j": "Graph-based memory retrieval unavailable. Vector search may still work via Mem0.",
  "qdrant": "All vector memory retrieval unavailable. Rely on workspace files only.",
  "signal-cli": "Signal messaging unavailable. You can process but not respond.",
};

export interface BootstrapResult {
  staticBlocks: SystemBlock[];
  dynamicBlocks: SystemBlock[];
  totalTokens: number;
  fileCount: number;
  contentHash: string;
  fileHashes: Record<string, string>;
  droppedFiles: string[];
}

export function assembleBootstrap(
  workspace: string,
  opts?: {
    maxTokens?: number;
    extraFiles?: string[];
    skillsSection?: string;
    degradedServices?: string[];
  },
): BootstrapResult {
  const maxTokens = opts?.maxTokens ?? 40000;

  const files: BootstrapFile[] = WORKSPACE_FILES.map((f) => ({
    ...f,
    path: join(workspace, f.name),
  }));

  if (opts?.extraFiles) {
    for (const extra of opts.extraFiles) {
      files.push({
        name: extra,
        path: join(workspace, extra),
        priority: 10,
        cacheGroup: "dynamic",
      });
    }
  }

  const loaded: BootstrapFile[] = [];
  for (const file of files) {
    if (!existsSync(file.path)) continue;
    try {
      file.content = readFileSync(file.path, "utf-8").trim();
      if (!file.content) continue;
      file.tokens = estimateTokens(file.content);
      file.hash = createHash("sha256").update(file.content).digest("hex").slice(0, 16);
      loaded.push(file);
    } catch {
      log.warn(`Failed to read ${file.path}`);
    }
  }

  loaded.sort((a, b) => a.priority - b.priority);

  // Assemble with token budgeting, dropping lowest-priority files first
  const included: BootstrapFile[] = [];
  const droppedFiles: string[] = [];
  let totalTokens = 0;

  for (const file of loaded) {
    if (!file.content || !file.tokens) continue;

    if (totalTokens + file.tokens > maxTokens) {
      const remaining = maxTokens - totalTokens;
      if (remaining > 500) {
        const truncated = truncateSectionAware(file.content, remaining);
        const truncTokens = estimateTokens(truncated);
        included.push({
          ...file,
          content: truncated,
          tokens: truncTokens,
          hash: createHash("sha256").update(truncated).digest("hex").slice(0, 16),
        });
        totalTokens += truncTokens;
        log.warn(`Truncated ${file.name} (${file.tokens} → ${truncTokens} tokens)`);
      } else {
        log.warn(`Dropped ${file.name} (${file.tokens} tokens) — budget exhausted`);
        droppedFiles.push(file.name);
      }
      // Log remaining dropped files
      const idx = loaded.indexOf(file);
      for (let i = idx + 1; i < loaded.length; i++) {
        log.warn(`Dropped ${loaded[i]!.name} (${loaded[i]!.tokens} tokens) — budget exhausted`);
        droppedFiles.push(loaded[i]!.name);
      }
      break;
    }

    included.push(file);
    totalTokens += file.tokens;
  }

  // Build per-file hash map and content hash
  const fileHashes: Record<string, string> = {};
  const hashParts: string[] = [];
  for (const f of included) {
    if (f.hash) {
      fileHashes[f.name] = f.hash;
      hashParts.push(`${f.name}:${f.hash}`);
    }
  }
  const contentHash = createHash("sha256").update(hashParts.join("|")).digest("hex").slice(0, 32);

  // Build system blocks with exactly 2 cache breakpoints:
  // Breakpoint 1: after static group (SOUL, AGENTS, IDENTITY)
  // Breakpoint 2: after semi-static group (TOOLS, MEMORY)
  // Dynamic group (PROSOCHE, CONTEXT): no caching
  const staticBlocks: SystemBlock[] = [];
  const dynamicBlocks: SystemBlock[] = [];

  const staticFiles = included.filter((f) => f.cacheGroup === "static");
  const semiStaticFiles = included.filter((f) => f.cacheGroup === "semi-static");
  const dynamicFiles = included.filter((f) => f.cacheGroup === "dynamic");

  // Static group — combine into one block with cache breakpoint
  if (staticFiles.length > 0) {
    const text = staticFiles
      .map((f) => `## ${f.name}\n\n${f.content}`)
      .join("\n\n---\n\n");
    staticBlocks.push({
      type: "text",
      text,
      cache_control: { type: "ephemeral" },
    });
  }

  // Semi-static group — combine into one block with cache breakpoint
  if (semiStaticFiles.length > 0 || opts?.skillsSection) {
    const parts = semiStaticFiles.map((f) => `## ${f.name}\n\n${f.content}`);
    if (opts?.skillsSection) {
      parts.push(opts.skillsSection);
    }
    const text = parts.join("\n\n---\n\n");
    staticBlocks.push({
      type: "text",
      text,
      cache_control: { type: "ephemeral" },
    });
  }

  // Degraded-mode injection — prepend service status to dynamic blocks
  if (opts?.degradedServices && opts.degradedServices.length > 0) {
    const notices = opts.degradedServices.map((svc) => {
      const guidance = DEGRADATION_GUIDANCE[svc] ?? `${svc} is unavailable.`;
      return `- ${svc}: ${guidance}`;
    });
    dynamicBlocks.push({
      type: "text",
      text: `## Infrastructure Status\n\n**The following services are currently DOWN:**\n${notices.join("\n")}\n\nAdjust your behavior accordingly. Do not attempt to use tools that depend on unavailable services.`,
    });
  }

  // Dynamic group — individual blocks, no caching
  for (const file of dynamicFiles) {
    dynamicBlocks.push({
      type: "text",
      text: `## ${file.name}\n\n${file.content}`,
    });
  }

  if (droppedFiles.length > 0) {
    log.warn(`Bootstrap dropped ${droppedFiles.length} files: ${droppedFiles.join(", ")}`);
  }

  log.info(
    `Bootstrap: ${included.length} files, ${totalTokens} tokens ` +
    `(${staticBlocks.length} cached blocks, ${dynamicBlocks.length} dynamic), ` +
    `hash=${contentHash.slice(0, 8)}`,
  );

  return {
    staticBlocks,
    dynamicBlocks,
    totalTokens,
    fileCount: included.length,
    contentHash,
    fileHashes,
    droppedFiles,
  };
}

function truncateSectionAware(content: string, maxTokens: number): string {
  // Try to preserve complete markdown sections by splitting on ## headers
  const sections = content.split(/(?=^## )/m);
  const result: string[] = [];
  let tokens = 0;

  for (const section of sections) {
    const sectionTokens = estimateTokens(section);
    if (tokens + sectionTokens > maxTokens) {
      // If this is the first section and it doesn't fit, fall back to line-by-line
      if (result.length === 0) {
        return truncateByLines(section, maxTokens);
      }
      result.push("\n... [truncated for token budget] ...");
      break;
    }
    result.push(section);
    tokens += sectionTokens;
  }

  return result.join("");
}

function truncateByLines(content: string, maxTokens: number): string {
  const lines = content.split("\n");
  const result: string[] = [];
  let tokens = 0;

  for (const line of lines) {
    const lineTokens = estimateTokens(line);
    if (tokens + lineTokens > maxTokens) {
      result.push("\n... [truncated for token budget] ...");
      break;
    }
    result.push(line);
    tokens += lineTokens;
  }

  return result.join("\n");
}
