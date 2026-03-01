// plans-db.ts — standalone plans.db opener with one-time migration from sessions.db
import Database from "better-sqlite3";
import { existsSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
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
 * One-time migration: copy planning tables from sessions.db into the new plans.db.
 * Uses VACUUM INTO to produce a WAL-free clean copy, then plans.db applies its own migrations.
 * Safe to call multiple times — exits early if plans.db already exists.
 */
function migratePlansDb(sessionsDbPath: string, targetPath: string): void {
  if (existsSync(targetPath)) return; // already migrated

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
 * Runs one-time migration from sessionsDbPath if plans.db is absent.
 * Applies all PLANNING_V* migrations to the opened DB.
 */
export function openPlansDb(sessionsDbPath: string): Database.Database {
  const targetPath = plansDbPath();
  const dir = dirname(targetPath);
  mkdirSync(dir, { recursive: true });

  // One-time migration if sessions.db exists and plans.db doesn't yet
  if (existsSync(sessionsDbPath) && !existsSync(targetPath)) {
    migratePlansDb(sessionsDbPath, targetPath);
  }

  const db = new Database(targetPath);
  db.pragma("journal_mode = WAL");
  db.pragma("foreign_keys = ON");

  // Apply all planning migrations — each is idempotent (CREATE TABLE IF NOT EXISTS or ALTER TABLE)
  // Run in version order. Use a user_version pragma to track applied migrations.
  const currentVersion = (db.pragma("user_version", { simple: true }) as number) ?? 0;

  // Migration array: [version_number, sql]
  // Version 20 = initial DDL, 21-31 = incremental migrations
  // Apply only if current user_version < migration version
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
