// Diagnostic checks for `aletheia doctor`
import { existsSync, mkdirSync, readFileSync, statSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { paths } from "../taxis/paths.js";
import { loadConfig } from "../taxis/loader.js";
import type { AletheiaConfig } from "../taxis/schema.js";

export interface DiagnosticResult {
  name: string;
  status: "ok" | "warn" | "error";
  message: string;
  fix?: {
    description: string;
    action: () => void;
  };
}

type DiagnosticCheck = (config: AletheiaConfig | null) => DiagnosticResult;

const checks: DiagnosticCheck[] = [
  checkConfigValid,
  checkWorkspacesExist,
  checkSharedDirs,
  checkSessionsDb,
  checkPluginPaths,
  checkStalePid,
  checkBootstrapFiles,
  checkSignalConfig,
  checkCredentialsDir,
];

function checkConfigValid(_config: AletheiaConfig | null): DiagnosticResult {
  try {
    loadConfig();
    return { name: "config_valid", status: "ok", message: "Config parses and validates" };
  } catch (err) {
    return {
      name: "config_valid",
      status: "error",
      message: `Config invalid: ${err instanceof Error ? err.message : String(err)}`,
    };
  }
}

function checkWorkspacesExist(config: AletheiaConfig | null): DiagnosticResult {
  if (!config) return { name: "workspaces_exist", status: "warn", message: "Skipped (no config)" };

  const missing: string[] = [];
  for (const nous of config.agents.list) {
    const ws = nous.workspace.startsWith("/")
      ? nous.workspace
      : join(paths.root, nous.workspace);
    if (!existsSync(ws)) missing.push(`${nous.id}: ${ws}`);
  }

  if (missing.length === 0) {
    return { name: "workspaces_exist", status: "ok", message: `All ${config.agents.list.length} workspaces exist` };
  }

  return {
    name: "workspaces_exist",
    status: "error",
    message: `Missing workspaces: ${missing.join(", ")}`,
    fix: {
      description: `Create ${missing.length} workspace directories`,
      action: () => {
        for (const nous of config.agents.list) {
          const ws = nous.workspace.startsWith("/")
            ? nous.workspace
            : join(paths.root, nous.workspace);
          if (!existsSync(ws)) mkdirSync(ws, { recursive: true });
        }
      },
    },
  };
}

function checkSharedDirs(_config: AletheiaConfig | null): DiagnosticResult {
  const required = [
    join(paths.shared, "skills"),
    join(paths.shared, "tools", "authored"),
    join(paths.shared, "competence"),
    join(paths.shared, "calibration"),
  ];

  const missing = required.filter((d) => !existsSync(d));
  if (missing.length === 0) {
    return { name: "shared_dirs", status: "ok", message: "All shared directories exist" };
  }

  return {
    name: "shared_dirs",
    status: "warn",
    message: `Missing: ${missing.map((d) => d.replace(paths.root + "/", "")).join(", ")}`,
    fix: {
      description: `Create ${missing.length} shared directories`,
      action: () => {
        for (const d of missing) mkdirSync(d, { recursive: true });
      },
    },
  };
}

function checkSessionsDb(_config: AletheiaConfig | null): DiagnosticResult {
  const dbPath = paths.sessionsDb();
  if (existsSync(dbPath)) {
    try {
      const stat = statSync(dbPath);
      if (stat.size === 0) {
        return { name: "sessions_db", status: "warn", message: "Sessions DB exists but is empty (will be initialized on start)" };
      }
      return { name: "sessions_db", status: "ok", message: `Sessions DB exists (${Math.round(stat.size / 1024)}KB)` };
    } catch { /* service check failed */
      return { name: "sessions_db", status: "error", message: "Sessions DB exists but is not readable" };
    }
  }

  return {
    name: "sessions_db",
    status: "warn",
    message: "Sessions DB does not exist (will be created on first start)",
    fix: {
      description: "Create config directory for sessions DB",
      action: () => {
        const dir = paths.configDir();
        if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
      },
    },
  };
}

function checkPluginPaths(config: AletheiaConfig | null): DiagnosticResult {
  if (!config) return { name: "plugin_paths", status: "warn", message: "Skipped (no config)" };

  const loadPaths = config.plugins.load.paths;
  if (loadPaths.length === 0) {
    return { name: "plugin_paths", status: "ok", message: "No plugin paths configured" };
  }

  const missing = loadPaths.filter((p) => !existsSync(p));
  if (missing.length === 0) {
    return { name: "plugin_paths", status: "ok", message: `All ${loadPaths.length} plugin paths exist` };
  }

  return {
    name: "plugin_paths",
    status: "warn",
    message: `Missing plugin paths: ${missing.join(", ")}`,
    fix: {
      description: `Create ${missing.length} plugin directories`,
      action: () => {
        for (const p of missing) mkdirSync(p, { recursive: true });
      },
    },
  };
}

function checkStalePid(_config: AletheiaConfig | null): DiagnosticResult {
  const pidPath = join(paths.configDir(), "aletheia.pid");
  if (!existsSync(pidPath)) {
    return { name: "stale_pid", status: "ok", message: "No PID file" };
  }

  try {
    const pid = parseInt(readFileSync(pidPath, "utf-8").trim(), 10);
    if (isNaN(pid)) {
      return {
        name: "stale_pid",
        status: "warn",
        message: "PID file contains invalid data",
        fix: {
          description: "Remove invalid PID file",
          action: () => unlinkSync(pidPath),
        },
      };
    }

    try {
      process.kill(pid, 0); // Signal 0 = check if alive
      return { name: "stale_pid", status: "ok", message: `Process ${pid} is running` };
    } catch { /* stat failed — skip */
      return {
        name: "stale_pid",
        status: "warn",
        message: `Stale PID file (process ${pid} not running)`,
        fix: {
          description: "Remove stale PID file",
          action: () => unlinkSync(pidPath),
        },
      };
    }
  } catch { /* diagnostics dir listing failed */
    return { name: "stale_pid", status: "ok", message: "No PID file" };
  }
}

function checkBootstrapFiles(config: AletheiaConfig | null): DiagnosticResult {
  if (!config) return { name: "bootstrap_files", status: "warn", message: "Skipped (no config)" };

  const missing: string[] = [];
  for (const nous of config.agents.list) {
    const ws = nous.workspace.startsWith("/")
      ? nous.workspace
      : join(paths.root, nous.workspace);
    if (!existsSync(ws)) continue; // workspace doesn't exist, caught by other check
    const soul = join(ws, "SOUL.md");
    if (!existsSync(soul)) missing.push(`${nous.id}/SOUL.md`);
  }

  if (missing.length === 0) {
    return { name: "bootstrap_files", status: "ok", message: "All agents have SOUL.md" };
  }

  return {
    name: "bootstrap_files",
    status: "warn",
    message: `Missing: ${missing.join(", ")}`,
    fix: {
      description: `Create ${missing.length} minimal SOUL.md files`,
      action: () => {
        for (const nous of config.agents.list) {
          const ws = nous.workspace.startsWith("/")
            ? nous.workspace
            : join(paths.root, nous.workspace);
          if (!existsSync(ws)) continue;
          const soul = join(ws, "SOUL.md");
          if (!existsSync(soul)) {
            const name = nous.identity?.name ?? nous.name ?? nous.id;
            writeFileSync(soul, `# ${name}\n\nYou are ${name}, an Aletheia agent.\n`, "utf-8");
          }
        }
      },
    },
  };
}

function checkSignalConfig(config: AletheiaConfig | null): DiagnosticResult {
  if (!config) return { name: "signal_config", status: "warn", message: "Skipped (no config)" };

  if (!config.channels.signal.enabled) {
    return { name: "signal_config", status: "ok", message: "Signal disabled" };
  }

  const accounts = Object.entries(config.channels.signal.accounts);
  if (accounts.length === 0) {
    return { name: "signal_config", status: "warn", message: "Signal enabled but no accounts configured" };
  }

  const noAccount = accounts.filter(([, acct]) => !acct.account);
  if (noAccount.length > 0) {
    return {
      name: "signal_config",
      status: "warn",
      message: `Signal accounts without phone number: ${noAccount.map(([k]) => k).join(", ")}`,
    };
  }

  return { name: "signal_config", status: "ok", message: `${accounts.length} Signal account(s) configured` };
}

function checkCredentialsDir(_config: AletheiaConfig | null): DiagnosticResult {
  const credDir = join(paths.configDir(), "credentials");
  if (!existsSync(credDir)) {
    return {
      name: "credentials_dir",
      status: "warn",
      message: "Credentials directory missing",
      fix: {
        description: "Create credentials directory",
        action: () => mkdirSync(credDir, { recursive: true }),
      },
    };
  }
  return { name: "credentials_dir", status: "ok", message: "Credentials directory exists" };
}

export function runDiagnostics(): { results: DiagnosticResult[]; config: AletheiaConfig | null } {
  let config: AletheiaConfig | null = null;
  try {
    config = loadConfig();
  } catch { /* workspace stat failed */
    // Config check itself will report the error
  }

  const results = checks.map((check) => check(config));
  return { results, config };
}

export function applyFixes(results: DiagnosticResult[]): { applied: number; failed: string[] } {
  let applied = 0;
  const failed: string[] = [];

  for (const r of results) {
    if (!r.fix) continue;
    try {
      r.fix.action();
      applied++;
    } catch (err) {
      failed.push(`${r.name}: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  return { applied, failed };
}

export function formatResults(results: DiagnosticResult[], showFixes = false): string {
  const lines: string[] = [];
  const icons = { ok: "+", warn: "!", error: "X" };

  for (const r of results) {
    const icon = icons[r.status];
    lines.push(`  ${icon} ${r.name}: ${r.message}`);
    if (showFixes && r.fix) {
      lines.push(`    fix: ${r.fix.description}`);
    }
  }

  const errors = results.filter((r) => r.status === "error").length;
  const warns = results.filter((r) => r.status === "warn").length;
  const fixable = results.filter((r) => r.fix).length;

  lines.push("");
  if (errors === 0 && warns === 0) {
    lines.push("All checks passed.");
  } else {
    const parts: string[] = [];
    if (errors > 0) parts.push(`${errors} error(s)`);
    if (warns > 0) parts.push(`${warns} warning(s)`);
    if (fixable > 0) parts.push(`${fixable} fixable (run with --fix)`);
    lines.push(parts.join(", "));
  }

  return lines.join("\n");
}
