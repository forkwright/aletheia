// Runtime version — read from package.json at startup
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

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
  } catch {
    cached = "0.0.0";
  }
  return cached;
}
