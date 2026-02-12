// Token-aware context assembly with cache boundary optimization
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
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
  { name: "TOOLS.md", priority: 5, cacheGroup: "semi-static" },
  { name: "MEMORY.md", priority: 6, cacheGroup: "semi-static" },
  { name: "PROSOCHE.md", priority: 7, cacheGroup: "dynamic" },
  { name: "CONTEXT.md", priority: 8, cacheGroup: "dynamic" },
];

type SystemBlock = { type: "text"; text: string; cache_control?: { type: "ephemeral" } };

export interface BootstrapResult {
  staticBlocks: SystemBlock[];
  dynamicBlocks: SystemBlock[];
  totalTokens: number;
  fileCount: number;
}

export function assembleBootstrap(
  workspace: string,
  opts?: {
    maxTokens?: number;
    extraFiles?: string[];
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
      loaded.push(file);
    } catch {
      log.warn(`Failed to read ${file.path}`);
    }
  }

  loaded.sort((a, b) => a.priority - b.priority);

  // Assemble with token budgeting, dropping lowest-priority files first
  const included: BootstrapFile[] = [];
  let totalTokens = 0;

  for (const file of loaded) {
    if (!file.content || !file.tokens) continue;

    if (totalTokens + file.tokens > maxTokens) {
      const remaining = maxTokens - totalTokens;
      if (remaining > 500) {
        const truncated = truncateSectionAware(file.content, remaining);
        included.push({
          ...file,
          content: truncated,
          tokens: estimateTokens(truncated),
        });
        totalTokens += estimateTokens(truncated);
        log.warn(`Truncated ${file.name} (${file.tokens} → ${estimateTokens(truncated)} tokens)`);
      } else {
        log.warn(`Dropped ${file.name} (${file.tokens} tokens) — budget exhausted`);
      }
      // Log remaining dropped files
      const idx = loaded.indexOf(file);
      for (let i = idx + 1; i < loaded.length; i++) {
        log.warn(`Dropped ${loaded[i]!.name} (${loaded[i]!.tokens} tokens) — budget exhausted`);
      }
      break;
    }

    included.push(file);
    totalTokens += file.tokens;
  }

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
  if (semiStaticFiles.length > 0) {
    const text = semiStaticFiles
      .map((f) => `## ${f.name}\n\n${f.content}`)
      .join("\n\n---\n\n");
    staticBlocks.push({
      type: "text",
      text,
      cache_control: { type: "ephemeral" },
    });
  }

  // Dynamic group — individual blocks, no caching
  for (const file of dynamicFiles) {
    dynamicBlocks.push({
      type: "text",
      text: `## ${file.name}\n\n${file.content}`,
    });
  }

  log.info(
    `Bootstrap: ${included.length} files, ${totalTokens} tokens ` +
    `(${staticBlocks.length} cached blocks, ${dynamicBlocks.length} dynamic)`,
  );

  return {
    staticBlocks,
    dynamicBlocks,
    totalTokens,
    fileCount: included.length,
  };
}

function truncateSectionAware(content: string, maxTokens: number): string {
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
