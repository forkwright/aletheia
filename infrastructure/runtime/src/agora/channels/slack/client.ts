// Slack client factory — @slack/bolt App + WebClient (Spec 34, Phase 3)
//
// Reference: OpenClaw src/slack/client.ts, src/slack/monitor/provider.ts
// Uses Socket Mode by default (no public URL required).

import SlackBolt from "@slack/bolt";
import { WebClient, type RetryOptions } from "@slack/web-api";
import { createLogger } from "../../../koina/logger.js";

const log = createLogger("agora:slack:client");

// Bun allows named imports from CJS; Node ESM doesn't. Use default+fallback.
// (Pattern copied from OpenClaw for compatibility)
const slackBoltModule = SlackBolt as typeof import("@slack/bolt") & {
  default?: typeof import("@slack/bolt");
};
const slackBolt =
  (slackBoltModule.App ? slackBoltModule : slackBoltModule.default) ?? slackBoltModule;

export const { App } = slackBolt;

export const SLACK_RETRY_OPTIONS: RetryOptions = {
  retries: 2,
  factor: 2,
  minTimeout: 500,
  maxTimeout: 3000,
  randomize: true,
};

/**
 * Create a Slack WebClient with sensible retry defaults.
 */
export function createWebClient(token: string): WebClient {
  return new WebClient(token, {
    retryConfig: SLACK_RETRY_OPTIONS,
  });
}

export interface SlackAppConfig {
  appToken: string;
  botToken: string;
  signingSecret?: string;
  mode: "socket" | "http";
}

export interface SlackAppHandle {
  app: InstanceType<typeof SlackBolt.App>;
  webClient: WebClient;
  botUserId: string;
  teamId: string;
}

/**
 * Create and connect a Slack Bolt App in Socket Mode.
 *
 * Calls auth.test() on connect to resolve botUserId and teamId.
 * These are needed for self-message filtering and mention stripping.
 */
export async function createSlackApp(config: SlackAppConfig): Promise<SlackAppHandle> {
  if (config.mode !== "socket") {
    throw new Error("Only Socket Mode is supported in v1. Set channels.slack.mode to 'socket'.");
  }

  const app = new App({
    token: config.botToken,
    appToken: config.appToken,
    socketMode: true,
    // Disable built-in logging — we use our own
    logLevel: slackBolt.LogLevel?.ERROR ?? ("error" as never),
  });

  const webClient = createWebClient(config.botToken);

  // Resolve bot identity via auth.test()
  const authResult = await webClient.auth.test();
  const botUserId = authResult.user_id;
  const teamId = authResult.team_id;

  if (!botUserId) {
    throw new Error("Slack auth.test() returned no user_id — check bot token");
  }

  log.info(`Slack authenticated: bot=${botUserId} team=${teamId}`);

  return { app, webClient, botUserId, teamId: teamId ?? "unknown" };
}
