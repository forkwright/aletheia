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
    if (!trimmed.startsWith("!")) return null;
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

export function createDefaultRegistry(): CommandRegistry {
  const registry = new CommandRegistry();

  registry.register({
    name: "ping",
    description: "Check if the system is alive",
    async execute() {
      return "pong";
    },
  });

  registry.register({
    name: "help",
    aliases: ["commands"],
    description: "List available commands",
    async execute(_args, ctx) {
      const cmds = ctx.manager
        ? (registry as CommandRegistry).listAll()
        : [];
      const lines = ["Available commands:", ""];
      for (const cmd of cmds) {
        lines.push(`  !${cmd.name} — ${cmd.description}`);
      }
      return lines.join("\n");
    },
  });

  registry.register({
    name: "status",
    description: "System status overview",
    async execute(_args, ctx) {
      const metrics = ctx.store.getMetrics();
      const uptime = process.uptime();
      const hours = Math.floor(uptime / 3600);
      const mins = Math.floor((uptime % 3600) / 60);

      const lines: string[] = ["Aletheia Status", ""];
      lines.push(`Uptime: ${hours}h ${mins}m`);
      lines.push("");

      if (ctx.watchdog) {
        const svcStatus = ctx.watchdog.getStatus();
        const allHealthy = svcStatus.every((s) => s.healthy);
        lines.push(`Services: ${allHealthy ? "all healthy" : "DEGRADED"}`);
        for (const svc of svcStatus) {
          lines.push(`  ${svc.healthy ? "+" : "X"} ${svc.name}`);
        }
        lines.push("");
      }

      lines.push("Nous:");
      for (const a of ctx.config.agents.list) {
        const m = metrics.perNous[a.id];
        const u = metrics.usageByNous[a.id];
        const name = a.name ?? a.id;
        const sessions = m?.activeSessions ?? 0;
        const msgs = m?.totalMessages ?? 0;
        const lastSeen = m?.lastActivity ? timeSince(new Date(m.lastActivity)) : "never";
        const tokens = u ? `${Math.round(u.inputTokens / 1000)}k in` : "0k in";
        lines.push(`  ${name}: ${sessions}s, ${msgs} msgs, ${tokens}, last ${lastSeen}`);
      }
      lines.push("");

      const cacheHitRate =
        metrics.usage.totalInputTokens > 0
          ? Math.round((metrics.usage.totalCacheReadTokens / metrics.usage.totalInputTokens) * 100)
          : 0;
      lines.push(
        `Tokens: ${Math.round(metrics.usage.totalInputTokens / 1000)}k in, ${Math.round(metrics.usage.totalOutputTokens / 1000)}k out`,
      );
      lines.push(`Cache: ${cacheHitRate}% hit rate`);
      lines.push(`Turns: ${metrics.usage.turnCount}`);

      return lines.join("\n");
    },
  });

  registry.register({
    name: "sessions",
    description: "List active sessions for this sender",
    async execute(_args, ctx) {
      const sessionKey = `signal:${ctx.isGroup ? ctx.target.groupId : ctx.sender}`;
      const sessions = ctx.store.findSessionsByKey(sessionKey);
      if (!sessions || sessions.length === 0) {
        return "No active sessions.";
      }
      const lines = ["Active sessions:", ""];
      for (const s of sessions.slice(0, 10)) {
        const age = timeSince(new Date(s.updatedAt));
        lines.push(`  ${s.nousId} (${s.messageCount} msgs, ${age})`);
      }
      return lines.join("\n");
    },
  });

  registry.register({
    name: "reset",
    description: "Archive current session and start fresh",
    async execute(_args, ctx) {
      const sessionKey = `signal:${ctx.isGroup ? ctx.target.groupId : ctx.sender}`;
      const sessions = ctx.store.findSessionsByKey(sessionKey);
      if (!sessions || sessions.length === 0) {
        return "No active session to reset.";
      }
      let count = 0;
      for (const s of sessions) {
        ctx.store.archiveSession(s.id);
        count += s.messageCount;
      }
      return `${sessions.length} session(s) archived (${count} messages). Next message starts fresh.`;
    },
  });

  registry.register({
    name: "agent",
    description: "Show which agent handles this conversation",
    async execute(args, ctx) {
      if (!args) {
        const sessionKey = `signal:${ctx.isGroup ? ctx.target.groupId : ctx.sender}`;
        const sessions = ctx.store.findSessionsByKey(sessionKey);
        const nousId = sessions?.[0]?.nousId ?? "default";
        const nous = ctx.config.agents.list.find((a) => a.id === nousId);
        return `Current agent: ${nous?.name ?? nousId}`;
      }
      return `Agent routing is managed via config bindings. Current route would need a config change.`;
    },
  });

  registry.register({
    name: "skills",
    description: "List available skills",
    async execute(_args, ctx) {
      if (!ctx.skills || ctx.skills.size === 0) {
        return "No skills loaded.";
      }
      const lines = ["Available skills:", ""];
      for (const skill of ctx.skills.listAll()) {
        lines.push(`  ${skill.name} — ${skill.description}`);
      }
      return lines.join("\n");
    },
  });

  // --- Pairing / Contact Management ---

  registry.register({
    name: "approve",
    description: "Approve a pending contact request by code",
    adminOnly: true,
    async execute(args, ctx) {
      if (!args.trim()) return "Usage: !approve <code>";
      const result = ctx.store.approveContactByCode(args.trim());
      if (!result) return `No pending request found for code: ${args.trim()}`;
      return `Approved contact: ${result.sender} (${result.channel})`;
    },
  });

  registry.register({
    name: "deny",
    description: "Deny a pending contact request by code",
    adminOnly: true,
    async execute(args, ctx) {
      if (!args.trim()) return "Usage: !deny <code>";
      const denied = ctx.store.denyContactByCode(args.trim());
      if (!denied) return `No pending request found for code: ${args.trim()}`;
      return `Denied contact request for code: ${args.trim()}`;
    },
  });

  registry.register({
    name: "contacts",
    description: "List pending contact requests",
    adminOnly: true,
    async execute(_args, ctx) {
      const pending = ctx.store.getPendingRequests();
      if (pending.length === 0) return "No pending contact requests.";
      const lines = ["Pending contact requests:", ""];
      for (const r of pending) {
        const age = timeSince(new Date(r.createdAt));
        lines.push(`  ${r.senderName} — code: ${r.code} (${age})`);
      }
      return lines.join("\n");
    },
  });

  return registry;
}

function timeSince(date: Date): string {
  const seconds = Math.floor((Date.now() - date.getTime()) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}
