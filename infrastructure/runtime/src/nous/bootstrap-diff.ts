// Bootstrap diff detection — file-level change tracking across sessions
import { readFileSync, writeFileSync, appendFileSync, mkdirSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
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
      if (existsSync(hashFile)) {
        const data = JSON.parse(readFileSync(hashFile, "utf-8")) as Record<
          string,
          Record<string, string>
        >;
        if (data[nousId]) previous = data[nousId];
      }
    } catch {
      /* ignore corrupt file */
    }
  }

  hashCache.set(nousId, currentHashes);

  // Persist current hashes
  const statusDir = resolveSharedStatusDir(workspace);
  try {
    mkdirSync(statusDir, { recursive: true });
    const hashFile = join(statusDir, "bootstrap-hashes.json");
    const existing = existsSync(hashFile)
      ? (JSON.parse(readFileSync(hashFile, "utf-8")) as Record<
          string,
          Record<string, string>
        >)
      : {};
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
      log.info(
        `Bootstrap files changed for ${diff.nousId}: ${diff.changed.join(", ")}`,
      );
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
