// dianoia/migration.ts — detection and migration of legacy absolute-path projects
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import { toSlug } from "./slug.js";
import { ensureProjectDir, getProjectDir } from "./project-files.js";

const log = createLogger("dianoia:migration");

export interface LegacyProject {
  id: string;
  projectDir: string; // absolute path (old style)
  goal: string;
}

/**
 * Detect projects with old-style absolute paths in project_dir.
 * Only checks non-abandoned, non-complete projects.
 */
export function detectLegacyProjectPaths(db: Database.Database): LegacyProject[] {
  const rows = db
    .prepare(
      `SELECT id, project_dir, goal FROM planning_projects
       WHERE project_dir IS NOT NULL
       AND (project_dir LIKE '/%' OR project_dir LIKE '%.dianoia%')
       AND state NOT IN ('abandoned', 'complete')`,
    )
    .all() as Array<{ id: string; project_dir: string; goal: string }>;

  return rows.map((r) => ({ id: r.id, projectDir: r.project_dir, goal: r.goal }));
}

/**
 * Generate a human-readable migration prompt listing legacy projects.
 */
export function generateMigrationPrompt(projects: LegacyProject[]): string {
  const list = projects
    .map((p) => `  - "${p.goal || p.id}" at ${p.projectDir}`)
    .join("\n");

  return [
    "The following in-flight planning projects are stored at old-style paths and can be migrated",
    "to the new data/plans/{slug}/:",
    "",
    list,
    "",
    "Migrate now? (yes / not now)",
    "  yes — files are copied to the new location, old paths are removed after successful copy",
    "  not now — projects continue working at their current paths; you'll be asked again on next startup",
  ].join("\n");
}

/**
 * Migrate a single legacy project to a new slug-based path.
 * Strategy: copy all files to new path, update DB, delete old path only on success.
 * Safe: old path is never deleted unless copy succeeds.
 */
export function migrateProjectToSlug(
  project: LegacyProject,
  slug: string,
  db: Database.Database,
): void {
  const oldDir = project.projectDir;
  const newDir = getProjectDir(slug);

  if (!existsSync(oldDir)) {
    log.warn(`Legacy project dir does not exist — updating DB only`, { id: project.id, oldDir });
    db.prepare("UPDATE planning_projects SET project_dir = ? WHERE id = ?").run(slug, project.id);
    return;
  }

  // Create new directory structure
  ensureProjectDir(slug);

  // Copy all files recursively from old path to new path
  copyDirRecursive(oldDir, newDir);

  // Update DB to slug
  db.prepare("UPDATE planning_projects SET project_dir = ? WHERE id = ?").run(slug, project.id);

  // Delete old path only after successful copy + DB update
  try {
    rmSync(oldDir, { recursive: true, force: true });
    log.info(`Migrated project ${project.id} from ${oldDir} to ${newDir}`);
  } catch (err) {
    log.warn(`Migration: copied and updated DB but could not remove old path`, { oldDir, err });
    // Non-fatal — old files remain but DB now points to new location
  }
}

/**
 * Derive a slug for migration purposes.
 * Uses the project goal or a truncated ID as the base.
 * Falls back to a suffixed slug if the base is already taken.
 */
export function deriveMigrationSlug(project: LegacyProject, db: Database.Database): string {
  const base = toSlug(project.goal || project.id.slice(0, 16));
  if (!isSlugTakenInMigration(base, project.id, db)) return base;
  // For migration only: append short id suffix to avoid collision
  const candidate = `${base.slice(0, 48)}-${project.id.slice(0, 8)}`;
  return candidate;
}

function isSlugTakenInMigration(slug: string, excludeId: string, db: Database.Database): boolean {
  const existing = db
    .prepare(
      "SELECT id FROM planning_projects WHERE project_dir = ? AND id != ? AND state NOT IN ('abandoned', 'complete')",
    )
    .get(slug, excludeId);
  if (existing) return true;
  try {
    return existsSync(getProjectDir(slug));
  } catch {
    return false;
  }
}

function copyDirRecursive(src: string, dest: string): void {
  mkdirSync(dest, { recursive: true });
  for (const entry of readdirSync(src, { withFileTypes: true })) {
    const srcPath = join(src, entry.name);
    const destPath = join(dest, entry.name);
    if (entry.isDirectory()) {
      copyDirRecursive(srcPath, destPath);
    } else {
      copyFileSync(srcPath, destPath);
    }
  }
}
