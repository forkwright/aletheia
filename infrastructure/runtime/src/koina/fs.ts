// Filesystem utilities
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export function readText(path: string): string | null {
  try {
    return readFileSync(path, "utf-8");
  } catch { /* file missing or unreadable */
    return null;
  }
}

export function readJson<T = unknown>(path: string): T | null {
  const text = readText(path);
  if (text === null) return null;
  try {
    return JSON.parse(text) as T;
  } catch { /* malformed JSON */
    return null;
  }
}

export function writeText(path: string, content: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content, "utf-8");
}

export function writeJson(path: string, data: unknown): void {
  writeText(path, JSON.stringify(data, null, 2) + "\n");
}

export function exists(path: string): boolean {
  return existsSync(path);
}
