// Load, validate, and resolve config
import { readJson } from "../koina/fs.js";
import { ConfigError } from "../koina/errors.js";
import { createLogger } from "../koina/logger.js";
import { paths } from "./paths.js";
import {
  AletheiaConfigSchema,
  type AletheiaConfig,
  type NousConfig,
} from "./schema.js";

const log = createLogger("taxis");

export function loadConfig(configPath?: string): AletheiaConfig {
  const file = configPath ?? paths.configFile();
  log.info(`Loading config from ${file}`);

  const raw = readJson(file);
  if (raw === null) {
    throw new ConfigError(`Config file not found: ${file}`);
  }

  const result = AletheiaConfigSchema.safeParse(raw);
  if (!result.success) {
    const issues = result.error.issues
      .map((i) => `  ${i.path.join(".")}: ${i.message}`)
      .join("\n");
    throw new ConfigError(`Invalid config:\n${issues}`);
  }

  const config = result.data;

  warnUnknownKeys(raw as Record<string, unknown>);

  log.info(
    `Loaded ${config.agents.list.length} nous, ${config.bindings.length} bindings`,
  );
  return config;
}

export function resolveNous(
  config: AletheiaConfig,
  nousId: string,
): NousConfig | undefined {
  return config.agents.list.find((n) => n.id === nousId);
}

export function resolveDefaultNous(
  config: AletheiaConfig,
): NousConfig | undefined {
  return (
    config.agents.list.find((n) => n.default) ?? config.agents.list[0]
  );
}

export function resolveModel(config: AletheiaConfig, nous?: NousConfig): string {
  if (nous?.model) {
    return typeof nous.model === "string"
      ? nous.model
      : nous.model.primary;
  }
  return config.agents.defaults.model.primary;
}

export function resolveWorkspace(
  config: AletheiaConfig,
  nous: NousConfig,
): string {
  return nous.workspace ?? config.agents.defaults.workspace ?? paths.nousDir(nous.id);
}

export function allNousIds(config: AletheiaConfig): string[] {
  return config.agents.list.map((n) => n.id);
}

const KNOWN_TOP_KEYS = new Set([
  "agents", "bindings", "channels", "gateway", "plugins",
  "session", "cron", "models", "env", "watchdog",
]);

const KNOWN_NOUS_KEYS = new Set([
  "id", "default", "name", "workspace", "model", "subagents",
  "tools", "heartbeat", "identity",
]);

function warnUnknownKeys(
  raw: Record<string, unknown>,
): void {
  for (const key of Object.keys(raw)) {
    if (!KNOWN_TOP_KEYS.has(key)) {
      log.warn(`Unknown top-level config key "${key}" — will be preserved but may be a typo`);
    }
  }

  const rawAgents = raw.agents as Record<string, unknown> | undefined;
  const rawList = rawAgents?.list as Array<Record<string, unknown>> | undefined;
  if (rawList) {
    for (const entry of rawList) {
      const nousId = entry.id ?? "unknown";
      for (const key of Object.keys(entry)) {
        if (!KNOWN_NOUS_KEYS.has(key)) {
          log.warn(`Unknown key "${key}" in nous "${nousId}" config — will be preserved but may be a typo`);
        }
      }
    }
  }
}
