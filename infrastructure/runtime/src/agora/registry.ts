// Agora Registry — manages channel providers (Spec 34)
//
// The registry is the central coordination point for all channels.
// It handles lifecycle (start/stop), send routing, and health probes.

import { createLogger } from "../koina/logger.js";
import type {
  ChannelContext,
  ChannelProbeResult,
  ChannelProvider,
  ChannelSendParams,
  ChannelSendResult,
} from "./types.js";

const log = createLogger("agora");

export class AgoraRegistry {
  private providers = new Map<string, ChannelProvider>();
  private started = false;

  /** Register a channel provider. Must be called before startAll(). */
  register(provider: ChannelProvider): void {
    if (this.providers.has(provider.id)) {
      throw new Error(`Channel provider "${provider.id}" already registered`);
    }
    this.providers.set(provider.id, provider);
    log.info(`Registered channel: ${provider.id} (${provider.name})`);
  }

  /** Get a provider by channel ID */
  get(channelId: string): ChannelProvider | undefined {
    return this.providers.get(channelId);
  }

  /** Check if a channel is registered */
  has(channelId: string): boolean {
    return this.providers.has(channelId);
  }

  /** List all registered provider IDs */
  list(): string[] {
    return [...this.providers.keys()];
  }

  /** Number of registered channels */
  get size(): number {
    return this.providers.size;
  }

  /**
   * Start all registered channels.
   * Each channel receives the shared context and begins listening for inbound messages.
   * A channel that fails to start logs a warning but doesn't block other channels.
   */
  async startAll(ctx: ChannelContext): Promise<void> {
    if (this.started) {
      log.warn("AgoraRegistry.startAll() called more than once — ignoring");
      return;
    }
    this.started = true;

    const results = await Promise.allSettled(
      [...this.providers.values()].map(async (provider) => {
        try {
          await provider.start(ctx);
          log.info(`Channel started: ${provider.id}`);
        } catch (error) {
          log.error(
            `Channel ${provider.id} failed to start: ${error instanceof Error ? error.message : error}`,
          );
          throw error;
        }
      }),
    );

    const succeeded = results.filter((r) => r.status === "fulfilled").length;
    const failed = results.filter((r) => r.status === "rejected").length;

    if (failed > 0) {
      log.warn(`Agora: ${succeeded} channels started, ${failed} failed`);
    } else {
      log.info(`Agora: ${succeeded} channel(s) started`);
    }
  }

  /** Stop all channels gracefully */
  async stopAll(): Promise<void> {
    const results = await Promise.allSettled(
      [...this.providers.values()].map(async (provider) => {
        try {
          await provider.stop();
          log.debug(`Channel stopped: ${provider.id}`);
        } catch (error) {
          log.warn(
            `Channel ${provider.id} failed to stop cleanly: ${error instanceof Error ? error.message : error}`,
          );
        }
      }),
    );
    this.started = false;
    log.info(`Agora: ${results.length} channel(s) stopped`);
  }

  /**
   * Send a message through a specific channel.
   * Returns error result if channel is not found.
   */
  async send(channelId: string, params: ChannelSendParams): Promise<ChannelSendResult> {
    const provider = this.providers.get(channelId);
    if (!provider) {
      return { sent: false, error: `Channel "${channelId}" not registered` };
    }
    return provider.send(params);
  }

  /** Probe all channels for health status */
  async probeAll(): Promise<Map<string, ChannelProbeResult>> {
    const results = new Map<string, ChannelProbeResult>();
    for (const [id, provider] of this.providers) {
      if (provider.probe) {
        try {
          results.set(id, await provider.probe());
        } catch (error) {
          results.set(id, { ok: false, error: error instanceof Error ? error.message : String(error) });
        }
      } else {
        // No probe = assume OK if registered
        results.set(id, { ok: true });
      }
    }
    return results;
  }

  /**
   * Get the first registered provider that matches a predicate.
   * Useful for legacy code that needs "any available Signal client."
   */
  getFirst(predicate?: (p: ChannelProvider) => boolean): ChannelProvider | undefined {
    for (const provider of this.providers.values()) {
      if (!predicate || predicate(provider)) return provider;
    }
    return undefined;
  }
}
