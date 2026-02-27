// Diagnostic checks for `aletheia doctor`
import { execSync } from "node:child_process";
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
  } catch (error) {
    return {
      name: "config_valid",
      status: "error",
      message: `Config invalid: ${error instanceof Error ? error.message : String(error)}`,
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
    } catch (error) {
      failed.push(`${r.name}: ${error instanceof Error ? error.message : String(error)}`);
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

// ── New async doctor checks ─────────────────────────────────────────────────

export interface CheckResult {
  name: string;
  pass: boolean;
  hint?: string;
}

const isTTY = Boolean(process.stdout.isTTY);
const SYM_PASS = isTTY ? "\x1b[32m✓\x1b[0m" : "PASS";
const SYM_FAIL = isTTY ? "\x1b[31m✗\x1b[0m" : "FAIL";

function sectionHeader(label: string): string {
  return `\n── ${label} ──\n`;
}

function formatCheckLine(label: string, pass: boolean, hint?: string): string {
  const sym = pass ? SYM_PASS : SYM_FAIL;
  const paddedLabel = label.padEnd(18);
  const hintStr = (!pass && hint) ? `  — ${hint}` : "";
  return `  ${sym}  ${paddedLabel}${hintStr}`;
}

export async function runConnectivityChecks(): Promise<CheckResult[]> {
  let port = 18789;
  try {
    const config = loadConfig();
    port = config.gateway?.port ?? 18789;
  } catch { /* config unavailable — use default port */ }

  const endpoints: Array<{ name: string; url: string }> = [
    { name: "gateway", url: `http://localhost:${port}/health` },
    { name: "qdrant", url: "http://localhost:6333/healthz" },
    { name: "neo4j", url: "http://localhost:7474/" },
    { name: "mem0", url: "http://localhost:8000/health" },
  ];

  return Promise.all(
    endpoints.map(async ({ name, url }): Promise<CheckResult> => {
      try {
        const res = await fetch(url, { signal: AbortSignal.timeout(3_000) });
        if (res.ok) return { name, pass: true };
        return { name, pass: false, hint: `HTTP ${res.status} — check service logs` };
      } catch (error) {
        const msg = error instanceof Error ? error.message : String(error);
        const hint = msg.includes("ECONNREFUSED") || msg.includes("ENOTFOUND")
          ? "run: aletheia start"
          : msg.includes("TimeoutError") || msg.includes("timed out")
            ? "service unreachable within 3s — check if running"
            : "check service logs";
        return { name, pass: false, hint };
      }
    }),
  );
}

export function runDependencyChecks(): CheckResult[] {
  const results: CheckResult[] = [];

  const nodeVersion = process.version;
  const majorStr = nodeVersion.slice(1).split(".").at(0) ?? "0";
  const major = parseInt(majorStr, 10);
  results.push(major >= 22
    ? { name: "node", pass: true }
    : { name: "node", pass: false, hint: `v${major} found — Node 22+ required` });

  let hasContainer = false;
  for (const cmd of ["docker", "podman"]) {
    try {
      execSync(`command -v ${cmd}`, { stdio: "ignore" });
      hasContainer = true;
      break;
    } catch { /* not found */ }
  }
  results.push(hasContainer
    ? { name: "docker/podman", pass: true }
    : { name: "docker/podman", pass: false, hint: "install Docker or Podman" });

  const artifactPath = join(paths.root, "infrastructure", "runtime", "dist", "entry.mjs");
  const artifactExists = existsSync(artifactPath);
  results.push(artifactExists
    ? { name: "build artifact", pass: true }
    : { name: "build artifact", pass: false, hint: "run: cd infrastructure/runtime && npx tsdown" });

  return results;
}

export function runBootPersistenceChecks(): CheckResult[] {
  const platform = process.platform;
  const results: CheckResult[] = [];

  if (platform === "darwin") {
    const uid = (process.getuid?.() ?? 501).toString();
    const gatewayEnabled = isLaunchdLoaded("com.aletheia.gateway", uid);
    const memoryEnabled = isLaunchdLoaded("com.aletheia.memory", uid);
    results.push(gatewayEnabled
      ? { name: "boot:gateway", pass: true }
      : { name: "boot:gateway", pass: false, hint: "run: aletheia enable" });
    results.push(memoryEnabled
      ? { name: "boot:memory", pass: true }
      : { name: "boot:memory", pass: false, hint: "run: aletheia enable" });
  } else {
    const gatewayEnabled = isSystemdEnabled("aletheia.service");
    const memoryEnabled = isSystemdEnabled("aletheia-memory.service");
    results.push(gatewayEnabled
      ? { name: "boot:gateway", pass: true }
      : { name: "boot:gateway", pass: false, hint: "run: aletheia enable" });
    results.push(memoryEnabled
      ? { name: "boot:memory", pass: true }
      : { name: "boot:memory", pass: false, hint: "run: aletheia enable" });
  }

  return results;
}

function isLaunchdLoaded(label: string, uid: string): boolean {
  try {
    execSync(`launchctl print gui/${uid}/${label}`, { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function isSystemdEnabled(unit: string): boolean {
  try {
    const out = execSync(`systemctl --user is-enabled ${unit} 2>/dev/null`, { encoding: "utf-8" }).trim();
    return out === "enabled";
  } catch {
    return false;
  }
}

export function formatDoctorOutput(
  connectivity: CheckResult[],
  dependencies: CheckResult[],
  bootPersistence: CheckResult[],
): string {
  const lines: string[] = [];
  const allChecks = [...connectivity, ...dependencies, ...bootPersistence];
  const passed = allChecks.filter((c) => c.pass).length;
  const failed = allChecks.filter((c) => !c.pass).length;

  const allConnDown = connectivity.length > 0 && connectivity.every((c) => !c.pass);
  if (allConnDown) {
    lines.push(isTTY
      ? "\x1b[33m  Aletheia is not running — try: aletheia start\x1b[0m"
      : "  Aletheia is not running — try: aletheia start");
  }

  lines.push(sectionHeader("Connectivity"));
  for (const c of connectivity) {
    lines.push(formatCheckLine(c.name, c.pass, c.hint));
  }

  lines.push(sectionHeader("Dependencies"));
  for (const c of dependencies) {
    lines.push(formatCheckLine(c.name, c.pass, c.hint));
  }

  lines.push(sectionHeader("Boot Persistence"));
  for (const c of bootPersistence) {
    lines.push(formatCheckLine(c.name, c.pass, c.hint));
  }

  lines.push(`\n  ${passed} checks passed, ${failed} failed`);

  return lines.join("\n");
}
