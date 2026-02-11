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
