// dianoia/slug.ts — project slug generation and collision detection
import { existsSync } from "node:fs";
import type Database from "better-sqlite3";
import { getProjectDir } from "./project-files.js";

/**
 * Convert a display name to a URL-safe kebab-case slug.
 * Max 64 characters. Matches GitHub/npm naming conventions.
 */
export function toSlug(displayName: string): string {
  return displayName
    .toLowerCase()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")   // remove diacritics
    .replace(/[^a-z0-9\s-]/g, "")      // keep only alphanumeric, space, dash
    .trim()
    .replace(/\s+/g, "-")              // spaces to dashes
    .replace(/-+/g, "-")               // collapse multiple dashes
    .slice(0, 64);
}

/**
 * Check whether a slug is already taken — checks both DB and filesystem.
 * Filesystem check catches orphaned dirs (DB cleared but files remain).
 * Checks only active/non-abandoned projects.
 */
export function isSlugTaken(slug: string, db: Database.Database): boolean {
  const existing = db
    .prepare(
      "SELECT id FROM planning_projects WHERE project_dir = ? AND state NOT IN ('abandoned', 'complete')",
    )
    .get(slug);
  if (existing) return true;
  // Filesystem check catches orphaned dirs — skip gracefully if paths not initialized (test env)
  try {
    return existsSync(getProjectDir(slug));
  } catch {
    return false;
  }
}
