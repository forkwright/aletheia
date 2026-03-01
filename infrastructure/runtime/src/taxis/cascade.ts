// Oikos three-tier cascade resolution
// Resolution order (most specific wins):
//   1. instance/nous/{id}/{subdir}/  — agent-specific
//   2. instance/shared/{subdir}/     — shared across all agents
//   3. instance/theke/{subdir}/      — human + agent collaborative
//
// See: docs/specs/44_oikos.md

import { existsSync, readdirSync, statSync } from "node:fs";
import { join, extname } from "node:path";
import { createLogger } from "../koina/logger.js";
import { paths } from "./paths.js";

const log = createLogger("taxis.cascade");

export type CascadeTier = "nous" | "shared" | "theke";

export interface CascadeResult {
  /** Absolute file path */
  path: string;
  /** Which tier it came from */
  tier: CascadeTier;
  /** Filename (basename) */
  name: string;
}

/**
 * Walk the three-tier oikos cascade and return discovered files.
 *
 * When a filename exists in multiple tiers, only the most-specific version
 * is returned (nous > shared > theke). All tiers are always walked — the
 * deduplication happens after collection.
 *
 * @param nousId - Agent ID for tier-1 (nous-specific) resolution
 * @param subdir - Subdirectory name within each tier (e.g. "tools", "hooks", "templates", "commands")
 * @param ext - Optional file extension filter (e.g. ".md", ".yaml"). If omitted, returns all files.
 */
export function cascadeDiscover(nousId: string, subdir: string, ext?: string): CascadeResult[] {
  const tiers: { tier: CascadeTier; dir: string }[] = [
    { tier: "nous",   dir: join(paths.nousDir(nousId), subdir) },
    { tier: "shared", dir: join(paths.shared, subdir) },
    { tier: "theke",  dir: join(paths.theke, subdir) },
  ];

  // Collect all files, most-specific first
  const seen = new Map<string, CascadeResult>();

  for (const { tier, dir } of tiers) {
    if (!existsSync(dir)) continue;

    let entries: string[];
    try {
      entries = readdirSync(dir);
    } catch {
      continue;
    }

    for (const entry of entries) {
      const fullPath = join(dir, entry);

      // Skip directories, hidden files, and non-matching extensions
      try {
        if (!statSync(fullPath).isFile()) continue;
      } catch {
        continue;
      }
      if (entry.startsWith(".")) continue;
      if (ext && extname(entry) !== ext) continue;

      // Most-specific wins — only store if not already seen
      if (!seen.has(entry)) {
        seen.set(entry, { path: fullPath, tier, name: entry });
      }
    }
  }

  const results = [...seen.values()];
  if (results.length > 0) {
    log.debug(
      `Cascade ${subdir}: discovered ${results.length} files for ${nousId} ` +
      `(${results.filter(r => r.tier === "nous").length} nous, ` +
      `${results.filter(r => r.tier === "shared").length} shared, ` +
      `${results.filter(r => r.tier === "theke").length} theke)`,
    );
  }

  return results;
}

/**
 * Resolve a single named file through the cascade.
 * Returns the most-specific path, or null if not found in any tier.
 *
 * Resolution order: nous/{id}/ → shared/ → theke/ → null
 *
 * @param nousId - Agent ID
 * @param filename - File to resolve (e.g. "USER.md", "tools.yaml")
 * @param subdir - Optional subdirectory within each tier
 */
export function cascadeResolve(nousId: string, filename: string, subdir?: string): string | null {
  const candidates = subdir
    ? [
        join(paths.nousDir(nousId), subdir, filename),
        join(paths.shared, subdir, filename),
        join(paths.theke, subdir, filename),
      ]
    : [
        join(paths.nousDir(nousId), filename),
        join(paths.shared, filename),
        join(paths.theke, filename),
      ];

  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      log.debug(`Cascade resolve ${filename}: found at ${candidate}`);
      return candidate;
    }
  }

  return null;
}

/**
 * Resolve all instances of a named file across all tiers.
 * Unlike cascadeResolve (which returns first match), this returns all matches
 * ordered most-specific first. Useful when you want to merge content from
 * multiple tiers (e.g. config deep-merge).
 */
export function cascadeResolveAll(nousId: string, filename: string, subdir?: string): CascadeResult[] {
  const tiers: { tier: CascadeTier; path: string }[] = subdir
    ? [
        { tier: "nous",   path: join(paths.nousDir(nousId), subdir, filename) },
        { tier: "shared", path: join(paths.shared, subdir, filename) },
        { tier: "theke",  path: join(paths.theke, subdir, filename) },
      ]
    : [
        { tier: "nous",   path: join(paths.nousDir(nousId), filename) },
        { tier: "shared", path: join(paths.shared, filename) },
        { tier: "theke",  path: join(paths.theke, filename) },
      ];

  return tiers
    .filter(t => existsSync(t.path))
    .map(t => ({ path: t.path, tier: t.tier, name: filename }));
}
