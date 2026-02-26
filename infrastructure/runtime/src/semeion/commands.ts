// Signal command registry — !status, !help, !ping, etc.
import { createLogger } from "../koina/logger.js";
import type { SignalClient } from "./client.js";
import type { SendTarget } from "./sender.js";
import type { SessionStore } from "../mneme/store.js";
import type { NousManager } from "../nous/manager.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import type { Watchdog } from "../daemon/watchdog.js";
import type { SkillRegistry } from "../organon/skills.js";

const log = createLogger("semeion:cmd");

export interface CommandContext {
  sender: string;
  senderName: string;
  isGroup: boolean;
  accountId: string;
  target: SendTarget;
  client: SignalClient;
  store: SessionStore;
  config: AletheiaConfig;
  manager: NousManager;
  watchdog: Watchdog | null;
  skills: SkillRegistry | null;
  sessionId?: string | undefined;
}

export interface CommandHandler {
  name: string;
  aliases?: string[];
  description: string;
  adminOnly?: boolean;
  execute: (args: string, ctx: CommandContext) => Promise<string>;
}

export class CommandRegistry {
  private commands = new Map<string, CommandHandler>();

  register(handler: CommandHandler): void {
    this.commands.set(handler.name, handler);
    for (const alias of handler.aliases ?? []) {
      this.commands.set(alias, handler);
    }
    log.debug(`Registered command: !${handler.name}`);
  }

  match(text: string): { handler: CommandHandler; args: string } | null {
    const trimmed = text.trim();
    if (!trimmed.startsWith("!") && !trimmed.startsWith("/")) return null;
    const spaceIdx = trimmed.indexOf(" ", 1);
    const cmd = spaceIdx === -1 ? trimmed.slice(1) : trimmed.slice(1, spaceIdx);
    const args = spaceIdx === -1 ? "" : trimmed.slice(spaceIdx + 1).trim();
    const handler = this.commands.get(cmd.toLowerCase());
    return handler ? { handler, args } : null;
  }

  listAll(): CommandHandler[] {
    const seen = new Set<string>();
    const result: CommandHandler[] = [];
    for (const handler of this.commands.values()) {
      if (seen.has(handler.name)) continue;
      seen.add(handler.name);
      result.push(handler);
    }
    return result;
  }
}

function findSession(ctx: CommandContext, nousId?: string): ReturnType<SessionStore["findSessionById"]> {
  if (ctx.sessionId) return ctx.store.findSessionById(ctx.sessionId);
  const sessionKey = `signal:${ctx.isGroup ? ctx.target.groupId : ctx.sender}`;
  return ctx.store.findSession(nousId ?? ctx.config.agents.list[0]?.id ?? "syn", sessionKey);
}

function findSessionsByCtx(ctx: CommandContext): ReturnType<SessionStore["findSessionsByKey"]> {
  if (ctx.sessionId) {
    const s = ctx.store.findSessionById(ctx.sessionId);
    return s ? [s] : [];
  }
  const sessionKey = `signal:${ctx.isGroup ? ctx.target.groupId : ctx.sender}`;
  return ctx.store.findSessionsByKey(sessionKey);
}

export function createDefaultRegistry(): CommandRegistry {
  const registry = new CommandRegistry();

  registry.register({
    name: "ping",
    description: "Check if the system is alive",
    execute(): Promise<string> {
      const uptime = process.uptime();
      const hours = Math.floor(uptime / 3600);
      const mins = Math.floor((uptime % 3600) / 60);
      return Promise.resolve(`**pong** \u2014 \`${hours}h ${mins}m\` uptime`);
    },
  });

  registry.register({
    name: "help",
    aliases: ["commands"],
    description: "List available commands",
    execute(_args, ctx): Promise<string> {
      const cmds = ctx.manager
        ? (registry as CommandRegistry).listAll()
        : [];
      const prefix = ctx.target.groupId ? "!" : "/";
      const lines = [`**Commands** (use \`${prefix}\` prefix)\n`];
      lines.push("| Command | Description |");
      lines.push("|---------|-------------|");
      for (const cmd of cmds) {
        lines.push(`| \`${prefix}${cmd.name}\` | ${cmd.description} |`);
      }
      return Promise.resolve(lines.join("\n"));
    },
  });

  registry.register({
    name: "status",
    description: "System status overview",
    execute(_args, ctx): Promise<string> {
      const metrics = ctx.store.getMetrics();
      const uptime = process.uptime();
      const hours = Math.floor(uptime / 3600);
      const mins = Math.floor((uptime % 3600) / 60);
      const cacheHitRate =
        metrics.usage.totalInputTokens > 0
          ? Math.round((metrics.usage.totalCacheReadTokens / metrics.usage.totalInputTokens) * 100)
          : 0;

      const lines: string[] = ["## Aletheia Status\n"];
      lines.push(
        `**Uptime:** \`${hours}h ${mins}m\` \u00b7 ` +
        `**Turns:** ${metrics.usage.turnCount.toLocaleString()} \u00b7 ` +
        `**Tokens:** ${formatK(metrics.usage.totalInputTokens)} in / ${formatK(metrics.usage.totalOutputTokens)} out \u00b7 ` +
        `**Cache:** ${cacheHitRate}%\n`,
      );

      if (ctx.watchdog) {
        const svcStatus = ctx.watchdog.getStatus();
        const allHealthy = svcStatus.every((s) => s.healthy);
        lines.push(`### Services${allHealthy ? "" : " \u2014 DEGRADED"}\n`);
        lines.push("| Service | Status |");
        lines.push("|---------|--------|");
        for (const svc of svcStatus) {
          lines.push(`| ${svc.name} | ${svc.healthy ? "\u2713 healthy" : "\u2717 down"} |`);
        }
        lines.push("");
      }

      lines.push("### Nous\n");
      lines.push("| Agent | Sessions | Messages | Tokens In | Last Active |");
      lines.push("|-------|----------|----------|-----------|-------------|");
      for (const a of ctx.config.agents.list) {
        const m = metrics.perNous[a.id];
        const u = metrics.usageByNous[a.id];
        const name = a.name ?? a.id;
        const sessions = m?.activeSessions ?? 0;
        const msgs = m?.totalMessages ?? 0;
        const lastSeen = m?.lastActivity ? timeSince(new Date(m.lastActivity)) : "never";
        const tokens = u ? formatK(u.inputTokens) : "0";
        lines.push(`| ${name} | ${sessions} | ${msgs} | ${tokens} | ${lastSeen} |`);
      }

      return Promise.resolve(lines.join("\n"));
    },
  });

  registry.register({
    name: "sessions",
    description: "List active sessions for this sender",
    execute(_args, ctx): Promise<string> {
      const sessions = findSessionsByCtx(ctx);
      if (!sessions || sessions.length === 0) {
        return Promise.resolve("No active sessions.");
      }
      const lines = [`**Active sessions** (${sessions.length})\n`];
      lines.push("| Agent | Messages | Last Active |");
      lines.push("|-------|----------|-------------|");
      for (const s of sessions.slice(0, 10)) {
        const age = timeSince(new Date(s.updatedAt));
        lines.push(`| ${s.nousId} | ${s.messageCount} | ${age} |`);
      }
      return Promise.resolve(lines.join("\n"));
    },
  });

  registry.register({
    name: "reset",
    description: "Archive current session and start fresh",
    execute(_args, ctx): Promise<string> {
      const sessions = findSessionsByCtx(ctx);
      if (!sessions || sessions.length === 0) {
        return Promise.resolve("No active session to reset.");
      }
      let count = 0;
      for (const s of sessions) {
        ctx.store.archiveSession(s.id);
        count += s.messageCount;
      }
      return Promise.resolve(`**Reset:** ${sessions.length} session(s) archived (${count} messages). Next message starts fresh.`);
    },
  });

  registry.register({
    name: "agent",
    description: "Show which agent handles this conversation",
    execute(args, ctx): Promise<string> {
      if (!args) {
        const sessions = findSessionsByCtx(ctx);
        const nousId = sessions?.[0]?.nousId ?? "default";
        const nous = ctx.config.agents.list.find((a) => a.id === nousId);
        return Promise.resolve(`**Agent:** \`${nous?.name ?? nousId}\``);
      }
      return Promise.resolve("Agent routing is managed via config bindings. Current route would need a config change.");
    },
  });

  registry.register({
    name: "skills",
    description: "List available skills",
    execute(_args, ctx): Promise<string> {
      if (!ctx.skills || ctx.skills.size === 0) {
        return Promise.resolve("No skills loaded.");
      }
      const all = ctx.skills.listAll();
      const lines = [`**Available skills** (${all.length})\n`];
      lines.push("| Skill | Description |");
      lines.push("|-------|-------------|");
      for (const skill of all) {
        lines.push(`| \`${skill.name}\` | ${skill.description} |`);
      }
      return Promise.resolve(lines.join("\n"));
    },
  });

  registry.register({
    name: "model",
    description: "Show or switch model \u2014 /model [name]",
    execute(args, ctx): Promise<string> {
      const session = findSession(ctx);
      if (!args.trim()) {
        const current = session?.model ?? ctx.config.agents.defaults.model.primary;
        return Promise.resolve(`**Model:** \`${current}\``);
      }
      const modelName = args.trim().toLowerCase();
      const aliases: Record<string, string> = {
        opus: "claude-opus-4-6",
        sonnet: "claude-sonnet-4-6",
        haiku: "claude-haiku-4-5-20251001",
      };
      const resolved = aliases[modelName] ?? modelName;
      if (session) {
        ctx.store.updateSessionModel(session.id, resolved);
        return Promise.resolve(`**Model switched to:** \`${resolved}\``);
      }
      return Promise.resolve("No active session. Model will be set when you send your next message.");
    },
  });

  registry.register({
    name: "think",
    description: "Toggle extended thinking \u2014 /think [on|off|budget]",
    execute(args, ctx): Promise<string> {
      const session = findSession(ctx);
      if (!session) return Promise.resolve("No active session.");
      const cfg = ctx.store.getThinkingConfig(session.id);
      const arg = args.trim().toLowerCase();
      if (!arg) {
        const newState = !cfg.enabled;
        ctx.store.setThinkingConfig(session.id, newState, cfg.budget);
        return Promise.resolve(`**Extended thinking:** ${newState ? "ON" : "OFF"} \u00b7 **Budget:** ${cfg.budget.toLocaleString()} tokens`);
      }
      if (arg === "on") {
        ctx.store.setThinkingConfig(session.id, true, cfg.budget);
        return Promise.resolve(`**Extended thinking:** ON \u00b7 **Budget:** ${cfg.budget.toLocaleString()} tokens`);
      }
      if (arg === "off") {
        ctx.store.setThinkingConfig(session.id, false, cfg.budget);
        return Promise.resolve("**Extended thinking:** OFF");
      }
      const budget = parseInt(arg, 10);
      if (!isNaN(budget) && budget > 0) {
        ctx.store.setThinkingConfig(session.id, true, budget);
        return Promise.resolve(`**Extended thinking:** ON \u00b7 **Budget:** ${budget.toLocaleString()} tokens`);
      }
      return Promise.resolve("**Usage:** `/think [on|off|<budget>]`");
    },
  });

  registry.register({
    name: "distill",
    aliases: [],
    description: "Distill context \u2014 compress older messages into long-term memory",
    async execute(_args, ctx) {
      const session = findSession(ctx);
      if (!session) return "No active session to distill.";
      try {
        await ctx.manager.triggerDistillation(session.id);
        return "**Context distilled.** Older messages compressed into memory.";
      } catch (error) {
        return `**Distillation failed:** ${error instanceof Error ? error.message : String(error)}`;
      }
    },
  });

  registry.register({
    name: "blackboard",
    description: "Cross-agent blackboard \u2014 /blackboard [list|read|write|delete] [key] [value]",
    execute(args, ctx): Promise<string> {
      const parts = args.trim().split(/\s+/);
      const sub = parts[0]?.toLowerCase();
      if (!sub || sub === "list") {
        const entries = ctx.store.blackboardList();
        if (entries.length === 0) return Promise.resolve("Blackboard is empty.");
        const lines = ["**Blackboard**\n"];
        lines.push("| Key | Entries | Authors |");
        lines.push("|-----|---------|---------|");
        for (const e of entries) {
          lines.push(`| \`${e.key}\` | ${e.count} | ${e.authors.join(", ")} |`);
        }
        return Promise.resolve(lines.join("\n"));
      }
      if (sub === "read") {
        const key = parts[1];
        if (!key) return Promise.resolve("**Usage:** `/blackboard read <key>`");
        const entries = ctx.store.blackboardRead(key);
        if (entries.length === 0) return Promise.resolve(`No entries for key: \`${key}\``);
        const lines = [`**Blackboard:** \`${key}\`\n`];
        for (const e of entries) {
          lines.push(`> **${e.author}:** ${e.value}`);
        }
        return Promise.resolve(lines.join("\n"));
      }
      if (sub === "write") {
        const key = parts[1];
        const value = parts.slice(2).join(" ");
        if (!key || !value) return Promise.resolve("**Usage:** `/blackboard write <key> <value>`");
        const session = findSession(ctx);
        const author = session?.nousId ?? "user";
        ctx.store.blackboardWrite(key, value, author);
        return Promise.resolve(`**Written to blackboard:** \`${key}\` = ${value}`);
      }
      if (sub === "delete") {
        const key = parts[1];
        if (!key) return Promise.resolve("**Usage:** `/blackboard delete <key>`");
        const session = findSession(ctx);
        const author = session?.nousId ?? "user";
        const count = ctx.store.blackboardDelete(key, author);
        return Promise.resolve(count > 0 ? `**Deleted** ${count} entries for key: \`${key}\`` : `No entries found for key: \`${key}\``);
      }
      return Promise.resolve("**Usage:** `/blackboard [list|read|write|delete] [key] [value]`");
    },
  });

  // --- Pairing / Contact Management ---

  registry.register({
    name: "approve",
    description: "Approve a pending contact request by code",
    adminOnly: true,
    execute(args, ctx): Promise<string> {
      if (!args.trim()) return Promise.resolve("**Usage:** `!approve <code>`");
      const result = ctx.store.approveContactByCode(args.trim());
      if (!result) return Promise.resolve(`No pending request found for code: \`${args.trim()}\``);
      return Promise.resolve(`**Approved contact:** ${result.sender} (${result.channel})`);
    },
  });

  registry.register({
    name: "deny",
    description: "Deny a pending contact request by code",
    adminOnly: true,
    execute(args, ctx): Promise<string> {
      if (!args.trim()) return Promise.resolve("**Usage:** `!deny <code>`");
      const denied = ctx.store.denyContactByCode(args.trim());
      if (!denied) return Promise.resolve(`No pending request found for code: \`${args.trim()}\``);
      return Promise.resolve(`**Denied** contact request for code: \`${args.trim()}\``);
    },
  });

  registry.register({
    name: "contacts",
    description: "List pending contact requests",
    adminOnly: true,
    execute(_args, ctx): Promise<string> {
      const pending = ctx.store.getPendingRequests();
      if (pending.length === 0) return Promise.resolve("No pending contact requests.");
      const lines = [`**Pending contact requests** (${pending.length})\n`];
      lines.push("| Name | Code | Requested |");
      lines.push("|------|------|-----------|");
      for (const r of pending) {
        const age = timeSince(new Date(r.createdAt));
        lines.push(`| ${r.senderName} | \`${r.code}\` | ${age} |`);
      }
      return Promise.resolve(lines.join("\n"));
    },
  });

  return registry;
}

function formatK(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
  return String(n);
}

function timeSince(date: Date): string {
  const seconds = Math.floor((Date.now() - date.getTime()) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}
