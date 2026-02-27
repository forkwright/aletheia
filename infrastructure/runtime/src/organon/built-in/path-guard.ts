// Path containment guard — prevents tools from accessing files outside workspace or allowedRoots
import { resolve, normalize } from "node:path";
import type { ToolContext } from "../registry.js";

/**
 * Resolve a file path and verify it falls within the workspace or allowedRoots.
 * Throws if the resolved path escapes containment.
 */
export function guardPath(filePath: string, context: ToolContext): string {
  const resolved = normalize(resolve(context.workspace, filePath));
  const workspace = normalize(context.workspace);
  const roots = [workspace, ...(context.allowedRoots ?? [])].map(normalize);

  for (const root of roots) {
    if (resolved === root || resolved.startsWith(root + "/")) {
      return resolved;
    }
  }

  throw new Error(`Path traversal blocked: ${filePath} resolves outside allowed roots`);
}
