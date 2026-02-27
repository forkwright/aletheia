// Agora CLI — channel management commands (Spec 34)
//
// `aletheia channel add slack` — interactive onboarding wizard
// `aletheia channel list`      — show configured channels and status
// `aletheia channel remove`    — remove a channel config

import { paths } from "../taxis/paths.js";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { createInterface } from "node:readline";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function ask(rl: ReturnType<typeof createInterface>, question: string): Promise<string> {
  return new Promise((resolve) => rl.question(question, resolve));
}

function loadConfigJson(configPath?: string): Record<string, unknown> {
  const file = configPath ?? paths.configFile();
  if (!existsSync(file)) {
    throw new Error(`Config file not found: ${file}`);
  }
  return JSON.parse(readFileSync(file, "utf-8")) as Record<string, unknown>;
}

function writeConfigJson(config: Record<string, unknown>, configPath?: string): void {
  const file = configPath ?? paths.configFile();
  writeFileSync(file, JSON.stringify(config, null, 2) + "\n", "utf-8");
}

// ---------------------------------------------------------------------------
// Channel: list
// ---------------------------------------------------------------------------

export function channelList(configPath?: string): void {
  const config = loadConfigJson(configPath);
  const channels = (config["channels"] ?? {}) as Record<string, unknown>;

  console.log("\nConfigured channels:\n");

  // Signal
  const signal = channels["signal"] as Record<string, unknown> | undefined;
  if (signal) {
    const enabled = signal["enabled"] !== false;
    const accounts = signal["accounts"] as Record<string, unknown> | undefined;
    const accountCount = accounts ? Object.keys(accounts).length : 0;
    console.log(`  signal  ${enabled ? "✓ enabled" : "✗ disabled"}  (${accountCount} account${accountCount !== 1 ? "s" : ""})`);
  } else {
    console.log("  signal  ✗ not configured");
  }

  // Slack
  const slack = channels["slack"] as Record<string, unknown> | undefined;
  if (slack) {
    const enabled = slack["enabled"] !== false;
    const mode = (slack["mode"] as string) ?? "socket";
    const hasTokens = !!(slack["appToken"] && slack["botToken"]);
    console.log(`  slack   ${enabled ? "✓ enabled" : "✗ disabled"}  (${mode} mode${hasTokens ? ", tokens set" : ", tokens missing"})`);
  } else {
    console.log("  slack   ✗ not configured");
  }

  console.log();
}

// ---------------------------------------------------------------------------
// Channel: remove
// ---------------------------------------------------------------------------

export function channelRemove(channelId: string, configPath?: string): void {
  if (channelId === "signal") {
    console.error("Cannot remove Signal — it is the default channel. Disable it in config instead.");
    process.exit(1);
  }

  const config = loadConfigJson(configPath);
  const channels = (config["channels"] ?? {}) as Record<string, unknown>;

  if (!channels[channelId]) {
    console.error(`Channel "${channelId}" is not configured.`);
    process.exit(1);
  }

  delete channels[channelId];
  config["channels"] = channels;
  writeConfigJson(config, configPath);

  console.log(`\n✓ Channel "${channelId}" removed from config.`);
  console.log("  Restart Aletheia to apply: systemctl restart aletheia\n");
}

// ---------------------------------------------------------------------------
// Channel: add slack
// ---------------------------------------------------------------------------

export async function channelAddSlack(configPath?: string): Promise<void> {
  const rl = createInterface({ input: process.stdin, output: process.stdout });

  try {
    const config = loadConfigJson(configPath);
    const channels = (config["channels"] ?? {}) as Record<string, unknown>;

    if (channels["slack"]) {
      const existing = channels["slack"] as Record<string, unknown>;
      if (existing["enabled"]) {
        const overwrite = (await ask(rl, "\nSlack is already configured. Overwrite? (y/N): ")).trim().toLowerCase();
        if (overwrite !== "y") {
          console.log("Aborted.");
          return;
        }
      }
    }

    console.log(`
  Slack Integration Setup
  ${"─".repeat(25)}

  Step 1: Create a Slack App

    Visit https://api.slack.com/apps and click "Create New App"
    Choose "From scratch" and select your workspace
`);

    console.log(`  Step 2: Enable Socket Mode

    In your app settings, go to "Socket Mode" and enable it
    Create an App-Level Token with 'connections:write' scope
    Copy the token (starts with xapp-)
`);

    const appToken = (await ask(rl, "  ? App Token (xapp-...): ")).trim();
    if (!appToken.startsWith("xapp-")) {
      console.error("\n  ✗ App token must start with 'xapp-'. Aborting.\n");
      return;
    }

    console.log(`
  Step 3: Bot Token

    Go to "OAuth & Permissions"
    Add these Bot Token Scopes:
      • channels:history    • channels:read
      • chat:write          • groups:history
      • groups:read         • im:history
      • im:read             • reactions:read
      • reactions:write     • users:read
      • chat:write.customize (optional — agent identity)

    Install the app to your workspace
    Copy the Bot User OAuth Token (starts with xoxb-)
`);

    const botToken = (await ask(rl, "  ? Bot Token (xoxb-...): ")).trim();
    if (!botToken.startsWith("xoxb-")) {
      console.error("\n  ✗ Bot token must start with 'xoxb-'. Aborting.\n");
      return;
    }

    console.log(`
  Step 4: Subscribe to Events

    Go to "Event Subscriptions" → "Subscribe to bot events"
    Add these events:
      • app_mention         • message.channels
      • message.groups      • message.im
      • reaction_added
`);

    console.log("  Step 5: Configure access\n");

    const dmPolicyInput = (await ask(rl, "  ? DM policy (open/allowlist/disabled) [open]: ")).trim().toLowerCase() || "open";
    const dmPolicy = ["open", "allowlist", "disabled"].includes(dmPolicyInput) ? dmPolicyInput : "open";

    const groupPolicyInput = (await ask(rl, "  ? Channel policy (open/allowlist/disabled) [allowlist]: ")).trim().toLowerCase() || "allowlist";
    const groupPolicy = ["open", "allowlist", "disabled"].includes(groupPolicyInput) ? groupPolicyInput : "allowlist";

    const requireMentionInput = (await ask(rl, "  ? Require @mention in channels? (Y/n): ")).trim().toLowerCase();
    const requireMention = requireMentionInput !== "n";

    // Write config
    const slackConfig: Record<string, unknown> = {
      enabled: true,
      mode: "socket",
      appToken,
      botToken,
      dmPolicy,
      groupPolicy,
      allowedChannels: [],
      allowedUsers: [],
      requireMention,
      identity: {
        useAgentIdentity: true,
      },
    };

    channels["slack"] = slackConfig;
    config["channels"] = channels;
    writeConfigJson(config, configPath);

    console.log(`
  ✓ Slack configuration written to config.
  ✓ Restart Aletheia to activate: systemctl restart aletheia

  To bind an agent to a Slack channel:
    Edit bindings in config to add:
    {
      "agentId": "syn",
      "match": { "channel": "slack", "peer": { "kind": "channel", "id": "C0123456789" } }
    }
`);
  } finally {
    rl.close();
  }
}

// ---------------------------------------------------------------------------
// Supported channels registry (for validation + help text)
// ---------------------------------------------------------------------------

const SUPPORTED_CHANNELS = ["slack"] as const;
type SupportedChannel = (typeof SUPPORTED_CHANNELS)[number];

export function isSupportedChannel(id: string): id is SupportedChannel {
  return (SUPPORTED_CHANNELS as readonly string[]).includes(id);
}

export function listSupportedChannels(): string[] {
  return [...SUPPORTED_CHANNELS];
}
