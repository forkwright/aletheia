// Resolve user-supplied paths against workspace root with traversal guard
import { resolve, sep } from "node:path";

export function safePath(workspace: string, userPath: string, allowedRoots?: string[]): string {
  const resolved = resolve(workspace, userPath);
  const normalizedWorkspace = workspace.endsWith(sep) ? workspace : workspace + sep;

  if (resolved === workspace || resolved.startsWith(normalizedWorkspace)) {
    return resolved;
  }

  if (allowedRoots) {
    for (const root of allowedRoots) {
      const normalizedRoot = root.endsWith(sep) ? root : root + sep;
      if (resolved === root || resolved.startsWith(normalizedRoot)) {
        return resolved;
      }
    }
  }

  throw new Error(`Path traversal blocked: ${userPath} resolves outside workspace`);
}
