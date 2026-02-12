// Workspace path containment — prevents traversal outside agent workspace
import { resolve, relative } from "node:path";

export function safePath(workspace: string, userPath: string): string {
  const resolved = resolve(workspace, userPath);
  const rel = relative(workspace, resolved);
  if (rel.startsWith("..") || resolve(workspace, rel) !== resolved) {
    throw new Error(`Path outside workspace: ${userPath}`);
  }
  return resolved;
}
