// Bootstrap diff detection — file-level change tracking across sessions
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { createLogger } from "../koina/logger.js";

const log = createLogger("nous.bootstrap-diff");

export interface BootstrapDiff {
  timestamp: string;
  nousId: string;
  added: string[];
  removed: string[];
  changed: string[];
}

const hashCache = new Map<string, Record<string, string>>();

function resolveSharedStatusDir(workspace: string): string {
  // workspace = /mnt/ssd/aletheia/nous/syn → shared/status = /mnt/ssd/aletheia/shared/status
  return join(dirname(workspace), "..", "shared", "status");
}

export function detectBootstrapDiff(
  nousId: string,
  currentHashes: Record<string, string>,
  workspace: string,
): BootstrapDiff | null {
  let previous = hashCache.get(nousId);

  if (!previous) {
    const statusDir = resolveSharedStatusDir(workspace);
    const hashFile = join(statusDir, "bootstrap-hashes.json");
    try {
      const data = JSON.parse(readFileSync(hashFile, "utf-8")) as Record<
        string,
        Record<string, string>
      >;
      if (data[nousId]) previous = data[nousId];
    } catch { /* file unreadable — skip diff */
      /* ignore missing or corrupt file */
    }
  }

  hashCache.set(nousId, currentHashes);

  // Persist current hashes
  const statusDir = resolveSharedStatusDir(workspace);
  try {
    mkdirSync(statusDir, { recursive: true });
    const hashFile = join(statusDir, "bootstrap-hashes.json");
    let existing: Record<string, Record<string, string>> = {};
    try {
      existing = JSON.parse(readFileSync(hashFile, "utf-8")) as Record<
        string,
        Record<string, string>
      >;
    } catch { /* hash computation failed — skip */
      /* missing or corrupt — start fresh */
    }
    existing[nousId] = currentHashes;
    writeFileSync(hashFile, JSON.stringify(existing, null, 2));
  } catch (err) {
    log.warn(
      `Failed to persist bootstrap hashes: ${err instanceof Error ? err.message : err}`,
    );
  }

  if (!previous) return null;

  const currentKeys = new Set(Object.keys(currentHashes));
  const previousKeys = new Set(Object.keys(previous));

  const added = [...currentKeys].filter((k) => !previousKeys.has(k));
  const removed = [...previousKeys].filter((k) => !currentKeys.has(k));
  const changed = [...currentKeys].filter(
    (k) => previousKeys.has(k) && currentHashes[k] !== previous![k],
  );

  if (added.length === 0 && removed.length === 0 && changed.length === 0) {
    return null;
  }

  return {
    timestamp: new Date().toISOString(),
    nousId,
    added,
    removed,
    changed,
  };
}

export function logBootstrapDiff(
  diff: BootstrapDiff,
  workspace: string,
): void {
  const statusDir = resolveSharedStatusDir(workspace);
  const logPath = join(statusDir, "bootstrap-changes.jsonl");

  try {
    mkdirSync(statusDir, { recursive: true });
    appendFileSync(logPath, JSON.stringify(diff) + "\n");

    if (diff.removed.length > 0) {
      log.warn(
        `ALERT: Bootstrap files REMOVED for ${diff.nousId}: ${diff.removed.join(", ")}`,
      );
    }
    if (diff.changed.length > 0) {
      // Identify cache-invalidating changes: static files changing between turns
      // forces Anthropic to re-cache the entire prefix (expensive).
      const STATIC_FILES = new Set(["SOUL.md", "USER.md", "AGENTS.md", "IDENTITY.md"]);
      const SEMI_STATIC_FILES = new Set(["GOALS.md", "TOOLS.md", "MEMORY.md", "EVAL_FEEDBACK.md"]);
      const staticChanges = diff.changed.filter((f) => STATIC_FILES.has(f));
      const semiStaticChanges = diff.changed.filter((f) => SEMI_STATIC_FILES.has(f));
      const dynamicChanges = diff.changed.filter((f) => !STATIC_FILES.has(f) && !SEMI_STATIC_FILES.has(f));

      if (staticChanges.length > 0) {
        log.warn(
          `Cache invalidation: static files changed for ${diff.nousId}: ${staticChanges.join(", ")} — ` +
          `full prefix re-cache required (all 4 cache breakpoints invalidated)`,
        );
      }
      if (semiStaticChanges.length > 0) {
        log.info(
          `Cache invalidation: semi-static files changed for ${diff.nousId}: ${semiStaticChanges.join(", ")} — ` +
          `breakpoint 2 re-cached (breakpoint 1 preserved if static unchanged)`,
        );
      }
      if (dynamicChanges.length > 0) {
        log.debug(
          `Dynamic bootstrap files changed for ${diff.nousId}: ${dynamicChanges.join(", ")} (no cache impact)`,
        );
      }
    }
    if (diff.added.length > 0) {
      log.info(
        `Bootstrap files added for ${diff.nousId}: ${diff.added.join(", ")}`,
      );
    }
  } catch (err) {
    log.warn(
      `Failed to log bootstrap diff: ${err instanceof Error ? err.message : err}`,
    );
  }
}
