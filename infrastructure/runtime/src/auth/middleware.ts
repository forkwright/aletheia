// TODO(unused): scaffolded for spec 3 (Auth & Updates) — not yet integrated into gateway
// Multi-mode auth middleware for Hono
import type { Context, Next } from "hono";
import { timingSafeEqual } from "node:crypto";
import { signToken, verifyToken } from "./tokens.js";
import { verifyPassword } from "./passwords.js";
import type { AuthSessionStore } from "./sessions.js";
import type { AuditLog } from "./audit.js";
import { getRequiredPermission, hasPermission } from "./rbac.js";

export interface AuthUser {
  username: string;
  role: string;
  sessionId?: string;
}

export interface AuthConfig {
  mode: "none" | "token" | "password" | "session";
  token?: string;
  session?: {
    secret: string;
    accessTokenTtl: number;
    refreshTokenTtl: number;
    maxSessions: number;
    secureCookies: boolean;
  };
  users?: Array<{ username: string; passwordHash: string; role: string }>;
  roles?: Record<string, string[]>;
}

function safeCompare(a: string, b: string): boolean {
  const bufA = Buffer.from(a);
  const bufB = Buffer.from(b);
  if (bufA.length !== bufB.length) return false;
  return timingSafeEqual(bufA, bufB);
}

function getClientIp(c: Context): string {
  return (
    c.req.header("X-Forwarded-For")?.split(",")[0]?.trim() ??
    c.req.header("X-Real-IP") ??
    "unknown"
  );
}

function extractBearerToken(c: Context): string | null {
  const header = c.req.header("Authorization");
  if (header?.startsWith("Bearer ")) return header.slice(7);
  return c.req.query("token") ?? null;
}

export function createAuthMiddleware(
  authConfig: AuthConfig,
  _sessionStore: AuthSessionStore | null,
  audit: AuditLog | null,
) {
  const skipPaths = new Set([
    "/health",
    "/api/branding",
    "/api/auth/login",
    "/api/auth/refresh",
    "/api/auth/mode",
  ]);

  return async (c: Context, next: Next) => {
    const path = c.req.path;

    // Skip auth for public endpoints and UI static assets
    if (skipPaths.has(path) || path.startsWith("/ui")) return next();

    // No-auth mode (development/testing)
    if (authConfig.mode === "none") {
      c.set("user", { username: "anonymous", role: "admin" } as AuthUser);
      return next();
    }

    const startTime = Date.now();
    let user: AuthUser | null = null;

    if (authConfig.mode === "token" && authConfig.token) {
      const token = extractBearerToken(c);
      if (!token || !safeCompare(token, authConfig.token)) {
        return c.json({ error: "Unauthorized" }, 401);
      }
      user = { username: "token", role: "admin" };
    } else if (authConfig.mode === "password" && authConfig.token) {
      const header = c.req.header("Authorization");
      if (!header?.startsWith("Basic ")) {
        c.header(
          "WWW-Authenticate",
          'Basic realm="Aletheia"',
        );
        return c.json({ error: "Unauthorized" }, 401);
      }
      const decoded = Buffer.from(header.slice(6), "base64").toString();
      const password = decoded.includes(":")
        ? decoded.split(":").slice(1).join(":")
        : decoded;
      if (!safeCompare(password, authConfig.token)) {
        return c.json({ error: "Invalid credentials" }, 401);
      }
      user = { username: "basic", role: "admin" };
    } else if (authConfig.mode === "session") {
      const token = extractBearerToken(c);
      if (!token || !authConfig.session) {
        return c.json({ error: "Unauthorized" }, 401);
      }
      const payload = verifyToken(token, authConfig.session.secret);
      if (!payload) {
        return c.json({ error: "Invalid or expired token" }, 401);
      }
      user = {
        username: payload.sub,
        role: payload.role,
        sessionId: payload.sid,
      };
    }

    if (!user) {
      return c.json({ error: "Unauthorized" }, 401);
    }

    // RBAC check
    const permission = getRequiredPermission(c.req.method, path);
    if (permission && !hasPermission(user.role, permission, authConfig.roles)) {
      if (audit) {
        audit.record({
          timestamp: new Date().toISOString(),
          actor: user.username,
          role: user.role,
          action: `${c.req.method} ${path}`,
          ip: getClientIp(c),
          userAgent: c.req.header("User-Agent") ?? "",
          status: 403,
          durationMs: Date.now() - startTime,
        });
      }
      return c.json({ error: "Forbidden" }, 403);
    }

    // Attach user to context for downstream handlers
    c.set("user", user);

    await next();

    // Audit log
    if (audit) {
      audit.record({
        timestamp: new Date().toISOString(),
        actor: user.username,
        role: user.role,
        action: `${c.req.method} ${path}`,
        ip: getClientIp(c),
        userAgent: c.req.header("User-Agent") ?? "",
        status: c.res.status,
        durationMs: Date.now() - startTime,
      });
    }
  };
}

export function createAuthRoutes(
  authConfig: AuthConfig,
  sessionStore: AuthSessionStore | null,
) {
  return {
    mode: () => ({
      mode: authConfig.mode,
      sessionAuth: authConfig.mode === "session",
    }),

    login: async (
      username: string,
      password: string,
      ip?: string,
      userAgent?: string,
    ): Promise<{
      accessToken: string;
      refreshToken: string;
      expiresIn: number;
      username: string;
      role: string;
    } | null> => {
      if (authConfig.mode !== "session" || !authConfig.session || !sessionStore) {
        return null;
      }

      const user = authConfig.users?.find((u) => u.username === username);
      if (!user) return null;

      if (!verifyPassword(password, user.passwordHash)) return null;

      const { sessionId, refreshToken } = sessionStore.create({
        username: user.username,
        role: user.role,
        refreshTokenTtl: authConfig.session.refreshTokenTtl,
        ...(ip ? { ipAddress: ip } : {}),
        ...(userAgent ? { userAgent } : {}),
        maxSessions: authConfig.session.maxSessions,
      });

      const accessToken = signToken(
        {
          sub: user.username,
          role: user.role,
          sid: sessionId,
          iat: Math.floor(Date.now() / 1000),
          exp:
            Math.floor(Date.now() / 1000) +
            authConfig.session.accessTokenTtl,
        },
        authConfig.session.secret,
      );

      return {
        accessToken,
        refreshToken,
        expiresIn: authConfig.session.accessTokenTtl,
        username: user.username,
        role: user.role,
      };
    },

    refresh: async (
      refreshToken: string,
    ): Promise<{
      accessToken: string;
      refreshToken: string;
      expiresIn: number;
    } | null> => {
      if (authConfig.mode !== "session" || !authConfig.session || !sessionStore) {
        return null;
      }

      const session = sessionStore.validateRefresh(refreshToken);
      if (!session) return null;

      const rotated = sessionStore.rotate(
        refreshToken,
        authConfig.session.refreshTokenTtl,
      );
      if (!rotated) return null;

      const accessToken = signToken(
        {
          sub: session.username,
          role: session.role,
          sid: rotated.sessionId,
          iat: Math.floor(Date.now() / 1000),
          exp:
            Math.floor(Date.now() / 1000) +
            authConfig.session.accessTokenTtl,
        },
        authConfig.session.secret,
      );

      return {
        accessToken,
        refreshToken: rotated.refreshToken,
        expiresIn: authConfig.session.accessTokenTtl,
      };
    },

    logout: (sessionId: string): boolean => {
      if (!sessionStore) return false;
      return sessionStore.revoke(sessionId);
    },
  };
}
