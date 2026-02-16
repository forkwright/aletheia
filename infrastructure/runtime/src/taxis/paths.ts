// Config path resolution
import { join } from "node:path";
import { homedir } from "node:os";

const ALETHEIA_ROOT = process.env["ALETHEIA_ROOT"] ?? "/mnt/ssd/aletheia";

export const paths = {
  root: ALETHEIA_ROOT,
  nous: join(ALETHEIA_ROOT, "nous"),
  shared: join(ALETHEIA_ROOT, "shared"),
  sharedBin: join(ALETHEIA_ROOT, "shared", "bin"),
  sharedConfig: join(ALETHEIA_ROOT, "shared", "config"),
  sharedMemory: join(ALETHEIA_ROOT, "shared", "memory"),
  infrastructure: join(ALETHEIA_ROOT, "infrastructure"),

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
