// Config path resolution
import { join } from "node:path";
import { homedir } from "node:os";
import { ConfigError } from "../koina/errors.js";

const ALETHEIA_ROOT = process.env["ALETHEIA_ROOT"] ?? join(homedir(), ".aletheia");

export const paths = {
  root: ALETHEIA_ROOT,
  nous: join(ALETHEIA_ROOT, "nous"),
  shared: join(ALETHEIA_ROOT, "shared"),
  sharedBin: join(ALETHEIA_ROOT, "shared", "bin"),
  sharedConfig: join(ALETHEIA_ROOT, "shared", "config"),
  sharedMemory: join(ALETHEIA_ROOT, "shared", "memory"),
  infrastructure: join(ALETHEIA_ROOT, "infrastructure"),
  pluginRoot: process.env["ALETHEIA_PLUGIN_ROOT"] ?? join(ALETHEIA_ROOT, "shared", "plugins"),

  configDir(): string {
    return (
      process.env["ALETHEIA_CONFIG_DIR"] ??
      join(homedir(), ".aletheia")
    );
  },

  configFile(): string {
    return join(this.configDir(), "aletheia.json");
  },

  nousDir(nousId: string): string {
    return join(ALETHEIA_ROOT, "nous", nousId);
  },

  nousFile(nousId: string, filename: string): string {
    return join(this.nousDir(nousId), filename);
  },

  sessionsDir(): string {
    return join(this.configDir(), "sessions");
  },

  sessionsDb(): string {
    return join(this.configDir(), "sessions.db");
  },

  agentSessionsDir(nousId: string): string {
    return join(this.configDir(), "agents", nousId, "sessions");
  },
} as const;

// Anchor-based path resolution — set once by initPaths() at startup
let _nousDir: string | null = null;
let _deployDir: string | null = null;

export function initPaths(anchor: { nousDir: string; deployDir: string }): void {
  _nousDir = anchor.nousDir;
  _deployDir = anchor.deployDir;
}

export function nousSharedDir(): string {
  if (_nousDir === null) {
    throw new ConfigError("nousSharedDir() called before initPaths()", {
      code: "CONFIG_ANCHOR_NOT_FOUND",
    });
  }
  return _nousDir;
}

export function deployDir(): string {
  if (_deployDir === null) {
    throw new ConfigError("deployDir() called before initPaths()", {
      code: "CONFIG_ANCHOR_NOT_FOUND",
    });
  }
  return _deployDir;
}

export function nousAgentDir(nousId: string): string {
  return join(nousSharedDir(), nousId);
}
