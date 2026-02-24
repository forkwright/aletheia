// Shared types and helpers for route modules
import type { Context } from "hono";
import type { NousManager } from "../../nous/manager.js";
import type { SessionStore } from "../../mneme/store.js";
import type { AletheiaConfig } from "../../taxis/schema.js";
import type { AuthSessionStore } from "../../auth/sessions.js";
import type { AuditLog } from "../../auth/audit.js";
import type { CronScheduler } from "../../daemon/cron.js";
import type { Watchdog } from "../../daemon/watchdog.js";
import type { SkillRegistry } from "../../organon/skills.js";
import type { McpClientManager } from "../../organon/mcp-client.js";
import type { AuthConfig, AuthUser } from "../../auth/middleware.js";
import type { CommandRegistry } from "../../semeion/commands.js";
import type { DianoiaOrchestrator } from "../../dianoia/index.js";

export type { NousManager, SessionStore, AletheiaConfig, AuthSessionStore, AuditLog };
export type { CronScheduler, Watchdog, SkillRegistry, McpClientManager };
export type { AuthConfig, AuthUser, CommandRegistry };

export interface RouteDeps {
  config: AletheiaConfig;
  manager: NousManager;
  store: SessionStore;
  authConfig: AuthConfig;
  authSessionStore: AuthSessionStore | null;
  auditLog: AuditLog | null;
  authRoutes: {
    mode: () => { mode: string };
    login: (username: string, password: string, ip: string, userAgent: string) => Promise<{
      accessToken: string;
      refreshToken: string;
      expiresIn: number;
      username: string;
      role: string;
    } | null>;
    refresh: (token: string) => Promise<{
      accessToken: string;
      refreshToken: string;
      expiresIn: number;
    } | null>;
    logout: (sessionId: string) => void;
  };
  planningOrchestrator?: DianoiaOrchestrator;
}

export interface RouteRefs {
  cron: () => CronScheduler | null;
  watchdog: () => Watchdog | null;
  skills: () => SkillRegistry | null;
  mcp: () => McpClientManager | null;
  commands: () => CommandRegistry | null;
}

export function getUser(c: Context): AuthUser | undefined {
  return (c as unknown as { get(key: string): unknown }).get("user") as AuthUser | undefined;
}
