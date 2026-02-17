// Workspace path containment — prevents traversal outside allowed roots
import { resolve, relative } from "node:path";

export function safePath(workspace: string, userPath: string, allowedRoots?: string[]): string {
  const resolved = resolve(workspace, userPath);

  // Check workspace first
  const rel = relative(workspace, resolved);
  if (!rel.startsWith("..") && resolve(workspace, rel) === resolved) {
    return resolved;
  }

  // Check additional allowed roots
  if (allowedRoots) {
    for (const root of allowedRoots) {
      const r = relative(root, resolved);
      if (!r.startsWith("..") && resolve(root, r) === resolved) {
        return resolved;
      }
    }
  }

  throw new Error(`Path outside workspace: ${userPath}`);
}
