// Hono HTTP gateway
import { Hono } from "hono";
import { serve } from "@hono/node-server";
import { createLogger } from "../koina/logger.js";
import type { NousManager } from "../nous/manager.js";
import type { SessionStore } from "../mneme/store.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import { type AuthConfig, createAuthMiddleware, createAuthRoutes } from "../symbolon/middleware.js";
import type { AuthSessionStore } from "../symbolon/sessions.js";
import type { AuditLog } from "../symbolon/audit.js";
import type { CronScheduler } from "../daemon/cron.js";
import type { Watchdog } from "../daemon/watchdog.js";
import type { SkillRegistry } from "../organon/skills.js";
import type { McpClientManager } from "../organon/mcp-client.js";
import type { CommandRegistry } from "../semeion/commands.js";
import type { RouteDeps, RouteRefs } from "./routes/deps.js";

import { authRoutes } from "./routes/auth.js";
import { auditRoutes } from "./routes/audit.js";
import { systemRoutes } from "./routes/system.js";
import { sessionRoutes } from "./routes/sessions.js";
import { eventRoutes } from "./routes/events.js";
import { agentRoutes } from "./routes/agents.js";
import { turnRoutes } from "./routes/turns.js";
import { commandRoutes } from "./routes/commands.js";
import { planRoutes } from "./routes/plans.js";
import { threadRoutes } from "./routes/threads.js";
import { contactRoutes } from "./routes/contacts.js";
import { skillRoutes } from "./routes/skills.js";
import { mcpRoutes } from "./routes/mcp.js";
import { costRoutes } from "./routes/costs.js";
import { metricRoutes } from "./routes/metrics.js";
import { memoryRoutes } from "./routes/memory.js";
import { exportRoutes } from "./routes/export.js";
import { blackboardRoutes } from "./routes/blackboard.js";
import { workspaceRoutes } from "./routes/workspace.js";
import { setupRoutes } from "./routes/setup.js";
import { planningRoutes } from "../dianoia/routes.js";

const log = createLogger("pylon");

// Set after gateway creation — avoids circular dependency
let cronRef: CronScheduler | null = null;
let watchdogRef: Watchdog | null = null;
let skillsRef: SkillRegistry | null = null;
let mcpRef: McpClientManager | null = null;
let commandsRef: CommandRegistry | null = null;
export function setCronRef(cron: CronScheduler): void {
  cronRef = cron;
}
export function setWatchdogRef(wd: Watchdog): void {
  watchdogRef = wd;
}
export function setSkillsRef(sr: SkillRegistry): void {
  skillsRef = sr;
}
export function setMcpRef(mcp: McpClientManager): void {
  mcpRef = mcp;
}
export function setCommandsRef(reg: CommandRegistry): void {
  commandsRef = reg;
}

export interface GatewayAuthDeps {
  sessionStore: AuthSessionStore | null;
  auditLog: AuditLog | null;
  secret: string | null;
}

export function createGateway(
  config: AletheiaConfig,
  manager: NousManager,
  store: SessionStore,
  authDeps?: GatewayAuthDeps,
): Hono {
  const app = new Hono();

  // Security headers
  app.use("*", async (c, next) => {
    c.header("X-Content-Type-Options", "nosniff");
    c.header("X-Frame-Options", "DENY");
    c.header("Referrer-Policy", "no-referrer");
    c.header("X-XSS-Protection", "0");
    return next();
  });

  // CORS
  const allowedOrigins = config.gateway.cors?.allowOrigins ?? [];
  if (allowedOrigins.length > 0) {
    app.use("*", async (c, next) => {
      const origin = c.req.header("Origin");
      if (origin && allowedOrigins.includes(origin)) {
        c.header("Access-Control-Allow-Origin", origin);
        c.header("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        c.header("Access-Control-Allow-Headers", "Content-Type, Authorization");
        c.header("Access-Control-Max-Age", "3600");
      }
      if (c.req.method === "OPTIONS") return c.body(null, 204);
      return next();
    });
  }

  // Rate limiting — sliding window per IP
  const rateLimit = config.gateway.rateLimit?.requestsPerMinute ?? 60;
  const rateBuckets = new Map<string, { count: number; resetAt: number }>();

  app.use("/mcp/*", async (c, next) => {
    const ip = c.req.header("X-Forwarded-For")?.split(",")[0]?.trim()
      ?? c.req.header("X-Real-IP")
      ?? "unknown";
    const now = Date.now();
    const bucket = rateBuckets.get(ip);

    if (bucket && bucket.resetAt > now) {
      if (bucket.count >= rateLimit) {
        c.header("Retry-After", String(Math.ceil((bucket.resetAt - now) / 1000)));
        return c.json({ error: "Rate limit exceeded" }, 429);
      }
      bucket.count++;
    } else {
      rateBuckets.set(ip, { count: 1, resetAt: now + 60_000 });
    }

    return next();
  });

  // Periodic cleanup of expired rate limit buckets
  setInterval(() => {
    const now = Date.now();
    for (const [ip, bucket] of rateBuckets) {
      if (bucket.resetAt <= now) rateBuckets.delete(ip);
    }
  }, 60_000);

  // Auth middleware — multi-mode (none, token, password, session)
  const authConfig: AuthConfig = {
    mode: config.gateway.auth.mode as AuthConfig["mode"],
    ...(config.gateway.auth.token ? { token: config.gateway.auth.token } : {}),
    users: config.gateway.auth.users,
    ...(authDeps?.secret ? {
      session: {
        secret: authDeps.secret,
        accessTokenTtl: config.gateway.auth.session.accessTokenTtl,
        refreshTokenTtl: config.gateway.auth.session.refreshTokenTtl,
        maxSessions: config.gateway.auth.session.maxSessionsPerUser,
        secureCookies: config.gateway.auth.session.secureCookies,
      },
    } : {}),
  };

  const authSessionStore = authDeps?.sessionStore ?? null;
  const auditLog = authDeps?.auditLog ?? null;

  app.use("*", createAuthMiddleware(authConfig, authSessionStore, auditLog));

  const authRouteFns = createAuthRoutes(authConfig, authSessionStore);

  // Global error handler — log details server-side, return generic message to client
  app.onError((err, c) => {
    log.error(`Unhandled error on ${c.req.method} ${c.req.path}: ${err.message}`);
    return c.json({ error: "Internal server error" }, 500);
  });

  // Build shared dependencies and refs for route modules
  const planningOrchestrator = manager.getPlanningOrchestrator();
  const deps: RouteDeps = {
    config,
    manager,
    store,
    authConfig,
    authSessionStore,
    auditLog,
    authRoutes: authRouteFns,
    ...(planningOrchestrator ? { planningOrchestrator } : {}),
  };

  const refs: RouteRefs = {
    cron: () => cronRef,
    watchdog: () => watchdogRef,
    skills: () => skillsRef,
    mcp: () => mcpRef,
    commands: () => commandsRef,
  };

  // Mount all route modules
  const modules = [
    setupRoutes,
    systemRoutes,
    authRoutes,
    auditRoutes,
    sessionRoutes,
    eventRoutes,
    agentRoutes,
    turnRoutes,
    commandRoutes,
    planRoutes,
    threadRoutes,
    contactRoutes,
    skillRoutes,
    mcpRoutes,
    costRoutes,
    metricRoutes,
    memoryRoutes,
    exportRoutes,
    blackboardRoutes,
    workspaceRoutes,
    planningRoutes,
  ];

  for (const factory of modules) {
    app.route("", factory(deps, refs));
  }

  return app;
}

export function startGateway(
  app: Hono,
  port: number,
): ReturnType<typeof serve> {
  log.info(`Starting gateway on port ${port}`);
  return serve({
    fetch: app.fetch,
    port,
  });
}
