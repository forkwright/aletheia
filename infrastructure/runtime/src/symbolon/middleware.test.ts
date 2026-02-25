// Auth middleware tests
import { beforeEach, describe, expect, it, vi } from "vitest";
import { type AuthConfig, createAuthMiddleware, createAuthRoutes } from "./middleware.js";
import type { Context, Next } from "hono";
import Database from "better-sqlite3";
import { AuthSessionStore } from "./sessions.js";
import { AuditLog } from "./audit.js";
import { hashPassword } from "./passwords.js";
import { signToken } from "./tokens.js";

function makeContext(overrides?: Record<string, unknown>): { c: Context; json: ReturnType<typeof vi.fn>; set: ReturnType<typeof vi.fn>; status: number } {
  const headers = new Map<string, string>();
  const _queryParams = new Map<string, string>();
  const json = vi.fn().mockReturnValue(new Response());
  const set = vi.fn();
  const headerFn = vi.fn().mockImplementation((name: string, value?: string) => {
    if (value !== undefined) { headers.set(name, value); return; }
    return headers.get(name) ?? undefined;
  });

  const c = {
    req: {
      path: overrides?.path ?? "/api/chat",
      method: overrides?.method ?? "POST",
      header: vi.fn().mockImplementation((name: string) => {
        const map = overrides?.headers as Record<string, string> ?? {};
        return map[name] ?? undefined;
      }),
      query: vi.fn().mockImplementation((name: string) => {
        return (overrides?.query as Record<string, string>)?.[name] ?? undefined;
      }),
    },
    json,
    set,
    header: headerFn,
    res: { status: overrides?.resStatus ?? 200 },
  } as unknown as Context;

  return { c, json, set, status: 200 };
}

describe("createAuthMiddleware", () => {
  const next: Next = vi.fn().mockResolvedValue(undefined);

  beforeEach(() => { vi.clearAllMocks(); });

  it("skips auth for public paths", async () => {
    const mw = createAuthMiddleware({ mode: "token", token: "secret" }, null, null);
    const { c } = makeContext({ path: "/health" });
    await mw(c, next);
    expect(next).toHaveBeenCalled();
  });

  it("skips auth for UI paths", async () => {
    const mw = createAuthMiddleware({ mode: "token", token: "secret" }, null, null);
    const { c } = makeContext({ path: "/ui/index.html" });
    await mw(c, next);
    expect(next).toHaveBeenCalled();
  });

  it("mode=none sets anonymous admin", async () => {
    const mw = createAuthMiddleware({ mode: "none" }, null, null);
    const { c, set } = makeContext();
    await mw(c, next);
    expect(set).toHaveBeenCalledWith("user", { username: "anonymous", role: "admin" });
    expect(next).toHaveBeenCalled();
  });

  it("mode=token accepts valid bearer token", async () => {
    const mw = createAuthMiddleware({ mode: "token", token: "my-secret" }, null, null);
    const { c, set } = makeContext({ headers: { Authorization: "Bearer my-secret" } });
    await mw(c, next);
    expect(set).toHaveBeenCalledWith("user", expect.objectContaining({ role: "admin" }));
  });

  it("mode=token rejects invalid bearer token", async () => {
    const mw = createAuthMiddleware({ mode: "token", token: "my-secret" }, null, null);
    const { c, json } = makeContext({ headers: { Authorization: "Bearer wrong" } });
    await mw(c, next);
    expect(json).toHaveBeenCalledWith({ error: "Unauthorized" }, 401);
    expect(next).not.toHaveBeenCalled();
  });

  it("mode=token accepts token via query param", async () => {
    const mw = createAuthMiddleware({ mode: "token", token: "my-secret" }, null, null);
    const { c, set } = makeContext({ query: { token: "my-secret" } });
    await mw(c, next);
    expect(set).toHaveBeenCalledWith("user", expect.objectContaining({ role: "admin" }));
  });

  it("mode=session validates JWT and sets user", async () => {
    const secret = "test-secret-key-32chars-minimum!";
    const token = signToken({ sub: "alice", role: "user", sid: "s1", iat: Math.floor(Date.now() / 1000), exp: Math.floor(Date.now() / 1000) + 3600 }, secret);
    const mw = createAuthMiddleware({ mode: "session", session: { secret, accessTokenTtl: 3600, refreshTokenTtl: 86400, maxSessions: 5, secureCookies: false } }, null, null);
    const { c, set } = makeContext({ headers: { Authorization: `Bearer ${token}` } });
    await mw(c, next);
    expect(set).toHaveBeenCalledWith("user", expect.objectContaining({ username: "alice", role: "user" }));
  });

  it("mode=session rejects expired JWT", async () => {
    const secret = "test-secret-key-32chars-minimum!";
    const token = signToken({ sub: "alice", role: "user", sid: "s1", iat: Math.floor(Date.now() / 1000) - 7200, exp: Math.floor(Date.now() / 1000) - 3600 }, secret);
    const mw = createAuthMiddleware({ mode: "session", session: { secret, accessTokenTtl: 3600, refreshTokenTtl: 86400, maxSessions: 5, secureCookies: false } }, null, null);
    const { c, json } = makeContext({ headers: { Authorization: `Bearer ${token}` } });
    await mw(c, next);
    expect(json).toHaveBeenCalledWith({ error: "Invalid or expired token" }, 401);
  });

  it("records audit log on RBAC denial", async () => {
    const db = new Database(":memory:");
    const audit = new AuditLog(db);
    const recordSpy = vi.spyOn(audit, "record");

    const secret = "test-secret-key-32chars-minimum!";
    const token = signToken({ sub: "viewer", role: "readonly", sid: "s1", iat: Math.floor(Date.now() / 1000), exp: Math.floor(Date.now() / 1000) + 3600 }, secret);
    const mw = createAuthMiddleware({ mode: "session", session: { secret, accessTokenTtl: 3600, refreshTokenTtl: 86400, maxSessions: 5, secureCookies: false } }, null, audit);
    // Use a path that has a RBAC permission mapping and readonly can't access
    const { c, json } = makeContext({ path: "/api/sessions/send", method: "POST", headers: { Authorization: `Bearer ${token}` } });
    await mw(c, next);
    expect(json).toHaveBeenCalledWith({ error: "Forbidden" }, 403);
    expect(recordSpy).toHaveBeenCalledWith(expect.objectContaining({ status: 403, actor: "viewer" }));
  });
});

describe("createAuthRoutes", () => {
  it("mode() returns auth mode info", () => {
    const routes = createAuthRoutes({ mode: "session" } as AuthConfig, null);
    expect(routes.mode()).toEqual({ mode: "session", sessionAuth: true });
  });

  it("login returns null for non-session mode", async () => {
    const routes = createAuthRoutes({ mode: "token" } as AuthConfig, null);
    expect(await routes.login("user", "pass")).toBeNull();
  });

  it("login authenticates valid user and returns tokens", async () => {
    const db = new Database(":memory:");
    const store = new AuthSessionStore(db);
    const hash = hashPassword("correct-password");
    const config: AuthConfig = {
      mode: "session",
      session: { secret: "test-secret-32chars-at-minimum!!", accessTokenTtl: 3600, refreshTokenTtl: 86400, maxSessions: 5, secureCookies: false },
      users: [{ username: "alice", passwordHash: hash, role: "user" }],
    };

    const routes = createAuthRoutes(config, store);
    const result = await routes.login("alice", "correct-password");
    expect(result).not.toBeNull();
    expect(result!.username).toBe("alice");
    expect(result!.role).toBe("user");
    expect(result!.accessToken).toBeTruthy();
    expect(result!.refreshToken).toBeTruthy();
  });

  it("login returns null for wrong password", async () => {
    const db = new Database(":memory:");
    const store = new AuthSessionStore(db);
    const hash = hashPassword("correct-password");
    const config: AuthConfig = {
      mode: "session",
      session: { secret: "test-secret-32chars-at-minimum!!", accessTokenTtl: 3600, refreshTokenTtl: 86400, maxSessions: 5, secureCookies: false },
      users: [{ username: "alice", passwordHash: hash, role: "user" }],
    };

    const routes = createAuthRoutes(config, store);
    expect(await routes.login("alice", "wrong")).toBeNull();
  });

  it("refresh rotates tokens", async () => {
    const db = new Database(":memory:");
    const store = new AuthSessionStore(db);
    const hash = hashPassword("pass");
    const config: AuthConfig = {
      mode: "session",
      session: { secret: "test-secret-32chars-at-minimum!!", accessTokenTtl: 3600, refreshTokenTtl: 86400, maxSessions: 5, secureCookies: false },
      users: [{ username: "alice", passwordHash: hash, role: "user" }],
    };

    const routes = createAuthRoutes(config, store);
    const loginResult = await routes.login("alice", "pass");
    expect(loginResult).not.toBeNull();

    const refreshResult = await routes.refresh(loginResult!.refreshToken);
    expect(refreshResult).not.toBeNull();
    expect(refreshResult!.refreshToken).not.toBe(loginResult!.refreshToken);
  });

  it("logout revokes session", () => {
    const db = new Database(":memory:");
    const store = new AuthSessionStore(db);
    const { sessionId } = store.create({ username: "alice", role: "user", refreshTokenTtl: 3600 });

    const routes = createAuthRoutes({ mode: "session" } as AuthConfig, store);
    expect(routes.logout(sessionId)).toBe(true);
  });
});
