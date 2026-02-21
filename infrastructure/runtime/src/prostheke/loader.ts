// Plugin loader — discover and load plugins from configured paths
import { existsSync, readdirSync, realpathSync } from "node:fs";
import { join, resolve, sep } from "node:path";
import { createLogger } from "../koina/logger.js";
import { readJson } from "../koina/fs.js";
import type { PluginDefinition, PluginManifest } from "./types.js";

const log = createLogger("prostheke:loader");

export async function loadPlugins(
  paths: string[],
): Promise<PluginDefinition[]> {
  const plugins: PluginDefinition[] = [];

  for (const pluginPath of paths) {
    const resolved = resolve(pluginPath);

    if (!existsSync(resolved)) {
      log.warn(`Plugin path not found: ${resolved}`);
      continue;
    }

    try {
      const plugin = await loadPlugin(resolved);
      if (plugin) {
        plugins.push(plugin);
        log.info(`Loaded plugin: ${plugin.manifest.id} v${plugin.manifest.version}`);
      }
    } catch (err) {
      log.error(
        `Failed to load plugin from ${resolved}: ${err instanceof Error ? err.message : err}`,
      );
    }
  }

  return plugins;
}

async function loadPlugin(
  pluginPath: string,
): Promise<PluginDefinition | null> {
  const manifestPath = findManifest(pluginPath);
  if (!manifestPath) {
    log.warn(`No manifest in ${pluginPath} (tried manifest.json, *.plugin.json)`);
    return null;
  }

  const manifest = (await readJson(manifestPath)) as PluginManifest | null;

  if (!manifest) {
    log.warn(`Failed to parse manifest at ${manifestPath}`);
    return null;
  }

  if (!manifest.id || !manifest.name || !manifest.version) {
    log.warn(`Invalid manifest in ${pluginPath}: missing required fields`);
    return null;
  }

  const dirName = pluginPath.split("/").pop();
  if (dirName !== manifest.id) {
    log.warn(
      `Plugin directory name '${dirName}' doesn't match manifest id '${manifest.id}'`,
    );
  }

  const entryPath = findEntry(pluginPath);
  if (!entryPath) {
    log.info(`Plugin ${manifest.id} has no code entry — manifest-only plugin`);
    return { manifest };
  }

  const mod = await import(entryPath);
  const definition: PluginDefinition = {
    manifest,
    tools: mod.tools ?? mod.default?.tools,
    hooks: mod.hooks ?? mod.default?.hooks,
  };

  return definition;
}

function findManifest(pluginPath: string): string | null {
  const direct = join(pluginPath, "manifest.json");
  if (existsSync(direct)) return direct;

  // Check for *.plugin.json (e.g. aletheia.plugin.json)
  try {
    const files = readdirSync(pluginPath);
    const pluginJson = files.find((f) => f.endsWith(".plugin.json"));
    if (pluginJson) return join(pluginPath, pluginJson);
  } catch { /* directory may not exist */ }

  return null;
}

function findEntry(pluginPath: string): string | null {
  const candidates = [
    "index.js",
    "index.mjs",
    "dist/index.js",
    "dist/index.mjs",
  ];

  for (const candidate of candidates) {
    const fullPath = join(pluginPath, candidate);
    if (existsSync(fullPath)) return fullPath;
  }

  return null;
}

/**
 * Validate that a plugin path resolves within the allowed root directory.
 * Prevents path traversal and symlink escapes. Only applied to auto-discovered
 * plugins — explicitly configured paths (config.plugins.load.paths) bypass this.
 */
export function validatePluginPath(pluginPath: string, pluginRoot: string): boolean {
  try {
    const resolved = realpathSync(resolve(pluginPath));
    const normalizedRoot = pluginRoot.endsWith(sep) ? pluginRoot : pluginRoot + sep;
    return resolved === pluginRoot || resolved.startsWith(normalizedRoot);
  } catch {
    return false;
  }
}

/**
 * Auto-discover plugins by scanning a root directory for subdirectories
 * containing valid plugin manifests. Applies path safety validation.
 */
export async function discoverPlugins(rootDir: string): Promise<PluginDefinition[]> {
  if (!existsSync(rootDir)) {
    log.debug(`Plugin root not found: ${rootDir}`);
    return [];
  }

  const plugins: PluginDefinition[] = [];
  let entries: import("node:fs").Dirent[];

  try {
    entries = readdirSync(rootDir, { withFileTypes: true });
  } catch (err) {
    log.warn(`Cannot read plugin root ${rootDir}: ${err instanceof Error ? err.message : err}`);
    return [];
  }

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    if (entry.name.startsWith("_") || entry.name.startsWith(".")) continue;

    const pluginPath = join(rootDir, entry.name);

    if (!validatePluginPath(pluginPath, rootDir)) {
      log.warn(`Plugin path validation failed for ${entry.name}, skipping`);
      continue;
    }

    try {
      const plugin = await loadPlugin(pluginPath);
      if (plugin) {
        plugins.push(plugin);
        log.info(`Discovered plugin: ${plugin.manifest.id} v${plugin.manifest.version}`);
      }
    } catch (err) {
      log.warn(`Failed to load discovered plugin ${entry.name}: ${err instanceof Error ? err.message : err}`);
    }
  }

  return plugins;
}
