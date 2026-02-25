// Auth session store tests
import { beforeEach, describe, expect, it } from "vitest";
import Database from "better-sqlite3";
import { AuthSessionStore } from "./sessions.js";

describe("AuthSessionStore", () => {
  let db: Database.Database;
  let store: AuthSessionStore;

  beforeEach(() => {
    db = new Database(":memory:");
    store = new AuthSessionStore(db);
  });

  const createOpts = (overrides?: Record<string, unknown>) => ({
    username: "alice",
    role: "user",
    refreshTokenTtl: 3600,
    ...overrides,
  });

  it("creates a session and returns sessionId + refreshToken", () => {
    const result = store.create(createOpts());
    expect(result.sessionId).toHaveLength(32);
    expect(result.refreshToken).toBeTruthy();
    expect(result.refreshToken.length).toBeGreaterThan(20);
  });

  it("validates a refresh token round-trip", () => {
    const { refreshToken } = store.create(createOpts());
    const session = store.validateRefresh(refreshToken);
    expect(session).not.toBeNull();
    expect(session!.username).toBe("alice");
    expect(session!.role).toBe("user");
    expect(session!.revoked).toBe(false);
  });

  it("returns null for invalid refresh token", () => {
    store.create(createOpts());
    expect(store.validateRefresh("wrong-token")).toBeNull();
  });

  it("rotates refresh tokens", () => {
    const { refreshToken: oldToken } = store.create(createOpts());
    const rotated = store.rotate(oldToken, 7200);
    expect(rotated).not.toBeNull();
    expect(rotated!.refreshToken).not.toBe(oldToken);

    // Old token is now invalid
    expect(store.validateRefresh(oldToken)).toBeNull();
    // New token works
    expect(store.validateRefresh(rotated!.refreshToken)).not.toBeNull();
  });

  it("returns null when rotating invalid token", () => {
    expect(store.rotate("nonexistent", 3600)).toBeNull();
  });

  it("revokes a session", () => {
    const { sessionId, refreshToken } = store.create(createOpts());
    expect(store.revoke(sessionId)).toBe(true);
    expect(store.validateRefresh(refreshToken)).toBeNull();
  });

  it("returns false when revoking nonexistent session", () => {
    expect(store.revoke("nonexistent-id")).toBe(false);
  });

  it("revokes all sessions for a user", () => {
    const { refreshToken: t1 } = store.create(createOpts());
    const { refreshToken: t2 } = store.create(createOpts());
    store.create(createOpts({ username: "bob" }));

    const count = store.revokeAllForUser("alice");
    expect(count).toBe(2);
    expect(store.validateRefresh(t1)).toBeNull();
    expect(store.validateRefresh(t2)).toBeNull();
  });

  it("lists active sessions for a user", () => {
    store.create(createOpts());
    store.create(createOpts());
    store.create(createOpts({ username: "bob" }));

    const list = store.listForUser("alice");
    expect(list).toHaveLength(2);
    expect(list.every((s) => s.username === "alice")).toBe(true);
  });

  it("does not list revoked sessions", () => {
    const { sessionId } = store.create(createOpts());
    store.create(createOpts());
    store.revoke(sessionId);

    const list = store.listForUser("alice");
    expect(list).toHaveLength(1);
  });

  it("cleans up revoked sessions", () => {
    const { sessionId } = store.create(createOpts());
    store.revoke(sessionId);

    const deleted = store.cleanup();
    expect(deleted).toBeGreaterThanOrEqual(1);

    const rows = db.prepare("SELECT COUNT(*) as cnt FROM auth_sessions").get() as { cnt: number };
    expect(rows.cnt).toBe(0);
  });

  it("evicts oldest session when maxSessions exceeded", () => {
    const { refreshToken: t1 } = store.create(createOpts({ maxSessions: 2 }));
    store.create(createOpts({ maxSessions: 2 }));

    // Third session should evict the oldest
    store.create(createOpts({ maxSessions: 2 }));

    // First token should have been evicted
    expect(store.validateRefresh(t1)).toBeNull();
    const list = store.listForUser("alice");
    expect(list).toHaveLength(2);
  });

  it("stores IP address and user agent", () => {
    const { refreshToken } = store.create(
      createOpts({ ipAddress: "10.0.0.1", userAgent: "TestBot/1.0" }),
    );
    const session = store.validateRefresh(refreshToken);
    expect(session!.ipAddress).toBe("10.0.0.1");
    expect(session!.userAgent).toBe("TestBot/1.0");
  });
});
