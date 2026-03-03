// Path containment guard — prevents tools from accessing files outside workspace or allowedRoots
import { resolve, normalize } from "node:path";
import type { ToolContext } from "../registry.js";

/**
 * Resolve a file path and verify it falls within the workspace or allowedRoots.
 * When pathGuard is false, resolves only — no containment check.
 */
export function guardPath(filePath: string, context: ToolContext): string {
  const resolved = normalize(resolve(context.workspace, filePath));

  // When guard is disabled, just resolve the path
  if (context.pathGuard === false) return resolved;

  const workspace = normalize(context.workspace);
  const roots = [workspace, ...(context.allowedRoots ?? [])].map(normalize);

  for (const root of roots) {
    if (resolved === root || resolved.startsWith(root + "/")) {
      return resolved;
    }
  }

  throw new Error(`Path traversal blocked: ${filePath} resolves outside allowed roots`);
}
