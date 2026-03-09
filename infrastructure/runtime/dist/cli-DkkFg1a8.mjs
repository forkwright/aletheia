import { c as paths } from "./entry.mjs";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";

//#region src/agora/cli.ts
function ask(rl, question) {
	return new Promise((resolve) => rl.question(question, resolve));
}
function loadConfigJson(configPath) {
	const file = configPath ?? paths.configFile();
	if (!existsSync(file)) throw new Error(`Config file not found: ${file}`);
	return JSON.parse(readFileSync(file, "utf-8"));
}
function writeConfigJson(config, configPath) {
	writeFileSync(configPath ?? paths.configFile(), JSON.stringify(config, null, 2) + "\n", "utf-8");
}
function channelList(configPath) {
	const channels = loadConfigJson(configPath)["channels"] ?? {};
	console.log("\nConfigured channels:\n");
	const signal = channels["signal"];
	if (signal) {
		const enabled = signal["enabled"] !== false;
		const accounts = signal["accounts"];
		const accountCount = accounts ? Object.keys(accounts).length : 0;
		console.log(`  signal  ${enabled ? "✓ enabled" : "✗ disabled"}  (${accountCount} account${accountCount !== 1 ? "s" : ""})`);
	} else console.log("  signal  ✗ not configured");
	const slack = channels["slack"];
	if (slack) {
		const enabled = slack["enabled"] !== false;
		const mode = slack["mode"] ?? "socket";
		const hasTokens = !!(slack["appToken"] && slack["botToken"]);
		console.log(`  slack   ${enabled ? "✓ enabled" : "✗ disabled"}  (${mode} mode${hasTokens ? ", tokens set" : ", tokens missing"})`);
	} else console.log("  slack   ✗ not configured");
	console.log();
}
function channelRemove(channelId, configPath) {
	if (channelId === "signal") {
		console.error("Cannot remove Signal — it is the default channel. Disable it in config instead.");
		process.exit(1);
	}
	const config = loadConfigJson(configPath);
	const channels = config["channels"] ?? {};
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
async function channelAddSlack(configPath) {
	const rl = createInterface({
		input: process.stdin,
		output: process.stdout
	});
	try {
		const config = loadConfigJson(configPath);
		const channels = config["channels"] ?? {};
		if (channels["slack"]) {
			if (channels["slack"]["enabled"]) {
				if ((await ask(rl, "\nSlack is already configured. Overwrite? (y/N): ")).trim().toLowerCase() !== "y") {
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
		const dmPolicy = [
			"open",
			"allowlist",
			"disabled"
		].includes(dmPolicyInput) ? dmPolicyInput : "open";
		const groupPolicyInput = (await ask(rl, "  ? Channel policy (open/allowlist/disabled) [allowlist]: ")).trim().toLowerCase() || "allowlist";
		channels["slack"] = {
			enabled: true,
			mode: "socket",
			appToken,
			botToken,
			dmPolicy,
			groupPolicy: [
				"open",
				"allowlist",
				"disabled"
			].includes(groupPolicyInput) ? groupPolicyInput : "allowlist",
			allowedChannels: [],
			allowedUsers: [],
			requireMention: (await ask(rl, "  ? Require @mention in channels? (Y/n): ")).trim().toLowerCase() !== "n",
			identity: { useAgentIdentity: true }
		};
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
const SUPPORTED_CHANNELS = ["slack"];
function isSupportedChannel(id) {
	return SUPPORTED_CHANNELS.includes(id);
}
function listSupportedChannels() {
	return [...SUPPORTED_CHANNELS];
}

//#endregion
export { channelAddSlack, channelList, channelRemove, isSupportedChannel, listSupportedChannels };
//# sourceMappingURL=cli-DkkFg1a8.mjs.map