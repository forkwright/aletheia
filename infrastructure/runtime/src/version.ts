// Runtime version — read from package.json at startup
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createLogger } from "./koina/logger.js";

const log = createLogger("version");

let cached: string | null = null;

export function getVersion(): string {
  if (cached) return cached;
  try {
    // When bundled: dist/entry.mjs → ../package.json
    // When unbundled: src/version.ts → ../package.json
    const dir = dirname(fileURLToPath(import.meta.url));
    const pkgPath = join(dir, "..", "package.json");
    const pkg = JSON.parse(readFileSync(pkgPath, "utf-8")) as { version: string };
    cached = pkg.version;
  } catch (error) {
    log.debug(`Could not read package.json for version: ${error instanceof Error ? error.message : error}`);
    cached = "0.0.0";
  }
  return cached;
}
