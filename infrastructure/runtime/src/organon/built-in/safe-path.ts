// Resolve user-supplied paths against workspace root
import { resolve } from "node:path";

export function safePath(workspace: string, userPath: string, _allowedRoots?: string[]): string {
  return resolve(workspace, userPath);
}
