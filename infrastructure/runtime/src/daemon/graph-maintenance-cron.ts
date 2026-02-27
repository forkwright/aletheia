// Graph and vector store maintenance cron — wraps sidecar QA scripts
// Runs monthly (or on-demand) to clean up drift in Neo4j + Qdrant:
// 1. Neo4j graph sanity audit (malformed nodes, orphaned edges, schema violations)
// 2. Qdrant near-duplicate deduplication (cosine > 0.95)
// 3. Qdrant orphan purge (points missing required metadata)

import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { createLogger } from "../koina/logger.js";

const log = createLogger("daemon:graph-maintenance");

export interface GraphMaintenanceResult {
  graphSanity: { nodes: number; relationships: number; violations: string[] };
  dedup: { scanned: number; removed: number };
  orphans: { scanned: number; removed: number };
  errors: string[];
  durationMs: number;
}

/**
 * Locate the sidecar scripts directory.
 * Walks up from this file's location to find infrastructure/memory/scripts.
 */
function findScriptsDir(): string {
  // Try relative to the repo root via known paths
  const candidates = [
    join(process.cwd(), "infrastructure/memory/scripts"),
    resolve(import.meta.dirname ?? "", "../../../../memory/scripts"),
  ];

  for (const candidate of candidates) {
    if (existsSync(candidate)) return candidate;
  }

  throw new Error(
    "Cannot find memory scripts directory. Expected at infrastructure/memory/scripts/",
  );
}

/**
 * Locate the Python interpreter for the sidecar venv.
 */
function findPython(): string {
  const venvCandidates = [
    join(process.cwd(), "infrastructure/memory/sidecar/.venv/bin/python"),
    join(process.cwd(), "infrastructure/memory/sidecar/.venv/bin/python3"),
  ];

  for (const candidate of venvCandidates) {
    if (existsSync(candidate)) return candidate;
  }

  // Fall back to system Python
  return "python3";
}

/**
 * Run a Python QA script and capture its output.
 * Returns stdout; throws on non-zero exit.
 */
function runScript(
  python: string,
  scriptPath: string,
  args: string[] = [],
  env: Record<string, string> = {},
): string {
  const cmd = [python, scriptPath, ...args].join(" ");
  log.debug(`Running: ${cmd}`);
  try {
    const output = execSync(cmd, {
      timeout: 120_000, // 2 minutes per script
      encoding: "utf-8",
      env: { ...process.env, ...env },
      stdio: ["pipe", "pipe", "pipe"],
    });
    return output;
  } catch (error) {
    const msg = error instanceof Error ? error.message : String(error);
    throw new Error(`Script failed: ${scriptPath} — ${msg}`);
  }
}

/**
 * Parse graph sanity output for key metrics.
 */
function parseGraphSanity(output: string): GraphMaintenanceResult["graphSanity"] {
  const nodes = parseInt(output.match(/Total nodes:\s*(\d+)/)?.[1] ?? "0", 10);
  const relationships = parseInt(output.match(/Total relationships:\s*(\d+)/)?.[1] ?? "0", 10);

  // Collect lines that look like violations/issues
  const violations: string[] = [];
  for (const line of output.split("\n")) {
    if (
      line.includes("MISSING") ||
      line.includes("orphan") ||
      line.includes("violation") ||
      line.includes("dangling") ||
      line.includes("invalid")
    ) {
      violations.push(line.trim());
    }
  }

  return { nodes, relationships, violations };
}

/**
 * Parse dedup output for removal count.
 */
function parseDedupOutput(output: string): { scanned: number; removed: number } {
  const scanned = parseInt(output.match(/(\d+)\s+(?:total\s+)?points/i)?.[1] ?? "0", 10);
  const removed = parseInt(
    output.match(/(?:removed|deleted|deduped)[:.\s]*(\d+)/i)?.[1] ?? "0",
    10,
  );
  return { scanned, removed };
}

/**
 * Parse orphan purge output for removal count.
 */
function parseOrphanOutput(output: string): { scanned: number; removed: number } {
  const scanned = parseInt(output.match(/(\d+)\s+(?:total\s+)?points/i)?.[1] ?? "0", 10);
  const removed = parseInt(
    output.match(/(?:removed|deleted|purged)[:.\s]*(\d+)/i)?.[1] ?? "0",
    10,
  );
  return { scanned, removed };
}

/**
 * Run full graph and vector store maintenance.
 * Designed to be called from cron (default: monthly at 3am).
 */
export async function runGraphMaintenance(opts?: {
  /** Run orphan purge with --execute (actually delete). Default: true */
  executeOrphans?: boolean;
  /** NEO4J_PASSWORD override (default: from env) */
  neo4jPassword?: string | undefined;
}): Promise<GraphMaintenanceResult> {
  const startMs = Date.now();
  const errors: string[] = [];

  const scriptsDir = findScriptsDir();
  const python = findPython();
  const executeOrphans = opts?.executeOrphans ?? true;
  const neo4jEnv: Record<string, string> = {};
  if (opts?.neo4jPassword) {
    neo4jEnv["NEO4J_PASSWORD"] = opts.neo4jPassword;
  }

  log.info("Starting graph maintenance cycle");

  // 1. Neo4j graph sanity
  let graphSanity: GraphMaintenanceResult["graphSanity"] = { nodes: 0, relationships: 0, violations: [] };
  try {
    const output = runScript(python, join(scriptsDir, "qa-graph-sanity.py"), [], neo4jEnv);
    graphSanity = parseGraphSanity(output);
    log.info(`Graph sanity: ${graphSanity.nodes} nodes, ${graphSanity.relationships} rels, ${graphSanity.violations.length} issues`);
  } catch (error) {
    const msg = `Graph sanity check failed: ${error instanceof Error ? error.message : error}`;
    log.error(msg);
    errors.push(msg);
  }

  // 2. Qdrant near-duplicate deduplication
  let dedup = { scanned: 0, removed: 0 };
  try {
    const output = runScript(python, join(scriptsDir, "qa-dedup-memories.py"));
    dedup = parseDedupOutput(output);
    log.info(`Dedup: scanned ${dedup.scanned}, removed ${dedup.removed} duplicates`);
  } catch (error) {
    const msg = `Dedup failed: ${error instanceof Error ? error.message : error}`;
    log.error(msg);
    errors.push(msg);
  }

  // 3. Qdrant orphan purge
  let orphans = { scanned: 0, removed: 0 };
  try {
    const args = executeOrphans ? ["--execute"] : [];
    const output = runScript(python, join(scriptsDir, "purge-qdrant-orphans.py"), args);
    orphans = parseOrphanOutput(output);
    log.info(`Orphan purge: scanned ${orphans.scanned}, removed ${orphans.removed}`);
  } catch (error) {
    const msg = `Orphan purge failed: ${error instanceof Error ? error.message : error}`;
    log.error(msg);
    errors.push(msg);
  }

  const durationMs = Date.now() - startMs;
  log.info(
    `Graph maintenance complete in ${(durationMs / 1000).toFixed(1)}s: ` +
    `${graphSanity.violations.length} violations, ${dedup.removed} deduped, ${orphans.removed} orphans purged` +
    (errors.length > 0 ? `, ${errors.length} errors` : ""),
  );

  return { graphSanity, dedup, orphans, errors, durationMs };
}
