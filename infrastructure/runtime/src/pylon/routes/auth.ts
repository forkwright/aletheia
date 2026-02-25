// Auth routes — login, logout, refresh, sessions, revoke
import { Hono } from "hono";
import type { RouteDeps, RouteRefs } from "./deps.js";
import { getUser } from "./deps.js";

function getRefreshCookie(c: import("hono").Context): string | undefined {
  const header = c.req.header("Cookie") ?? "";
  const match = header.match(/(?:^|;\s*)aletheia_refresh=([^;]*)/);
  return match?.[1];
}

function setRefreshCookie(
  c: import("hono").Context,
  token: string,
  secureCookies: boolean,
  maxAge?: number,
): void {
  const parts = [
    `aletheia_refresh=${token}`,
    "HttpOnly",
    "SameSite=Strict",
    "Path=/api/auth",
  ];
  if (secureCookies) parts.push("Secure");
  if (maxAge !== undefined) parts.push(`Max-Age=${maxAge}`);
  c.header("Set-Cookie", parts.join("; "));
}

function clearRefreshCookie(c: import("hono").Context, secureCookies: boolean): void {
  const parts = [
    "aletheia_refresh=",
    "HttpOnly",
    "SameSite=Strict",
    "Path=/api/auth",
    "Max-Age=0",
  ];
  if (secureCookies) parts.push("Secure");
  c.header("Set-Cookie", parts.join("; "));
}

/**
 * Determine whether the Secure flag should be set on cookies.
 * When behind a TLS-terminating reverse proxy (Caddy, nginx), the backend
 * sees plain HTTP but the browser sees HTTPS.  In that case the configured
 * `secureCookies` should be false — but if the user forgets, we auto-detect
 * by checking for X-Forwarded-Proto / X-Real-IP headers that proxies inject.
 */
function resolveSecureCookies(
  c: import("hono").Context,
  configured: boolean,
): boolean {
  if (!configured) return false;
  // If the proxy tells us the original request was HTTPS, Secure is safe
  const proto = c.req.header("X-Forwarded-Proto");
  if (proto === "https") return true;
  // If we see proxy headers but no proto, assume TLS termination → don't set Secure
  if (c.req.header("X-Forwarded-For") || c.req.header("X-Real-IP")) return false;
  return configured;
}

export function authRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { authConfig, authSessionStore, authRoutes: auth } = deps;
  const configuredSecureCookies = authConfig.session?.secureCookies ?? true;

  app.get("/api/auth/mode", (c) => {
    return c.json(auth.mode());
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

    const result = await auth.login(username, password, ip, userAgent);
    if (!result) {
      return c.json({ error: "Invalid credentials" }, 401);
    }

    const maxAge = rememberMe
      ? authConfig.session?.refreshTokenTtl
      : undefined;
    const secure = resolveSecureCookies(c, configuredSecureCookies);
    setRefreshCookie(c, result.refreshToken, secure, maxAge);

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

    const result = await auth.refresh(refreshToken);
    const secure = resolveSecureCookies(c, configuredSecureCookies);
    if (!result) {
      clearRefreshCookie(c, secure);
      return c.json({ error: "Invalid or expired refresh token" }, 401);
    }

    setRefreshCookie(c, result.refreshToken, secure, authConfig.session?.refreshTokenTtl);

    return c.json({
      accessToken: result.accessToken,
      expiresIn: result.expiresIn,
    });
  });

  app.post("/api/auth/logout", (c) => {
    const user = getUser(c);
    if (user?.sessionId) {
      auth.logout(user.sessionId);
    }
    const secure = resolveSecureCookies(c, configuredSecureCookies);
    clearRefreshCookie(c, secure);
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

  return app;
}
