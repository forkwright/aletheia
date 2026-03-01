// Load, validate, and resolve config
// Supports YAML (.yaml) with JSON (.json) fallback, plus per-agent config cascade.
import { existsSync, readFileSync, type FSWatcher, watch } from "node:fs";
import { join } from "node:path";
import yaml from "js-yaml";
import { readJson } from "../koina/fs.js";
import { ConfigError } from "../koina/errors.js";
import { createLogger } from "../koina/logger.js";
import { paths } from "./paths.js";
import {
  type AletheiaConfig,
  AletheiaConfigSchema,
  type NousConfig,
} from "./schema.js";

const log = createLogger("taxis");

/**
 * Read a config file — tries YAML first, then JSON.
 * Returns the parsed object, or null if the file doesn't exist.
 */
function readConfigFile(basePath: string): Record<string, unknown> | null {
  // Try .yaml first
  const yamlPath = basePath.replace(/\.json$/, ".yaml");
  if (yamlPath !== basePath && existsSync(yamlPath)) {
    try {
      const content = readFileSync(yamlPath, "utf-8");
      const parsed = yaml.load(content);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        log.debug(`Loaded config from YAML: ${yamlPath}`);
        return parsed as Record<string, unknown>;
      }
    } catch (error) {
      log.warn(`Failed to parse YAML config ${yamlPath}: ${error instanceof Error ? error.message : error}`);
    }
  }

  // Fall back to JSON
  return readJson(basePath);
}

/**
 * Deep merge two config objects. Source values override target values.
 * Arrays are replaced (not concatenated). Null values in source delete the key.
 */
export function deepMerge(
  target: Record<string, unknown>,
  source: Record<string, unknown>,
): Record<string, unknown> {
  const result: Record<string, unknown> = { ...target };

  for (const [key, sourceVal] of Object.entries(source)) {
    if (sourceVal === null || sourceVal === undefined) {
      delete result[key];
      continue;
    }

    const targetVal = result[key];

    if (
      typeof sourceVal === "object" &&
      !Array.isArray(sourceVal) &&
      typeof targetVal === "object" &&
      !Array.isArray(targetVal) &&
      targetVal !== null
    ) {
      result[key] = deepMerge(
        targetVal as Record<string, unknown>,
        sourceVal as Record<string, unknown>,
      );
    } else {
      result[key] = sourceVal;
    }
  }

  return result;
}

/**
 * Load per-agent config override from instance/nous/{id}/config.yaml (or .json).
 * Returns null if no override exists.
 */
export function loadNousConfigOverride(nousId: string): Record<string, unknown> | null {
  const nousDir = paths.nousDir(nousId);
  const yamlPath = join(nousDir, "config.yaml");
  const jsonPath = join(nousDir, "config.json");

  if (existsSync(yamlPath)) {
    try {
      const content = readFileSync(yamlPath, "utf-8");
      const parsed = yaml.load(content);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        log.debug(`Loaded nous config override from ${yamlPath}`);
        return parsed as Record<string, unknown>;
      }
    } catch (error) {
      log.warn(`Failed to parse nous config ${yamlPath}: ${error instanceof Error ? error.message : error}`);
    }
  } else if (existsSync(jsonPath)) {
    const parsed = readJson<Record<string, unknown>>(jsonPath);
    if (parsed) {
      log.debug(`Loaded nous config override from ${jsonPath}`);
      return parsed;
    }
  }

  return null;
}

export function loadConfig(configPath?: string): AletheiaConfig {
  const file = configPath ?? paths.configFile();
  log.info(`Loading config from ${file}`);

  const raw = readConfigFile(file);
  if (raw === null) {
    throw new ConfigError(`Config file not found: ${file} (tried .yaml and .json)`, {
      code: "CONFIG_NOT_FOUND", context: { path: file },
    });
  }

  const result = AletheiaConfigSchema.safeParse(raw);
  if (!result.success) {
    const issues = result.error.issues
      .map((i) => `  ${i.path.join(".")}: ${i.message}`)
      .join("\n");
    throw new ConfigError(`Invalid config:\n${issues}`, {
      code: "CONFIG_VALIDATION_FAILED", context: { issueCount: result.error.issues.length },
    });
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

/**
 * Apply env vars from config.env.vars to process.env.
 * Must be called before any module reads process.env (router, providers, etc.).
 * Existing env vars take precedence (won't overwrite).
 */
export function applyEnv(config: AletheiaConfig): number {
  const vars = config.env?.vars;
  if (!vars || Object.keys(vars).length === 0) return 0;

  let applied = 0;
  for (const [key, value] of Object.entries(vars)) {
    if (process.env[key]) {
      log.debug(`env: ${key} already set in environment, skipping`);
      continue;
    }
    process.env[key] = value;
    applied++;
    // Mask sensitive values in log output
    const masked = value.length > 8 ? value.slice(0, 4) + "..." + value.slice(-4) : "****";
    log.info(`env: set ${key} from config (${masked})`);
  }
  return applied;
}

const KNOWN_TOP_KEYS = new Set([
  "agents", "bindings", "channels", "gateway", "plugins",
  "session", "cron", "models", "env", "watchdog", "branding",
  "mcp",
]);

const KNOWN_NOUS_KEYS = new Set([
  "id", "default", "name", "workspace", "model", "params", "subagents",
  "tools", "heartbeat", "identity",
]);

export function tryReloadConfig(configPath?: string): AletheiaConfig | null {
  const file = configPath ?? paths.configFile();
  const raw = readJson(file);
  if (raw === null) {
    log.error(`Config reload failed: file not found ${file}`);
    return null;
  }

  const result = AletheiaConfigSchema.safeParse(raw);
  if (!result.success) {
    const issues = result.error.issues
      .map((i) => `  ${i.path.join(".")}: ${i.message}`)
      .join("\n");
    log.error(`Config reload failed — invalid config:\n${issues}`);
    return null;
  }

  log.info(`Config reloaded: ${result.data.agents.list.length} nous, ${result.data.bindings.length} bindings`);
  return result.data;
}

export function watchConfig(
  configPath: string | undefined,
  onReload: (config: AletheiaConfig) => void,
): FSWatcher | null {
  const file = configPath ?? paths.configFile();
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  try {
    const watcher = watch(file, () => {
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        log.info("Config file changed — attempting reload");
        const newConfig = tryReloadConfig(file);
        if (newConfig) onReload(newConfig);
      }, 1000);
    });

    log.info(`Watching config file for changes: ${file}`);
    return watcher;
  } catch (error) {
    log.warn(`Cannot watch config file: ${error instanceof Error ? error.message : error}`);
    return null;
  }
}

function warnUnknownKeys(
  raw: Record<string, unknown>,
): void {
  for (const key of Object.keys(raw)) {
    if (!KNOWN_TOP_KEYS.has(key)) {
      log.warn(`Unknown top-level config key "${key}" — will be preserved but may be a typo`);
    }
  }

  const rawAgents = raw["agents"] as Record<string, unknown> | undefined;
  const rawList = rawAgents?.["list"] as Array<Record<string, unknown>> | undefined;
  if (rawList) {
    for (const entry of rawList) {
      const nousId = entry["id"] ?? "unknown";
      for (const key of Object.keys(entry)) {
        if (!KNOWN_NOUS_KEYS.has(key)) {
          log.warn(`Unknown key "${key}" in nous "${nousId}" config — will be preserved but may be a typo`);
        }
      }
    }
  }
}
