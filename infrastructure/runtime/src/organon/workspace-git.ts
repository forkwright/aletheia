// Workspace git tracking — auto-commit file changes for history and rollback
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { basename, join, relative } from "node:path";
import { createLogger } from "../koina/logger.js";

const log = createLogger("organon.workspace-git");

const COMMIT_TIMEOUT = 5000;

export function initWorkspaceRepo(workspace: string): boolean {
  if (existsSync(join(workspace, ".git"))) return true;

  try {
    execFileSync("git", ["init"], { cwd: workspace, timeout: COMMIT_TIMEOUT, stdio: "ignore" });
    execFileSync("git", ["config", "user.email", "aletheia@local"], { cwd: workspace, timeout: COMMIT_TIMEOUT, stdio: "ignore" });
    execFileSync("git", ["config", "user.name", "aletheia"], { cwd: workspace, timeout: COMMIT_TIMEOUT, stdio: "ignore" });
    log.info(`Initialized git repo in ${workspace}`);
    return true;
  } catch (err) {
    log.warn(`Failed to init git in ${workspace}: ${err instanceof Error ? err.message : err}`);
    return false;
  }
}

export function commitWorkspaceChange(
  workspace: string,
  filePath: string,
  operation: string,
): void {
  try {
    if (!existsSync(join(workspace, ".git"))) {
      if (!initWorkspaceRepo(workspace)) return;
    }

    const rel = relative(workspace, filePath);
    // Only track files inside the workspace
    if (rel.startsWith("..")) return;

    execFileSync("git", ["add", rel], { cwd: workspace, timeout: COMMIT_TIMEOUT, stdio: "ignore" });
    execFileSync("git", ["diff", "--cached", "--quiet"], { cwd: workspace, timeout: COMMIT_TIMEOUT, stdio: "ignore" });
    // If git diff --cached --quiet exits 0, nothing staged — skip commit
  } catch {
    // Exit code 1 from git diff means there ARE staged changes — commit them
    try {
      const rel = relative(workspace, filePath);
      const name = basename(rel);
      execFileSync(
        "git", ["commit", "-m", `${operation}: ${name}`, "--no-gpg-sign"],
        { cwd: workspace, timeout: COMMIT_TIMEOUT, stdio: "ignore" },
      );
      log.debug(`Committed workspace change: ${operation}: ${name}`);
    } catch (err) {
      log.debug(`Workspace commit skipped: ${err instanceof Error ? err.message : err}`);
    }
  }
}
