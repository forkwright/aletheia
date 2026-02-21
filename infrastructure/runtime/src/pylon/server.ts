// Hono HTTP gateway
import { Hono } from "hono";
import { serve } from "@hono/node-server";
import { createCipheriv, randomBytes, scryptSync } from "node:crypto";
import { createLogger, withTurnAsync } from "../koina/logger.js";
import type { NousManager } from "../nous/manager.js";
import type { SessionStore } from "../mneme/store.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import { tryReloadConfig } from "../taxis/loader.js";
import { type AuthConfig, type AuthUser, createAuthMiddleware, createAuthRoutes } from "../auth/middleware.js";
import type { AuthSessionStore } from "../auth/sessions.js";
import type { AuditLog } from "../auth/audit.js";
import type { CronScheduler } from "../daemon/cron.js";
import type { Watchdog } from "../daemon/watchdog.js";
import type { SkillRegistry } from "../organon/skills.js";
import type { McpClientManager } from "../organon/mcp-client.js";
import { calculateCostBreakdown } from "../hermeneus/pricing.js";
import { computeSelfAssessment } from "../distillation/reflect.js";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { eventBus, type EventName } from "../koina/event-bus.js";
import { join, resolve } from "node:path";
import { execSync } from "node:child_process";
import { getVersion } from "../version.js";

const log = createLogger("pylon");

function getUser(c: import("hono").Context): AuthUser | undefined {
  return (c as unknown as { get(key: string): unknown }).get("user") as AuthUser | undefined;
}

// Set after gateway creation — avoids circular dependency
let cronRef: CronScheduler | null = null;
let watchdogRef: Watchdog | null = null;
let skillsRef: SkillRegistry | null = null;
let mcpRef: McpClientManager | null = null;
let commandsRef: import("../semeion/commands.js").CommandRegistry | null = null;
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
export function setCommandsRef(reg: import("../semeion/commands.js").CommandRegistry): void {
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

  const authRoutes = createAuthRoutes(authConfig, authSessionStore);

  // Global error handler — log details server-side, return generic message to client
  app.onError((err, c) => {
    log.error(`Unhandled error on ${c.req.method} ${c.req.path}: ${err.message}`);
    return c.json({ error: "Internal server error" }, 500);
  });

  app.get("/health", (c) =>
    c.json({ status: "ok", version: getVersion(), timestamp: new Date().toISOString() }),
  );

  // --- Auth Routes ---

  function getRefreshCookie(c: import("hono").Context): string | undefined {
    const header = c.req.header("Cookie") ?? "";
    const match = header.match(/(?:^|;\s*)aletheia_refresh=([^;]*)/);
    return match?.[1];
  }

  function setRefreshCookie(
    c: import("hono").Context,
    token: string,
    maxAge?: number,
  ): void {
    const secure = authConfig.session?.secureCookies ?? true;
    const parts = [
      `aletheia_refresh=${token}`,
      "HttpOnly",
      "SameSite=Strict",
      "Path=/api/auth",
    ];
    if (secure) parts.push("Secure");
    if (maxAge !== undefined) parts.push(`Max-Age=${maxAge}`);
    c.header("Set-Cookie", parts.join("; "));
  }

  function clearRefreshCookie(c: import("hono").Context): void {
    const secure = authConfig.session?.secureCookies ?? true;
    const parts = [
      "aletheia_refresh=",
      "HttpOnly",
      "SameSite=Strict",
      "Path=/api/auth",
      "Max-Age=0",
    ];
    if (secure) parts.push("Secure");
    c.header("Set-Cookie", parts.join("; "));
  }

  app.get("/api/auth/mode", (c) => {
    return c.json(authRoutes.mode());
  });

  app.post("/api/auth/login", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = (await c.req.json()) as Record<string, unknown>;
    } catch {
      return c.json({ error: "Invalid JSON" }, 400);
    }

    const username = body["username"] as string;
    const password = body["password"] as string;
    const rememberMe = body["rememberMe"] === true;

    if (!username || !password) {
      return c.json({ error: "username and password required" }, 400);
    }

    const ip =
      c.req.header("X-Forwarded-For")?.split(",")[0]?.trim() ??
      c.req.header("X-Real-IP") ??
      "unknown";
    const userAgent = c.req.header("User-Agent") ?? "";

    const result = await authRoutes.login(username, password, ip, userAgent);
    if (!result) {
      return c.json({ error: "Invalid credentials" }, 401);
    }

    const maxAge = rememberMe
      ? authConfig.session?.refreshTokenTtl
      : undefined;
    setRefreshCookie(c, result.refreshToken, maxAge);

    return c.json({
      accessToken: result.accessToken,
      expiresIn: result.expiresIn,
      username: result.username,
      role: result.role,
    });
  });

  app.post("/api/auth/refresh", async (c) => {
    const refreshToken = getRefreshCookie(c);
    if (!refreshToken) {
      return c.json({ error: "No refresh token" }, 401);
    }

    const result = await authRoutes.refresh(refreshToken);
    if (!result) {
      clearRefreshCookie(c);
      return c.json({ error: "Invalid or expired refresh token" }, 401);
    }

    setRefreshCookie(c, result.refreshToken, authConfig.session?.refreshTokenTtl);

    return c.json({
      accessToken: result.accessToken,
      expiresIn: result.expiresIn,
    });
  });

  app.post("/api/auth/logout", (c) => {
    const user = getUser(c);
    if (user?.sessionId) {
      authRoutes.logout(user.sessionId);
    }
    clearRefreshCookie(c);
    return c.json({ ok: true });
  });

  app.get("/api/auth/sessions", (c) => {
    const user = getUser(c);
    if (!user || !authSessionStore) return c.json({ sessions: [] });
    const sessions = authSessionStore.listForUser(user.username);
    return c.json({
      sessions: sessions.map((s) => ({
        id: s.id,
        createdAt: s.createdAt,
        lastUsedAt: s.lastUsedAt,
        expiresAt: s.expiresAt,
        ipAddress: s.ipAddress,
        userAgent: s.userAgent,
        current: s.id === user.sessionId,
      })),
    });
  });

  app.post("/api/auth/revoke/:id", (c) => {
    if (!authSessionStore) return c.json({ error: "Session auth not enabled" }, 400);
    const id = c.req.param("id");
    const revoked = authSessionStore.revoke(id);
    if (!revoked) return c.json({ error: "Session not found" }, 404);
    return c.json({ ok: true });
  });

  // --- Audit Log ---

  app.get("/api/audit", (c) => {
    if (!auditLog) return c.json({ entries: [] });
    const actor = c.req.query("actor");
    const since = c.req.query("since");
    const limit = Math.min(parseInt(c.req.query("limit") ?? "100", 10), 500);
    const entries = auditLog.query({
      ...(actor ? { actor } : {}),
      ...(since ? { since } : {}),
      limit,
    });
    return c.json({ entries });
  });

  app.get("/api/status", (c) =>
    c.json({
      status: "ok",
      version: getVersion(),
      updateChannel: config.updates?.channel ?? "stable",
      agents: config.agents.list.map((a) => a.id),
      timestamp: new Date().toISOString(),
    }),
  );

  app.get("/api/system/update-channel", (c) =>
    c.json({ channel: config.updates?.channel ?? "stable" }),
  );

  app.post("/api/config/reload", (c) => {
    const newConfig = tryReloadConfig();
    if (!newConfig) {
      return c.json({ ok: false, error: "Config validation failed — check logs" }, 400);
    }
    const diff = manager.reloadConfig(newConfig);

    // Rebuild routing cache with new bindings
    const bindings = newConfig.bindings.map((b) => {
      const entry: { channel: string; peerKind?: string; peerId?: string; accountId?: string; nousId: string } = {
        channel: b.match.channel,
        nousId: b.agentId,
      };
      if (b.match.peer?.kind) entry.peerKind = b.match.peer.kind;
      if (b.match.peer?.id) entry.peerId = b.match.peer.id;
      if (b.match.accountId) entry.accountId = b.match.accountId;
      return entry;
    });
    store.rebuildRoutingCache(bindings);

    eventBus.emit("config:reloaded", { added: diff.added, removed: diff.removed });
    return c.json({ ok: true, added: diff.added, removed: diff.removed, bindings: bindings.length });
  });

  app.get("/api/sessions", (c) => {
    const nousId = c.req.query("nousId");
    const sessions = store.listSessions(nousId);
    // Include canonical session key per agent for webchat convergence
    const canonical: Record<string, string> = {};
    if (nousId) {
      const key = store.getCanonicalSessionKey(nousId);
      if (key) canonical[nousId] = key;
    } else {
      // Multi-agent view: resolve for each unique agent
      const seen = new Set<string>();
      for (const s of sessions) {
        if (!seen.has(s.nousId)) {
          seen.add(s.nousId);
          const key = store.getCanonicalSessionKey(s.nousId);
          if (key) canonical[s.nousId] = key;
        }
      }
    }
    return c.json({ sessions, canonical });
  });

  app.get("/api/sessions/:id/history", (c) => {
    const id = c.req.param("id");
    const limit = parseInt(c.req.query("limit") ?? "100", 10);
    const includeDistilled = c.req.query("includeDistilled") === "true";
    const history = store.getHistory(id, { limit, excludeDistilled: !includeDistilled });
    return c.json({ messages: history });
  });

  // Global SSE event stream — bridges eventBus to clients for real-time updates
  app.get("/api/events", (c) => {
    const encoder = new TextEncoder();
    let closed = false;

    const stream = new ReadableStream({
      start(controller) {
        const activeTurns = manager.getActiveTurnsByNous();
        controller.enqueue(encoder.encode(`event: init\ndata: ${JSON.stringify({ activeTurns })}\n\n`));

        const forward = (eventName: string) => (data: unknown) => {
          if (closed) return;
          try {
            controller.enqueue(encoder.encode(`event: ${eventName}\ndata: ${JSON.stringify(data)}\n\n`));
          } catch { closed = true; }
        };

        const handlers: Array<[EventName, (data: unknown) => void]> = [
          ["turn:before", forward("turn:before")],
          ["turn:after", forward("turn:after")],
          ["tool:called", forward("tool:called")],
          ["tool:failed", forward("tool:failed")],
          ["session:created", forward("session:created")],
          ["session:archived", forward("session:archived")],
          ["distill:before", forward("distill:before")],
          ["distill:stage", forward("distill:stage")],
          ["distill:after", forward("distill:after")],
        ];

        for (const [event, handler] of handlers) {
          eventBus.on(event, handler);
        }

        const pingInterval = setInterval(() => {
          if (closed) return;
          try { controller.enqueue(encoder.encode(`: ping\n\n`)); }
          catch { closed = true; }
        }, 15_000);

        c.req.raw.signal.addEventListener("abort", () => {
          closed = true;
          clearInterval(pingInterval);
          for (const [event, handler] of handlers) {
            eventBus.off(event, handler);
          }
          try { controller.close(); } catch { /* already closed */ }
        });
      },
    });

    return new Response(stream, {
      headers: {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        "Connection": "keep-alive",
        "X-Accel-Buffering": "no",
      },
    });
  });

  app.post("/api/sessions/send", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "Invalid JSON body" }, 400);
    }

    const { agentId, message, sessionKey, media } = body as {
      agentId: string;
      message: string;
      sessionKey?: string;
      media?: Array<{ contentType: string; data: string; filename?: string }>;
    };

    if (!agentId || !message) {
      return c.json({ error: "agentId and message required" }, 400);
    }

    // Session convergence: same logic as streaming endpoint
    let resolvedKey = sessionKey ?? "main";
    if (resolvedKey.startsWith("signal:")) {
      const signalPeerId = resolvedKey.slice("signal:".length);
      const routedOwner = store.resolveRoute("signal", "group", signalPeerId)
        ?? store.resolveRoute("signal", "dm", signalPeerId);
      if (routedOwner && routedOwner !== agentId) {
        resolvedKey = `web:${Date.now()}`;
        log.warn(`API session key ownership mismatch: signal key belongs to ${routedOwner}, not ${agentId}`);
      }
    } else if (!resolvedKey.startsWith("agent:") && !resolvedKey.startsWith("cron:") && !resolvedKey.startsWith("spawn:")) {
      const canonical = store.getCanonicalSessionKey(agentId);
      if (canonical) {
        if (canonical !== resolvedKey) {
          log.info(`API session converged: "${resolvedKey}" → "${canonical}" for ${agentId}`);
        }
        resolvedKey = canonical;
      }
    }

    try {
      const result = await withTurnAsync(
        { channel: "api", nousId: agentId, sessionKey: resolvedKey },
        () => manager.handleMessage({
          text: message,
          nousId: agentId,
          sessionKey: resolvedKey,
          ...(media?.length ? { media } : {}),
        }),
      );
      return c.json({
        response: result.text,
        sessionId: result.sessionId,
        toolCalls: result.toolCalls,
        ...(result.error ? { error: result.error } : {}),
        usage: {
          inputTokens: result.inputTokens,
          outputTokens: result.outputTokens,
          cacheReadTokens: result.cacheReadTokens,
          cacheWriteTokens: result.cacheWriteTokens,
        },
      });
    } catch (error) {
      const msg =
        error instanceof Error ? error.message : String(error);
      log.error(`Session send failed: ${msg}`);
      return c.json({ error: "Internal error processing message" }, 500);
    }
  });

  app.post("/api/sessions/stream", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "Invalid JSON body" }, 400);
    }

    const { agentId, message, sessionKey, media } = body as {
      agentId: string;
      message: string;
      sessionKey?: string;
      media?: Array<{ contentType: string; data: string; filename?: string }>;
    };

    if (!agentId || !message) {
      return c.json({ error: "agentId and message required" }, 400);
    }

    if (typeof manager.handleMessageStreaming !== "function") {
      return c.json({ error: "Streaming not implemented" }, 501);
    }

    // Validate media attachments from webchat
    const validMedia: Array<{ contentType: string; data: string; filename?: string }> = [];
    if (media?.length) {
      log.info(`Stream request has ${media.length} media attachment(s)`);
      const maxBytes = 25 * 1024 * 1024; // 25MB per attachment
      for (const item of media) {
        if (!item.contentType || !item.data) continue;
        const estimatedSize = Math.ceil(item.data.length * 0.75);
        if (estimatedSize > maxBytes) {
          log.warn(`Skipping oversized webchat attachment (${Math.round(estimatedSize / 1024)}KB)`);
          continue;
        }
        if (!/^(image|audio|application|text)\//i.test(item.contentType)) {
          log.warn(`Skipping unsupported media type: ${item.contentType}`);
          continue;
        }
        validMedia.push(item);
      }
    }

    if (validMedia.length > 0) {
      log.info(`Passing ${validMedia.length} valid media to manager (types: ${validMedia.map(m => m.contentType).join(", ")})`);
    }

    const encoder = new TextEncoder();
    const rawSessionKey = sessionKey ?? "main";

    // Session convergence: when webchat connects with a generic key (main, web:*, etc.),
    // resolve to the canonical DM session so webchat and Signal share the same conversation.
    // This ensures continuity across devices and transports.
    let resolvedSessionKey = rawSessionKey;

    if (rawSessionKey.startsWith("signal:")) {
      // Guard: if the webchat sends a signal: session key, verify agent ownership.
      const signalPeerId = rawSessionKey.slice("signal:".length);
      const routedOwner = store.resolveRoute("signal", "group", signalPeerId)
        ?? store.resolveRoute("signal", "dm", signalPeerId);
      if (routedOwner && routedOwner !== agentId) {
        resolvedSessionKey = `web:${agentId}`;
        log.warn(
          `Session key ownership mismatch: "${rawSessionKey}" belongs to ${routedOwner}, ` +
          `not ${agentId}. Reassigned to "${resolvedSessionKey}".`,
        );
      }
    } else if (!rawSessionKey.startsWith("agent:") && !rawSessionKey.startsWith("cron:") && !rawSessionKey.startsWith("spawn:")) {
      // For generic webchat keys (main, web:main, web:1234, etc.),
      // try to converge with the canonical DM session
      const canonical = store.getCanonicalSessionKey(agentId);
      if (canonical) {
        resolvedSessionKey = canonical;
        if (canonical !== rawSessionKey) {
          log.info(
            `Webchat session converged: "${rawSessionKey}" → "${canonical}" for ${agentId}`,
          );
        }
      }
    }

    // Thread resolution: webchat identity is "anonymous" until auth is wired
    let webchatThreadId: string | undefined;
    let webchatBindingId: string | undefined;
    let webchatLockKey: string | undefined;
    try {
      const identity = "anonymous";
      const channelKey = `web:${identity}:${agentId}`;
      const thread = manager.sessionStore.resolveThread(agentId, identity);
      const binding = manager.sessionStore.resolveBinding(thread.id, "webchat", channelKey);
      webchatThreadId = thread.id;
      webchatBindingId = binding.id;
      webchatLockKey = `binding:${binding.id}`;
    } catch (err) {
      log.warn(`Webchat thread resolution failed: ${err instanceof Error ? err.message : err}`);
    }

    // Handle /new topic boundary command
    const newTopicMatch = message.trim().match(/^\/new(?:\s+(.+))?$/i);
    if (newTopicMatch) {
      const topicLabel = newTopicMatch[1]?.trim() ?? "";
      const boundaryContent = topicLabel ? `[TOPIC: ${topicLabel}]` : "[TOPIC]";
      const ackText = topicLabel ? `New topic: ${topicLabel}` : "New topic started.";
      try {
        const session = store.findOrCreateSession(agentId, resolvedSessionKey);
        store.appendMessage(session.id, "user", boundaryContent, { tokenEstimate: 10 });
      } catch (err) {
        log.warn(`Topic boundary insert failed: ${err instanceof Error ? err.message : err}`);
      }
      const enc = new TextEncoder();
      const topicStream = new ReadableStream({
        start(ctrl) {
          const turnId = `webchat:topic:${Date.now()}`;
          ctrl.enqueue(enc.encode(`event: turn_start\ndata: ${JSON.stringify({ type: "turn_start", sessionId: "", nousId: agentId, turnId })}\n\n`));
          ctrl.enqueue(enc.encode(`event: text_delta\ndata: ${JSON.stringify({ type: "text_delta", text: ackText })}\n\n`));
          ctrl.enqueue(enc.encode(`event: turn_complete\ndata: ${JSON.stringify({ type: "turn_complete", outcome: { text: ackText, nousId: agentId, sessionId: "", toolCalls: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 } })}\n\n`));
          ctrl.close();
        },
      });
      return new Response(topicStream, { headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-cache", "X-Accel-Buffering": "no" } });
    }

    const stream = new ReadableStream({
      async start(controller) {
        await withTurnAsync(
          { channel: "webchat", nousId: agentId, sessionKey: resolvedSessionKey },
          async () => {
            try {
              for await (const event of manager.handleMessageStreaming({
                text: message,
                nousId: agentId,
                sessionKey: resolvedSessionKey,
                ...(webchatThreadId ? { threadId: webchatThreadId } : {}),
                ...(webchatBindingId ? { bindingId: webchatBindingId } : {}),
                ...(webchatLockKey ? { lockKey: webchatLockKey } : {}),
                ...(validMedia.length > 0 ? { media: validMedia } : {}),
              })) {
                try {
                  const payload = `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`;
                  controller.enqueue(encoder.encode(payload));
                } catch {
                  // Client disconnected — stop sending but don't abort turn
                  break;
                }
              }
            } catch (err) {
              const msg = err instanceof Error ? err.message : String(err);
              log.error(`Stream error: ${msg}`);
              const payload = `event: error\ndata: ${JSON.stringify({ type: "error", message: "Internal error" })}\n\n`;
              controller.enqueue(encoder.encode(payload));
            } finally {
              controller.close();
            }
          },
        );
      },
    });

    return new Response(stream, {
      headers: {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        "Connection": "keep-alive",
      },
    });
  });

  app.get("/api/agents/:id/identity", (c) => {
    const id = c.req.param("id");
    const agent = config.agents.list.find((a) => a.id === id);
    if (!agent) return c.json({ error: "Agent not found" }, 404);

    try {
      const workspace = agent.workspace;
      const identityPath = join(workspace, "IDENTITY.md");
      const raw = readFileSync(identityPath, "utf-8");
      const emojiMatch = raw.match(/emoji:\s*(.+)/i);
      const nameMatch = raw.match(/name:\s*(.+)/i);
      // Strip markdown bold markers and clean up
      let parsedName = nameMatch?.[1]?.replace(/\*+/g, "").trim() || "";
      if (!parsedName) parsedName = agent.name ?? agent.id;
      let parsedEmoji: string | null = null;
      if (emojiMatch?.[1]) {
        // Extract just emoji characters — strip markdown bold and any trailing text
        const cleaned = emojiMatch[1].replace(/\*+/g, "").trim();
        // Match leading emoji (Unicode emoji sequences) before any ASCII text
        const emojiOnly = cleaned.match(/^(\p{Emoji_Presentation}|\p{Emoji}\uFE0F)+/u);
        parsedEmoji = emojiOnly?.[0] || null;
      }
      return c.json({
        id: agent.id,
        name: parsedName,
        emoji: parsedEmoji,
      });
    } catch (err) {
      log.debug(`IDENTITY.md read failed for ${id}: ${err instanceof Error ? err.message : err}`);
      return c.json({
        id: agent.id,
        name: agent.identity?.name ?? agent.name ?? agent.id,
        emoji: agent.identity?.emoji ?? null,
      });
    }
  });

  app.get("/api/branding", (c) => {
    return c.json(config.branding ?? { name: "Aletheia" });
  });

  app.get("/api/config", (c) => {
    return c.json({
      agents: config.agents.list.map((a) => ({
        id: a.id,
        name: a.name ?? a.id,
        workspace: a.workspace,
      })),
      bindings: config.bindings.length,
      plugins: Object.keys(config.plugins.entries).length,
      branding: config.branding,
    });
  });

  // --- Turn management ---

  app.get("/api/turns/active", (c) => {
    return c.json({ turns: manager.getActiveTurnDetails() });
  });

  app.post("/api/turns/:id/abort", (c) => {
    const id = c.req.param("id");
    const aborted = manager.abortTurn(id);
    if (!aborted) return c.json({ error: "Turn not found or already completed" }, 404);
    return c.json({ ok: true, turnId: id });
  });

  // --- Commands ---

  app.get("/api/commands", (c) => {
    if (!commandsRef) return c.json({ commands: [] });
    const cmds = commandsRef.listAll().map((cmd) => ({
      name: cmd.name,
      description: cmd.description,
      aliases: cmd.aliases ?? [],
    }));
    return c.json({ commands: cmds });
  });

  app.post("/api/command", async (c) => {
    if (!commandsRef) return c.json({ error: "Commands not available" }, 503);
    const body = await c.req.json() as Record<string, unknown>;
    const command = body["command"] as string;
    const sessionId = body["sessionId"] as string | undefined;
    if (!command || typeof command !== "string") {
      return c.json({ error: "Missing command" }, 400);
    }
    const match = commandsRef.match(command);
    if (!match) return c.json({ error: `Unknown command: ${command}` }, 404);
    try {
      const ctx = {
        sender: "webchat",
        senderName: "WebUI",
        isGroup: false,
        accountId: "",
        target: { account: "" } as import("../semeion/sender.js").SendTarget,
        client: {} as import("../semeion/client.js").SignalClient,
        store,
        config,
        manager,
        watchdog: watchdogRef,
        skills: skillsRef,
        sessionId,
      };
      const result = await match.handler.execute(match.args, ctx);
      return c.json({ ok: true, result });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return c.json({ error: msg }, 500);
    }
  });

  // --- Tool Approval ---

  app.post("/api/turns/:turnId/tools/:toolId/approve", async (c) => {
    const turnId = c.req.param("turnId");
    const toolId = c.req.param("toolId");
    let alwaysAllow = false;
    try {
      const body = await c.req.json() as Record<string, unknown>;
      alwaysAllow = body["alwaysAllow"] === true;
    } catch {
      // No body is fine
    }
    const resolved = manager.approvalGate.resolveApproval(turnId, toolId, {
      decision: "approve",
      alwaysAllow,
    });
    if (!resolved) return c.json({ error: "No pending approval for this tool" }, 404);
    return c.json({ ok: true });
  });

  app.post("/api/turns/:turnId/tools/:toolId/deny", (c) => {
    const turnId = c.req.param("turnId");
    const toolId = c.req.param("toolId");
    const resolved = manager.approvalGate.resolveApproval(turnId, toolId, {
      decision: "deny",
    });
    if (!resolved) return c.json({ error: "No pending approval for this tool" }, 404);
    return c.json({ ok: true });
  });

  app.get("/api/approval/mode", (c) => {
    const approval = (config.agents.defaults as Record<string, unknown>)["approval"] as { mode?: string } | undefined;
    return c.json({ mode: approval?.mode ?? "autonomous" });
  });

  // --- Admin API ---

  app.get("/api/agents", (c) => {
    const agents = config.agents.list.map((a) => ({
      id: a.id,
      name: a.name ?? a.id,
      workspace: a.workspace,
      model: a.model ?? config.agents.defaults.model.primary,
    }));
    return c.json({ agents });
  });

  app.get("/api/agents/:id", (c) => {
    const id = c.req.param("id");
    const agent = config.agents.list.find((a) => a.id === id);
    if (!agent) return c.json({ error: "Agent not found" }, 404);

    const sessions = store.listSessions(id).slice(0, 20);
    const metrics = store.getMetrics();
    const usage = metrics.usageByNous[id];

    return c.json({
      id: agent.id,
      name: agent.name ?? agent.id,
      workspace: agent.workspace,
      model: agent.model ?? config.agents.defaults.model.primary,
      sessions,
      usage: usage ?? null,
    });
  });

  app.get("/api/cron", (c) => {
    return c.json({ jobs: cronRef?.getStatus() ?? [] });
  });

  app.post("/api/cron/:id/trigger", async (c) => {
    if (!cronRef) return c.json({ error: "Cron not enabled" }, 400);
    const id = c.req.param("id");
    const jobs = cronRef.getStatus();
    const job = jobs.find((j) => j.id === id);
    if (!job) return c.json({ error: "Job not found" }, 404);

    try {
      await manager.handleMessage({
        text: `[Cron trigger: ${id}] Manual trigger via admin API`,
        sessionKey: `cron:${id}`,
        channel: "cron",
        ...(job.agentId ? { nousId: job.agentId } : {}),
      });
      return c.json({ ok: true, jobId: id });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error(`Cron trigger failed: ${msg}`);
      return c.json({ error: "Failed to trigger cron job" }, 500);
    }
  });

  // --- Tool Stats API ---

  app.get("/api/tool-stats", (c) => {
    const agentId = c.req.query("agentId");
    const window = c.req.query("window");
    let windowHours = 168; // 7 days
    if (window) {
      const match = window.match(/^(\d+)(h|d)$/);
      if (match) {
        windowHours = match[2] === "d" ? parseInt(match[1]!, 10) * 24 : parseInt(match[1]!, 10);
      }
    }
    const stats = store.getToolStats({
      ...(agentId ? { nousId: agentId } : {}),
      windowHours,
    });
    return c.json({ ok: true, window: `${windowHours}h`, stats });
  });

  // --- Reflection API ---

  app.get("/api/reflection/:nousId", (c) => {
    const nousId = c.req.param("nousId");
    const limit = parseInt(c.req.query("limit") ?? "10", 10);
    const logs = store.getReflectionLog(nousId, { limit });
    return c.json({ nousId, reflections: logs });
  });

  app.get("/api/reflection/:nousId/assessment", (c) => {
    const nousId = c.req.param("nousId");
    const assessment = computeSelfAssessment(store, nousId);
    return c.json({ nousId, assessment });
  });

  app.get("/api/reflection/:nousId/latest", (c) => {
    const nousId = c.req.param("nousId");
    const last = store.getLastReflection(nousId);
    if (!last) return c.json({ nousId, reflection: null });
    return c.json({ nousId, reflection: last });
  });

  app.post("/api/sessions/:id/archive", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);
    store.archiveSession(id);
    return c.json({ ok: true, archived: id });
  });

  app.post("/api/sessions/:id/distill", async (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    try {
      await manager.triggerDistillation(id);
      return c.json({ ok: true, sessionId: id });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error(`Distillation trigger failed: ${msg}`);
      return c.json({ error: "Failed to trigger distillation" }, 500);
    }
  });

  // --- Message Queue API ---

  app.post("/api/sessions/:id/queue", async (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    const body = await c.req.json<{ text?: string; sender?: string }>().catch(() => null);
    if (!body?.text?.trim()) return c.json({ error: "text is required" }, 400);

    store.queueMessage(id, body.text.trim(), body.sender);
    return c.json({ ok: true, queued: true, queueLength: store.getQueueLength(id) });
  });

  app.get("/api/sessions/:id/queue", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    return c.json({ sessionId: id, queueLength: store.getQueueLength(id) });
  });

  // --- Plans API ---

  app.get("/api/plans/:id", (c) => {
    const plan = store.getPlan(c.req.param("id"));
    if (!plan) return c.json({ error: "Plan not found" }, 404);
    return c.json(plan);
  });

  app.get("/api/sessions/:id/plan", (c) => {
    const plan = store.getActivePlan(c.req.param("id"));
    if (!plan) return c.json({ error: "No active plan" }, 404);
    return c.json(plan);
  });

  app.post("/api/plans/:id/approve", async (c) => {
    const planId = c.req.param("id");
    const plan = store.getPlan(planId);
    if (!plan) return c.json({ error: "Plan not found" }, 404);
    if (plan.status !== "awaiting_approval") {
      return c.json({ error: `Plan is ${plan.status}, not awaiting_approval` }, 400);
    }

    let body: Record<string, unknown> = {};
    try { body = await c.req.json(); } catch { /* no body is fine — approve all */ }

    // Optional: skip specific steps by index
    const skipSteps = (body["skip"] as number[] | undefined) ?? [];
    const steps = plan.steps.map((step) =>
      skipSteps.includes(step.id) ? { ...step, status: "skipped" as const } : { ...step, status: "approved" as const },
    );

    store.updatePlanSteps(planId, steps);
    store.updatePlanStatus(planId, "executing");

    // Trigger plan execution by sending a message to the agent's session
    const approvedCount = steps.filter(s => s.status === "approved").length;
    const skippedCount = steps.filter(s => s.status === "skipped").length;
    const summary = `Plan approved (${approvedCount} steps${skippedCount ? `, ${skippedCount} skipped` : ""}). Executing now.`;

    // Queue the execution trigger as a user message
    store.queueMessage(plan.sessionId, `[PLAN_APPROVED:${planId}] ${summary}`, "system");

    // Look up session to get the correct session_key for lock routing
    const session = store.findSessionById(plan.sessionId);
    const sessionKey = session?.sessionKey ?? "main";
    const lockKey = `${plan.nousId}:${sessionKey}`;

    if (!manager.isSessionActive(lockKey)) {
      // No active turn — fire a new turn to pick up the plan
      setImmediate(() => {
        const gen = manager.handleMessageStreaming({
          text: `Execute the approved plan ${planId}. The plan steps are already stored — retrieve them and execute each approved step in order.`,
          nousId: plan.nousId,
          sessionKey,
        });
        (async () => { for await (const _ of gen) { /* drain */ } })().catch((err) => { log.warn(`Plan execution drain failed: ${err instanceof Error ? err.message : err}`); });
      });
    }

    return c.json({ ok: true, planId, approved: approvedCount, skipped: skippedCount });
  });

  app.post("/api/plans/:id/cancel", (c) => {
    const planId = c.req.param("id");
    const plan = store.getPlan(planId);
    if (!plan) return c.json({ error: "Plan not found" }, 404);
    if (plan.status !== "awaiting_approval" && plan.status !== "executing") {
      return c.json({ error: `Plan is ${plan.status}, cannot cancel` }, 400);
    }

    store.updatePlanStatus(planId, "cancelled");
    return c.json({ ok: true, planId, status: "cancelled" });
  });

  // --- Thread API ---

  app.get("/api/threads", (c) => {
    const nousId = c.req.query("nousId");
    const threads = store.listThreads(nousId ?? undefined);
    return c.json({ threads });
  });

  app.get("/api/threads/:id/history", (c) => {
    const id = c.req.param("id");
    const before = c.req.query("before");
    const limit = Math.min(parseInt(c.req.query("limit") ?? "50", 10), 200);
    const messages = store.getThreadHistory(id, {
      ...(before ? { before } : {}),
      limit,
    });
    return c.json({
      messages: messages.map((m) => ({
        id: m.id,
        sessionId: m.sessionId,
        seq: m.seq,
        role: m.role,
        content: m.content,
        toolCallId: m.toolCallId,
        toolName: m.toolName,
        createdAt: m.createdAt,
      })),
    });
  });

  app.get("/api/threads/:id/summary", (c) => {
    const id = c.req.param("id");
    const summary = store.getThreadSummary(id);
    if (!summary) return c.json({ error: "No summary for this thread" }, 404);
    return c.json(summary);
  });

  // --- Contact/Pairing API ---

  app.get("/api/contacts/pending", (c) => {
    return c.json({ requests: store.getPendingRequests() });
  });

  app.post("/api/contacts/:code/approve", (c) => {
    const code = c.req.param("code");
    const result = store.approveContactByCode(code);
    if (!result) return c.json({ error: "No pending request for that code" }, 404);
    return c.json({ ok: true, ...result });
  });

  app.post("/api/contacts/:code/deny", (c) => {
    const code = c.req.param("code");
    const denied = store.denyContactByCode(code);
    if (!denied) return c.json({ error: "No pending request for that code" }, 404);
    return c.json({ ok: true });
  });

  // --- Skills API ---

  app.get("/api/skills", (c) => {
    const list = skillsRef?.listAll() ?? [];
    return c.json({
      skills: list.map((s) => ({
        id: s.id,
        name: s.name,
        description: s.description,
      })),
    });
  });

  // --- MCP Servers API ---

  app.get("/api/mcp/servers", (c) => {
    return c.json({ servers: mcpRef?.getStatus() ?? [] });
  });

  app.post("/api/mcp/servers/:name/reconnect", async (c) => {
    if (!mcpRef) return c.json({ error: "MCP not enabled" }, 400);
    const name = c.req.param("name");
    const mcpConfig = (config as Record<string, unknown>)["mcp"] as { servers?: Record<string, unknown> } | undefined;
    const serverConfig = mcpConfig?.servers?.[name];
    if (!serverConfig) return c.json({ error: "Server not found in config" }, 404);
    try {
      await mcpRef.connect(name, serverConfig as import("../organon/mcp-client.js").McpServerConfig);
      return c.json({ ok: true, name });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return c.json({ error: `Reconnect failed: ${msg}` }, 500);
    }
  });

  // --- Cost Attribution API ---

  app.get("/api/costs/summary", (c) => {
    const metrics = store.getMetrics();
    const agentCosts = config.agents.list.map((a) => {
      const usage = metrics.usageByNous[a.id];
      if (!usage) return { agentId: a.id, cost: 0, turns: 0 };
      const cost = calculateCostBreakdown({
        inputTokens: usage.inputTokens,
        outputTokens: usage.outputTokens,
        cacheReadTokens: usage.cacheReadTokens,
        cacheWriteTokens: usage.cacheWriteTokens,
        model: null, // mixed models — approximate with default
      });
      return { agentId: a.id, ...cost, turns: usage.turns };
    });
    const totalCost = agentCosts.reduce((sum, a) => sum + ("totalCost" in a ? a.totalCost : a.cost), 0);
    return c.json({ totalCost: Math.round(totalCost * 10000) / 10000, agents: agentCosts });
  });

  app.get("/api/costs/session/:id", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    const turns = store.getCostsBySession(id);
    const costs = turns.map((t) => ({
      turn: t.turnSeq,
      ...calculateCostBreakdown(t),
      model: t.model,
      timestamp: t.createdAt,
    }));
    const totalCost = costs.reduce((sum, c) => sum + c.totalCost, 0);
    return c.json({
      sessionId: id,
      nousId: session.nousId,
      totalCost: Math.round(totalCost * 10000) / 10000,
      turns: costs,
    });
  });

  app.get("/api/costs/agent/:id", (c) => {
    const id = c.req.param("id");
    const agent = config.agents.list.find((a) => a.id === id);
    if (!agent) return c.json({ error: "Agent not found" }, 404);

    const byModel = store.getCostsByAgent(id);
    const breakdown = byModel.map((m) => ({
      model: m.model,
      ...calculateCostBreakdown(m),
      turns: m.turns,
    }));
    const totalCost = breakdown.reduce((sum, b) => sum + b.totalCost, 0);
    return c.json({
      agentId: id,
      totalCost: Math.round(totalCost * 10000) / 10000,
      byModel: breakdown,
    });
  });

  app.get("/api/costs/daily", (c) => {
    const days = Math.min(Number(c.req.query("days")) || 30, 90);
    const rows = store.getDailyCosts(days);
    const daily = rows.map((r) => {
      const cost = calculateCostBreakdown({
        inputTokens: r.inputTokens,
        outputTokens: r.outputTokens,
        cacheReadTokens: r.cacheReadTokens,
        cacheWriteTokens: r.cacheWriteTokens,
        model: null,
      });
      return {
        date: r.date,
        cost: Math.round(cost.totalCost * 10000) / 10000,
        tokens: r.inputTokens + r.outputTokens + r.cacheReadTokens + r.cacheWriteTokens,
        turns: r.turns,
      };
    });
    return c.json({ daily });
  });

  app.get("/api/metrics", (c) => {
    const metrics = store.getMetrics();
    const uptime = process.uptime();

    const nous = config.agents.list.map((a) => {
      const nousMetrics = metrics.perNous[a.id];
      const nousUsage = metrics.usageByNous[a.id];
      return {
        id: a.id,
        name: a.name ?? a.id,
        activeSessions: nousMetrics?.activeSessions ?? 0,
        totalMessages: nousMetrics?.totalMessages ?? 0,
        lastActivity: nousMetrics?.lastActivity ?? null,
        tokens: nousUsage
          ? {
              input: nousUsage.inputTokens,
              output: nousUsage.outputTokens,
              cacheRead: nousUsage.cacheReadTokens,
              cacheWrite: nousUsage.cacheWriteTokens,
              turns: nousUsage.turns,
            }
          : null,
      };
    });

    const cacheHitRate =
      metrics.usage.totalInputTokens > 0
        ? Math.round(
            (metrics.usage.totalCacheReadTokens /
              metrics.usage.totalInputTokens) *
              100,
          )
        : 0;

    return c.json({
      status: "ok",
      uptime: Math.round(uptime),
      timestamp: new Date().toISOString(),
      nous,
      usage: {
        ...metrics.usage,
        cacheHitRate,
      },
      cron: cronRef?.getStatus() ?? [],
      services: watchdogRef?.getStatus() ?? [],
      mcp: mcpRef?.getStatus() ?? [],
    });
  });

  // --- Memory Sidecar Proxy ---

  const memoryUrl = process.env["MEMORY_SIDECAR_URL"] ?? "http://127.0.0.1:8230";

  function memorySidecarHeaders(extra?: Record<string, string>): Record<string, string> {
    const headers: Record<string, string> = { ...extra };
    const key = process.env["ALETHEIA_MEMORY_KEY"];
    if (key) headers["Authorization"] = `Bearer ${key}`;
    return headers;
  }

  app.get("/api/memory/graph/export", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/export${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph/search", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/search${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph_stats", async (c) => {
    const res = await fetch(`${memoryUrl}/graph_stats`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.post("/api/memory/graph/analyze", async (c) => {
    const body = await c.req.text();
    const res = await fetch(`${memoryUrl}/graph/analyze`, {
      method: "POST",
      headers: memorySidecarHeaders({ "Content-Type": "application/json" }),
      body,
    });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/entity/:name", async (c) => {
    const name = c.req.param("name");
    const res = await fetch(`${memoryUrl}/entity/${encodeURIComponent(name)}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.delete("/api/memory/entity/:name", async (c) => {
    const name = c.req.param("name");
    const res = await fetch(`${memoryUrl}/entity/${encodeURIComponent(name)}`, {
      method: "DELETE",
      headers: memorySidecarHeaders(),
    });
    return c.json(await res.json(), res.status as 200);
  });

  app.patch("/api/memory/entity/:name/flag", async (c) => {
    const name = c.req.param("name");
    const body = await c.req.text();
    const res = await fetch(`${memoryUrl}/entity/${encodeURIComponent(name)}/flag`, {
      method: "PATCH",
      headers: memorySidecarHeaders({ "Content-Type": "application/json" }),
      body,
    });
    return c.json(await res.json(), res.status as 200);
  });

  app.post("/api/memory/entity/merge", async (c) => {
    const body = await c.req.text();
    const res = await fetch(`${memoryUrl}/entity/merge`, {
      method: "POST",
      headers: memorySidecarHeaders({ "Content-Type": "application/json" }),
      body,
    });
    return c.json(await res.json(), res.status as 200);
  });

  // --- Spec 09 Phases 8-13: Graph Intelligence Endpoints ---

  app.get("/api/memory/health", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/memory/health${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph/timeline", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/timeline${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph/agent-overlay", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/agent-overlay${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  app.get("/api/memory/graph/drift", async (c) => {
    const qs = c.req.url.includes("?") ? "?" + c.req.url.split("?")[1] : "";
    const res = await fetch(`${memoryUrl}/graph/drift${qs}`, { headers: memorySidecarHeaders() });
    return c.json(await res.json(), res.status as 200);
  });

  // --- Export / Analytics API ---

  app.get("/api/export/stats", (c) => {
    const nousId = c.req.query("nousId");
    const since = c.req.query("since");
    const stats = store.getExportStats({
      ...(nousId ? { nousId } : {}),
      ...(since ? { since } : {}),
    });
    return c.json(stats);
  });

  app.get("/api/export/sessions", (c) => {
    const nousId = c.req.query("nousId");
    const since = c.req.query("since");
    const until = c.req.query("until");
    const sessions = store.listSessionsFiltered({
      ...(nousId ? { nousId } : {}),
      ...(since ? { since } : {}),
      ...(until ? { until } : {}),
    });
    return c.json({ sessions });
  });

  app.get("/api/export/sessions/:id", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    const messages = store.getHistory(id, { excludeDistilled: false });
    const usage = store.getUsageForSession(id);

    // Stream as JSONL
    const lines: string[] = [];
    lines.push(JSON.stringify({ type: "session", ...session }));
    for (const m of messages) {
      lines.push(JSON.stringify({ type: "message", seq: m.seq, role: m.role, content: m.content, isDistilled: m.isDistilled, toolName: m.toolName ?? null, tokenEstimate: m.tokenEstimate ?? null, createdAt: m.createdAt }));
    }
    for (const u of usage) {
      lines.push(JSON.stringify({ type: "usage", turnSeq: u.turnSeq, inputTokens: u.inputTokens, outputTokens: u.outputTokens, model: u.model, createdAt: u.createdAt }));
    }

    return new Response(lines.join("\n") + "\n", {
      headers: {
        "Content-Type": "application/x-ndjson",
        "Content-Disposition": `attachment; filename="${id}.jsonl"`,
      },
    });
  });

  /**
   * POST /api/export/encrypted
   * Body: { password: string, nousId?: string, since?: string }
   * Returns: encrypted NDJSON payload (AES-256-GCM, scrypt KDF).
   * Format: JSON envelope { v, kdf, salt, n, r, p, iv, data }
   * where `data` is base64(ciphertext || authTag).
   */
  app.post("/api/export/encrypted", async (c) => {
    const body = await c.req.json<{ password?: unknown; nousId?: unknown; since?: unknown }>();
    const password = body.password;
    if (typeof password !== "string" || password.length < 8) {
      return c.json({ error: "password must be at least 8 characters" }, 400);
    }

    const nousId = typeof body.nousId === "string" ? body.nousId : undefined;
    const since = typeof body.since === "string" ? body.since : undefined;

    // Collect all sessions + messages as NDJSON
    const sessions = store.listSessionsFiltered({
      ...(nousId ? { nousId } : {}),
      ...(since ? { since } : {}),
    });
    const lines: string[] = [];
    for (const s of sessions) {
      lines.push(JSON.stringify({ type: "session", ...s }));
      const messages = store.getHistory(s.id, { excludeDistilled: false });
      for (const m of messages) {
        lines.push(JSON.stringify({ type: "message", sessionId: s.id, seq: m.seq, role: m.role, content: m.content, createdAt: m.createdAt }));
      }
    }
    const plaintext = Buffer.from(lines.join("\n") + "\n", "utf8");

    // Derive key: scrypt N=16384 r=8 p=1 → 32 bytes
    const salt = randomBytes(32);
    const N = 16384, r = 8, p = 1;
    const key = scryptSync(password, salt, 32, { N, r, p });

    // Encrypt: AES-256-GCM
    const iv = randomBytes(12);
    const cipher = createCipheriv("aes-256-gcm", key, iv);
    const ct = Buffer.concat([cipher.update(plaintext), cipher.final()]);
    const authTag = cipher.getAuthTag();
    const data = Buffer.concat([ct, authTag]).toString("base64");

    const envelope = {
      v: 1,
      kdf: "scrypt",
      salt: salt.toString("base64"),
      n: N, r, p,
      iv: iv.toString("base64"),
      cipher: "aes-256-gcm",
      data,
      sessionCount: sessions.length,
      exportedAt: new Date().toISOString(),
    };

    return new Response(JSON.stringify(envelope), {
      headers: {
        "Content-Type": "application/json",
        "Content-Disposition": `attachment; filename="aletheia-export-${Date.now()}.enc.json"`,
      },
    });
  });

  // Blackboard API — for prosoche and external systems to post broadcasts
  app.get("/api/blackboard", (c) => {
    return c.json({ entries: store.blackboardList() });
  });

  app.post("/api/blackboard", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = (await c.req.json()) as Record<string, unknown>;
    } catch {
      return c.json({ error: "Invalid JSON" }, 400);
    }
    const key = body["key"] as string;
    const value = body["value"] as string;
    const author = (body["author"] as string) ?? "prosoche";
    const ttl = (body["ttl_seconds"] as number) ?? 3600;
    if (!key || !value) {
      return c.json({ error: "key and value required" }, 400);
    }
    const id = store.blackboardWrite(key, value, author, ttl);
    return c.json({ ok: true, id });
  });

  // --- Workspace File Explorer API ---

  function resolveAgentWorkspace(agentId?: string): string | null {
    const id = agentId ?? config.agents.list.find((a) => a.default)?.id ?? config.agents.list[0]?.id;
    if (!id) return null;
    const agent = config.agents.list.find((a) => a.id === id);
    return agent?.workspace ?? null;
  }

  function safeWorkspacePath(workspace: string, userPath: string): string | null {
    const resolved = resolve(workspace, userPath);
    if (!resolved.startsWith(workspace)) return null;
    return resolved;
  }

  interface TreeEntry {
    name: string;
    type: "file" | "directory";
    size?: number | undefined;
    modified?: string | undefined;
    children?: TreeEntry[] | undefined;
  }

  function buildTree(dirPath: string, depth: number, maxDepth: number): TreeEntry[] {
    if (depth >= maxDepth) return [];
    try {
      const entries = readdirSync(dirPath, { withFileTypes: true });
      const result: TreeEntry[] = [];
      for (const entry of entries) {
        if (entry.name.startsWith(".")) continue;
        const fullPath = join(dirPath, entry.name);
        try {
          const stat = statSync(fullPath);
          if (entry.isDirectory()) {
            result.push({
              name: entry.name,
              type: "directory",
              modified: stat.mtime.toISOString(),
              children: depth + 1 < maxDepth ? buildTree(fullPath, depth + 1, maxDepth) : undefined,
            });
          } else {
            result.push({
              name: entry.name,
              type: "file",
              size: stat.size,
              modified: stat.mtime.toISOString(),
            });
          }
        } catch (err) {
          log.debug(`Skipping unreadable entry ${entry.name}: ${err instanceof Error ? err.message : err}`);
        }
      }
      // directories first, then files, alphabetical within each
      result.sort((a, b) => {
        if (a.type !== b.type) return a.type === "directory" ? -1 : 1;
        return a.name.localeCompare(b.name);
      });
      return result;
    } catch (err) {
      log.debug(`buildTree failed for directory: ${err instanceof Error ? err.message : err}`);
      return [];
    }
  }

  app.get("/api/workspace/tree", (c) => {
    const agentId = c.req.query("agentId");
    const subpath = c.req.query("path") ?? "";
    const depth = Math.min(parseInt(c.req.query("depth") ?? "2", 10), 5);
    const workspace = resolveAgentWorkspace(agentId ?? undefined);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    const targetPath = subpath ? safeWorkspacePath(workspace, subpath) : workspace;
    if (!targetPath) return c.json({ error: "Invalid path" }, 400);
    if (!existsSync(targetPath)) return c.json({ error: "Path not found" }, 404);

    const tree = buildTree(targetPath, 0, depth);
    return c.json({ root: subpath || ".", entries: tree });
  });

  app.get("/api/workspace/file", (c) => {
    const agentId = c.req.query("agentId");
    const filePath = c.req.query("path");
    if (!filePath) return c.json({ error: "path required" }, 400);

    const workspace = resolveAgentWorkspace(agentId ?? undefined);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    const resolved = safeWorkspacePath(workspace, filePath);
    if (!resolved) return c.json({ error: "Invalid path" }, 400);
    if (!existsSync(resolved)) return c.json({ error: "File not found" }, 404);

    try {
      const stat = statSync(resolved);
      if (stat.isDirectory()) return c.json({ error: "Path is a directory" }, 400);
      if (stat.size > 1_048_576) return c.json({ error: "File too large (>1MB)" }, 400);

      const content = readFileSync(resolved, "utf-8");
      return c.json({ path: filePath, size: stat.size, content });
    } catch (err) {
      return c.json({ error: err instanceof Error ? err.message : "Read failed" }, 500);
    }
  });

  app.put("/api/workspace/file", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = (await c.req.json()) as Record<string, unknown>;
    } catch {
      return c.json({ error: "Invalid JSON" }, 400);
    }

    const filePath = body["path"] as string;
    const content = body["content"];
    const agentId = body["agentId"] as string | undefined;

    if (!filePath || typeof content !== "string") {
      return c.json({ error: "path and content required" }, 400);
    }

    const workspace = resolveAgentWorkspace(agentId);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    const resolved = safeWorkspacePath(workspace, filePath);
    if (!resolved) return c.json({ error: "Invalid path" }, 400);

    try {
      writeFileSync(resolved, content, "utf-8");
      return c.json({ ok: true, path: filePath, size: Buffer.byteLength(content) });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error(`Workspace file write failed: ${msg}`);
      return c.json({ error: msg }, 500);
    }
  });

  app.get("/api/workspace/git-status", (c) => {
    const agentId = c.req.query("agentId");
    const workspace = resolveAgentWorkspace(agentId ?? undefined);
    if (!workspace) return c.json({ error: "No workspace configured" }, 400);

    try {
      const output = execSync("git status --porcelain 2>/dev/null || true", {
        cwd: workspace,
        encoding: "utf-8",
        timeout: 5000,
      });
      const files: Array<{ status: string; path: string }> = [];
      for (const line of output.split("\n")) {
        if (!line.trim()) continue;
        const status = line.slice(0, 2).trim();
        const path = line.slice(3);
        files.push({ status, path });
      }
      return c.json({ files });
    } catch (err) {
      log.debug(`git-status failed: ${err instanceof Error ? err.message : err}`);
      return c.json({ files: [] });
    }
  });

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
