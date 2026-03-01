// Oikos instance path resolution
import { dirname, join } from "node:path";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

function discoverRepoRoot(): string {
  let dir = dirname(fileURLToPath(import.meta.url));
  for (let i = 0; i < 10; i++) {
    if (existsSync(join(dir, "instance.example"))) return dir;
    const parent = dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  // Fallback: 4 levels up from taxis/ (infrastructure/runtime/src/taxis → repo root)
  return join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
}

const REPO_ROOT = discoverRepoRoot();
const INSTANCE_ROOT = process.env["ALETHEIA_ROOT"] ?? join(REPO_ROOT, "instance");

export const paths = {
  root: INSTANCE_ROOT,

  // Tier 0: Human + nous collaborative
  theke: join(INSTANCE_ROOT, "theke"),

  // Tier 1: Nous-only shared
  shared: join(INSTANCE_ROOT, "shared"),
  sharedBin: join(INSTANCE_ROOT, "shared", "bin"),
  sharedTools: join(INSTANCE_ROOT, "shared", "tools"),
  sharedSkills: join(INSTANCE_ROOT, "shared", "skills"),
  sharedHooks: join(INSTANCE_ROOT, "shared", "hooks"),
  sharedTemplates: join(INSTANCE_ROOT, "shared", "templates"),
  sharedCalibration: join(INSTANCE_ROOT, "shared", "calibration"),
  sharedSchemas: join(INSTANCE_ROOT, "shared", "schemas"),
  coordination: join(INSTANCE_ROOT, "shared", "coordination"),

  // Tier 2: Per-nous workspaces
  nous: join(INSTANCE_ROOT, "nous"),

  // Config
  config: join(INSTANCE_ROOT, "config"),
  credentials: join(INSTANCE_ROOT, "config", "credentials"),

  // Data (runtime stores)
  data: join(INSTANCE_ROOT, "data"),

  // Logs
  logs: join(INSTANCE_ROOT, "logs"),

  // Plugins
  pluginRoot: process.env["ALETHEIA_PLUGIN_ROOT"] ?? join(INSTANCE_ROOT, "shared", "plugins"),

  // Repo root (for infrastructure/ paths outside instance)
  repoRoot: REPO_ROOT,
  infrastructure: join(REPO_ROOT, "infrastructure"),

  configDir(): string {
    return process.env["ALETHEIA_CONFIG_DIR"] ?? join(INSTANCE_ROOT, "config");
  },

  configFile(): string {
    return join(this.configDir(), "aletheia.json");
  },

  nousDir(nousId: string): string {
    return join(INSTANCE_ROOT, "nous", nousId);
  },

  nousFile(nousId: string, filename: string): string {
    return join(this.nousDir(nousId), filename);
  },

  sessionsDir(): string {
    return join(INSTANCE_ROOT, "data", "sessions");
  },

  sessionsDb(): string {
    return join(INSTANCE_ROOT, "data", "sessions.db");
  },

  agentSessionsDir(nousId: string): string {
    return join(INSTANCE_ROOT, "data", "agents", nousId, "sessions");
  },

  credentialFile(provider: string): string {
    return join(INSTANCE_ROOT, "config", "credentials", `${provider}.json`);
  },

  credentialsDir(): string {
    return join(INSTANCE_ROOT, "config", "credentials");
  },

  sessionKey(): string {
    return join(INSTANCE_ROOT, "config", "session.key");
  },

  planningDb(): string {
    return join(INSTANCE_ROOT, "data", "planning.db");
  },

  plansDir(): string {
    return join(INSTANCE_ROOT, "data", "plans");
  },

  tracesDir(): string {
    return join(INSTANCE_ROOT, "shared", "coordination", "traces");
  },

  statusDir(): string {
    return join(INSTANCE_ROOT, "shared", "coordination", "status");
  },

  evolutionDir(): string {
    return join(INSTANCE_ROOT, "shared", "coordination", "evolution");
  },

  patchesDir(): string {
    return join(INSTANCE_ROOT, "shared", "coordination", "patches");
  },

  prosocheDir(): string {
    return join(INSTANCE_ROOT, "shared", "coordination", "prosoche");
  },

  memoryDir(): string {
    return join(INSTANCE_ROOT, "shared", "coordination", "memory");
  },

  authoredToolsDir(): string {
    return join(INSTANCE_ROOT, "shared", "tools", "authored");
  },
} as const;
