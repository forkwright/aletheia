// Plugin registry — manage loaded plugins and dispatch hooks
import { createLogger } from "../koina/logger.js";
import type { ToolRegistry } from "../organon/registry.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import type {
  DistillContext,
  DistillResult,
  HookName,
  PluginApi,
  PluginDefinition,
  TurnContext,
  TurnResult,
} from "./types.js";

const log = createLogger("prostheke");

export class PluginRegistry {
  private plugins = new Map<string, PluginDefinition>();
  private api: PluginApi;

  constructor(config: AletheiaConfig) {
    this.api = {
      config,
      log: (level: string, message: string) => {
        if (level === "error") log.error(message);
        else if (level === "warn") log.warn(message);
        else log.info(message);
      },
    };
  }

  register(plugin: PluginDefinition, tools?: ToolRegistry): void {
    const id = plugin.manifest.id;

    if (this.plugins.has(id)) {
      log.warn(`Plugin ${id} already registered, skipping`);
      return;
    }

    this.plugins.set(id, plugin);

    if (plugin.tools && tools) {
      for (const tool of plugin.tools) {
        tools.register(tool);
        log.info(`Plugin ${id} registered tool: ${tool.definition.name}`);
      }
    }

    log.info(`Plugin registered: ${id}`);
  }

  get(id: string): PluginDefinition | undefined {
    return this.plugins.get(id);
  }

  get size(): number {
    return this.plugins.size;
  }

  async dispatchStart(): Promise<void> {
    await this.dispatch("onStart");
  }

  async dispatchShutdown(): Promise<void> {
    await this.dispatch("onShutdown");
  }

  async dispatchBeforeTurn(context: TurnContext): Promise<void> {
    await this.dispatch("onBeforeTurn", context);
  }

  async dispatchAfterTurn(result: TurnResult): Promise<void> {
    await this.dispatch("onAfterTurn", result);
  }

  async dispatchBeforeDistill(context: DistillContext): Promise<void> {
    await this.dispatch("onBeforeDistill", context);
  }

  async dispatchAfterDistill(result: DistillResult): Promise<void> {
    await this.dispatch("onAfterDistill", result);
  }

  async dispatchConfigReload(): Promise<void> {
    await this.dispatch("onConfigReload");
  }

  private async dispatch(hookName: HookName, arg?: unknown): Promise<void> {
    for (const [id, plugin] of this.plugins) {
      const hook = plugin.hooks?.[hookName];
      if (!hook) continue;

      try {
        if (arg !== undefined) {
          await (hook as (api: PluginApi, arg: unknown) => Promise<void>)(
            this.api,
            arg,
          );
        } else {
          await (hook as (api: PluginApi) => Promise<void>)(this.api);
        }
      } catch (err) {
        log.error(
          `Plugin ${id} hook ${hookName} failed: ${err instanceof Error ? err.message : err}`,
        );
      }
    }
  }
}
