// plans-db.ts — standalone plans.db opener with one-time migration
import Database from "better-sqlite3";
import { copyFileSync, existsSync, mkdirSync, renameSync } from "node:fs";
import { dirname, join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { paths } from "../taxis/paths.js";
import {
  PLANNING_V20_DDL,
  PLANNING_V21_MIGRATION,
  PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION,
  PLANNING_V24_MIGRATION,
  PLANNING_V25_MIGRATION,
  PLANNING_V26_MIGRATION,
  PLANNING_V27_MIGRATION,
  PLANNING_V28_MIGRATION,
  PLANNING_V29_MIGRATION,
  PLANNING_V31_MIGRATION,
} from "./schema.js";

const log = createLogger("dianoia:plans-db");

export function plansDbPath(): string {
  return paths.planningDb();
}

/**
 * Legacy path where plans.db lived before the workspace consolidation (pre-0.11).
 * Check for this first since it may contain data that sessions.db doesn't.
 */
function legacyPlansDbPath(): string {
  return join(paths.root, "nous", "_shared", "workspace", "plans.db");
}

/**
 * One-time migration: move legacy plans.db or copy from sessions.db.
 *
 * Priority order:
 * 1. Legacy plans.db at nous/_shared/workspace/plans.db — rename to new location
 * 2. sessions.db with planning tables — VACUUM INTO new location
 * 3. Neither — fresh DB created by caller
 */
function migratePlansDb(sessionsDbPath: string, targetPath: string): void {
  if (existsSync(targetPath)) return; // already migrated

  // Priority 1: legacy standalone plans.db (has the real data)
  const legacyPath = legacyPlansDbPath();
  if (existsSync(legacyPath)) {
    // Copy rather than rename — the old path may be on a different filesystem,
    // and we want to leave the original intact until confirmed working
    copyFileSync(legacyPath, targetPath);
    // Also copy WAL/SHM if present
    for (const suffix of ["-wal", "-shm"]) {
      if (existsSync(legacyPath + suffix)) {
        copyFileSync(legacyPath + suffix, targetPath + suffix);
      }
    }
    log.info("Migrated legacy plans.db to new location", { from: legacyPath, to: targetPath });

    // Rename original to .migrated so it's not picked up again
    renameSync(legacyPath, legacyPath + ".migrated");
    for (const suffix of ["-wal", "-shm"]) {
      if (existsSync(legacyPath + suffix)) {
        try { renameSync(legacyPath + suffix, legacyPath + suffix + ".migrated"); } catch { /* ignore */ }
      }
    }
    return;
  }

  // Priority 2: extract from sessions.db
  if (!existsSync(sessionsDbPath)) return;

  const sessionsDb = new Database(sessionsDbPath, { readonly: true });
  const hasPlanningTables = sessionsDb
    .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='planning_projects'")
    .get();

  if (!hasPlanningTables) {
    sessionsDb.close();
    log.info("sessions.db has no planning tables — creating fresh plans.db");
    return;
  }

  // VACUUM INTO produces a clean WAL-free copy of the full sessions.db.
  // plans.db will contain ALL tables initially; the planning tables are what we want.
  // Session tables in plans.db are harmless — they will never be written to.
  sessionsDb.exec(`VACUUM INTO '${targetPath}'`);
  sessionsDb.close();
  log.info("plans.db created from sessions.db via VACUUM INTO", { target: targetPath });
}

/**
 * Open (or create) the standalone plans.db.
 * Runs one-time migration from legacy path or sessionsDbPath if plans.db is absent.
 * Applies all PLANNING_V* migrations to the opened DB.
 */
export function openPlansDb(sessionsDbPath: string): Database.Database {
  const targetPath = plansDbPath();
  const dir = dirname(targetPath);
  mkdirSync(dir, { recursive: true });

  // One-time migration if plans.db doesn't exist yet
  if (!existsSync(targetPath)) {
    migratePlansDb(sessionsDbPath, targetPath);
  }

  const db = new Database(targetPath);
  db.pragma("journal_mode = WAL");
  db.pragma("foreign_keys = ON");

  // Apply all planning migrations — each is idempotent (CREATE TABLE IF NOT EXISTS or ALTER TABLE)
  const currentVersion = (db.pragma("user_version", { simple: true }) as number) ?? 0;

  const migrations: Array<[number, string]> = [
    [20, PLANNING_V20_DDL],
    [21, PLANNING_V21_MIGRATION],
    [22, PLANNING_V22_MIGRATION],
    [23, PLANNING_V23_MIGRATION],
    [24, PLANNING_V24_MIGRATION],
    [25, PLANNING_V25_MIGRATION],
    [26, PLANNING_V26_MIGRATION],
    [27, PLANNING_V27_MIGRATION],
    [28, PLANNING_V28_MIGRATION],
    [29, PLANNING_V29_MIGRATION],
    [31, PLANNING_V31_MIGRATION],
  ];

  for (const [version, sql] of migrations) {
    if (currentVersion < version) {
      try {
        db.exec(sql);
        db.pragma(`user_version = ${version}`);
        log.info(`Applied planning migration V${version}`);
      } catch (err) {
        // Migration may already be applied if DB was created via VACUUM INTO — log and continue
        log.warn(`Planning migration V${version} skipped (likely already applied)`, { err });
      }
    }
  }

  return db;
}
