// One-time migration from ~/.aletheia/ to oikos instance layout ($ALETHEIA_ROOT)
// Copies state files that existed before PR #373 migrated path resolution.
// See: https://github.com/forkwright/aletheia/issues/392
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { paths } from "./paths.js";

const log = createLogger("taxis:migrate-legacy");

interface MigrationEntry {
  /** Source path relative to ~/.aletheia/ */
  src: string;
  /** Destination path relative to $ALETHEIA_ROOT/ */
  dest: string;
  /** If true, copy entire directory contents */
  dir?: boolean;
}

const MIGRATIONS: MigrationEntry[] = [
  { src: ".setup-complete", dest: "config/.setup-complete" },
  { src: "session.key", dest: "config/session.key" },
  { src: "sessions.db", dest: "data/sessions.db" },
  { src: "credentials", dest: "config/credentials", dir: true },
];

function copyDir(src: string, dest: string): number {
  mkdirSync(dest, { recursive: true });
  let count = 0;
  for (const entry of readdirSync(src)) {
    const srcPath = join(src, entry);
    const destPath = join(dest, entry);
    if (statSync(srcPath).isFile() && !existsSync(destPath)) {
      copyFileSync(srcPath, destPath);
      count++;
    }
  }
  return count;
}

/**
 * Check for legacy ~/.aletheia/ state files and copy them to the oikos
 * instance layout if the destination doesn't already have them.
 *
 * Safe to call on every startup — skips if legacy dir doesn't exist
 * or all files are already present at the destination.
 */
export function migrateLegacyPaths(): void {
  const legacyRoot = join(homedir(), ".aletheia");
  if (!existsSync(legacyRoot)) return; // no legacy install

  const instanceRoot = paths.root;
  let migrated = 0;

  for (const entry of MIGRATIONS) {
    const srcPath = join(legacyRoot, entry.src);
    const destPath = join(instanceRoot, entry.dest);

    if (!existsSync(srcPath)) continue;
    if (existsSync(destPath)) continue; // already migrated or created fresh

    if (entry.dir) {
      if (!statSync(srcPath).isDirectory()) continue;
      const count = copyDir(srcPath, destPath);
      if (count > 0) {
        log.info(`Migrated ${count} files from ${srcPath} → ${destPath}`);
        migrated += count;
      }
    } else {
      mkdirSync(join(destPath, ".."), { recursive: true });
      copyFileSync(srcPath, destPath);
      log.info(`Migrated ${srcPath} → ${destPath}`);
      migrated++;
    }
  }

  if (migrated > 0) {
    log.info(`Legacy migration complete: ${migrated} file(s) copied from ~/.aletheia/ to ${instanceRoot}`);
  }
}
