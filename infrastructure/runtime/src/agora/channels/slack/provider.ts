// SlackChannelProvider — Slack integration via Socket Mode (Spec 34, Phase 3)
//
// Implements ChannelProvider for Slack. On start():
//   - Creates @slack/bolt App in Socket Mode
//   - Authenticates via auth.test() to get botUserId
//   - Registers inbound event handlers (messages, mentions)
//   - Sets up inbound debouncing and dedup
//
// On send():
//   - Routes through the Slack sender (markdown→mrkdwn, chunking, identity)
//
// On stop():
//   - Disconnects the Socket Mode WebSocket
//
// Reference: OpenClaw src/slack/monitor/provider.ts

import { createLogger } from "../../../koina/logger.js";
import type {
  ChannelCapabilities,
  ChannelContext,
  ChannelProbeResult,
  ChannelProvider,
  ChannelSendParams,
  ChannelSendResult,
} from "../../types.js";
import type { AletheiaConfig } from "../../../taxis/schema.js";
import { createSlackApp, type SlackAppHandle } from "./client.js";
import { registerSlackListeners, type InboundDebouncer } from "./listener.js";
import { sendSlackMessage } from "./sender.js";

const log = createLogger("agora:slack");

export class SlackChannelProvider implements ChannelProvider {
  readonly id = "slack";
  readonly name = "Slack";
  readonly capabilities: ChannelCapabilities = {
    threads: true,
    reactions: true,
    typing: false,  // Slack bots can't send typing indicators
    media: true,
    streaming: true,  // Phase 5: native text streaming via ChatStreamer
    richFormatting: true,  // Block Kit (future)
    maxTextLength: 4000,
  };

  private readonly config: AletheiaConfig;
  private handle: SlackAppHandle | null = null;
  private debouncer: InboundDebouncer | null = null;
  private started = false;

  constructor(config: AletheiaConfig) {
    this.config = config;
  }

  async start(ctx: ChannelContext): Promise<void> {
    const slackConfig = this.config.channels.slack;
    if (!slackConfig?.enabled) {
      log.info("Slack channel disabled in config");
      return;
    }

    if (!slackConfig.appToken || !slackConfig.botToken) {
      throw new Error(
        "Slack requires both appToken (xapp-...) and botToken (xoxb-...). " +
        "Run `aletheia channel add slack` to configure.",
      );
    }

    // Create and connect the Slack app
    this.handle = await createSlackApp({
      appToken: slackConfig.appToken,
      botToken: slackConfig.botToken,
      mode: slackConfig.mode ?? "socket",
    });

    // Register inbound event handlers (Phase 3 core + Phase 5 streaming/reactions)
    this.debouncer = registerSlackListeners({
      app: this.handle.app as never,
      webClient: this.handle.webClient,
      botUserId: this.handle.botUserId,
      teamId: this.handle.teamId,
      ctx,
      requireMention: slackConfig.requireMention ?? true,
      dmPolicy: slackConfig.dmPolicy ?? "open",
      allowedUsers: slackConfig.allowedUsers ?? [],
      allowedChannels: slackConfig.allowedChannels ?? [],
      groupPolicy: slackConfig.groupPolicy ?? "allowlist",
      streaming: slackConfig.streaming ?? true,
      reactions: {
        enabled: slackConfig.reactions?.enabled ?? true,
        processingEmoji: slackConfig.reactions?.processingEmoji ?? "hourglass_flowing_sand",
      },
    });

    // Start the Socket Mode connection
    await this.handle.app.start();
    this.started = true;

    log.info(`Slack provider started: bot=${this.handle.botUserId} team=${this.handle.teamId}`);
  }

  async send(params: ChannelSendParams): Promise<ChannelSendResult> {
    if (!this.handle) {
      return { sent: false, error: "Slack provider not started" };
    }

    return sendSlackMessage(
      { webClient: this.handle.webClient },
      params,
    );
  }

  async stop(): Promise<void> {
    // Flush any pending debounced messages
    this.debouncer?.flushAll();
    this.debouncer = null;

    if (this.handle && this.started) {
      try {
        await this.handle.app.stop();
      } catch (err) {
        log.warn(`Slack app stop error: ${err instanceof Error ? err.message : err}`);
      }
    }
    this.handle = null;
    this.started = false;
    log.info("Slack provider stopped");
  }

  async probe(): Promise<ChannelProbeResult> {
    if (!this.handle) {
      return { ok: false, error: "Slack provider not started" };
    }

    try {
      const start = Date.now();
      const result = await this.handle.webClient.auth.test();
      const latencyMs = Date.now() - start;

      return {
        ok: Boolean(result.ok),
        latencyMs,
        details: {
          botUserId: this.handle.botUserId,
          teamId: this.handle.teamId,
        },
      };
    } catch (err) {
      return {
        ok: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  }

  /** Get the bot user ID (for external use) */
  get botUserId(): string | undefined {
    return this.handle?.botUserId;
  }

  /** Check if the provider is connected */
  get isConnected(): boolean {
    return this.started && this.handle !== null;
  }
}
