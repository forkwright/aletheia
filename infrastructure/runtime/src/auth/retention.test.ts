// Auth data retention enforcement tests
import { beforeEach, describe, expect, it } from "vitest";
import Database from "better-sqlite3";
import { enforceRetention, type RetentionPolicy } from "./retention.js";

function makePolicy(overrides?: Partial<RetentionPolicy>): RetentionPolicy {
  return {
    activeSessionMaxAgeDays: 30,
    archivedRetentionDays: 90,
    distilledMessageRetentionDays: 60,
    toolResultRetentionDays: 30,
    auditLogRetentionDays: 90,
    ...overrides,
  };
}

function setupSchema(db: Database.Database): void {
  db.exec(`
    CREATE TABLE sessions (
      id TEXT PRIMARY KEY,
      session_key TEXT NOT NULL,
      status TEXT NOT NULL DEFAULT 'active',
      updated_at TEXT NOT NULL,
      created_at TEXT NOT NULL
    );
    CREATE TABLE messages (
      id INTEGER PRIMARY KEY,
      session_id TEXT NOT NULL,
      role TEXT NOT NULL DEFAULT 'user',
      content TEXT NOT NULL,
      is_distilled INTEGER NOT NULL DEFAULT 0,
      created_at TEXT NOT NULL
    );
    CREATE TABLE usage (
      id INTEGER PRIMARY KEY,
      session_id TEXT NOT NULL
    );
    CREATE TABLE distillations (
      id INTEGER PRIMARY KEY,
      session_id TEXT NOT NULL
    );
    CREATE TABLE audit_log (
      id INTEGER PRIMARY KEY,
      timestamp TEXT NOT NULL
    );
  `);
}

describe("enforceRetention", () => {
  let db: Database.Database;

  beforeEach(() => {
    db = new Database(":memory:");
    setupSchema(db);
  });

  it("returns all zeros on empty database", () => {
    const result = enforceRetention(db, makePolicy());
    expect(result).toEqual({ archived: 0, purged: 0, trimmed: 0, auditPurged: 0 });
  });

  it("archives stale active sessions", () => {
    const old = new Date(Date.now() - 60 * 86400000).toISOString();
    db.prepare("INSERT INTO sessions (id, session_key, status, updated_at, created_at) VALUES (?, ?, 'active', ?, ?)").run("s1", "key:s1", old, old);
    db.prepare("INSERT INTO sessions (id, session_key, status, updated_at, created_at) VALUES (?, ?, 'active', ?, ?)").run("s2", "key:s2", new Date().toISOString(), new Date().toISOString());

    const result = enforceRetention(db, makePolicy({ activeSessionMaxAgeDays: 30 }));
    expect(result.archived).toBe(1);

    const s1 = db.prepare("SELECT status FROM sessions WHERE id = 's1'").get() as { status: string };
    expect(s1.status).toBe("archived");
  });

  it("purges old archived sessions and cascading data", () => {
    const old = new Date(Date.now() - 120 * 86400000).toISOString();
    db.prepare("INSERT INTO sessions (id, session_key, status, updated_at, created_at) VALUES (?, ?, 'archived', ?, ?)").run("s1", "key:s1", old, old);
    db.prepare("INSERT INTO messages (session_id, content, created_at) VALUES (?, 'hello', ?)").run("s1", old);
    db.prepare("INSERT INTO usage (session_id) VALUES (?)").run("s1");
    db.prepare("INSERT INTO distillations (session_id) VALUES (?)").run("s1");

    const result = enforceRetention(db, makePolicy({ archivedRetentionDays: 90 }));
    expect(result.purged).toBe(1);

    const remaining = db.prepare("SELECT COUNT(*) as cnt FROM sessions").get() as { cnt: number };
    expect(remaining.cnt).toBe(0);
    const msgs = db.prepare("SELECT COUNT(*) as cnt FROM messages").get() as { cnt: number };
    expect(msgs.cnt).toBe(0);
  });

  it("deletes old distilled messages", () => {
    const old = new Date(Date.now() - 90 * 86400000).toISOString();
    db.prepare("INSERT INTO sessions (id, session_key, status, updated_at, created_at) VALUES (?, ?, 'active', ?, ?)").run("s1", "key:s1", new Date().toISOString(), new Date().toISOString());
    db.prepare("INSERT INTO messages (session_id, content, is_distilled, created_at) VALUES (?, 'old summary', 1, ?)").run("s1", old);
    db.prepare("INSERT INTO messages (session_id, content, is_distilled, created_at) VALUES (?, 'recent msg', 0, ?)").run("s1", new Date().toISOString());

    const result = enforceRetention(db, makePolicy({ distilledMessageRetentionDays: 60 }));
    expect(result.trimmed).toBeGreaterThanOrEqual(1);

    const msgs = db.prepare("SELECT COUNT(*) as cnt FROM messages").get() as { cnt: number };
    expect(msgs.cnt).toBe(1);
  });

  it("truncates old tool results", () => {
    const old = new Date(Date.now() - 60 * 86400000).toISOString();
    db.prepare("INSERT INTO sessions (id, session_key, status, updated_at, created_at) VALUES (?, ?, 'active', ?, ?)").run("s1", "key:s1", new Date().toISOString(), new Date().toISOString());
    db.prepare("INSERT INTO messages (session_id, role, content, created_at) VALUES (?, 'tool_result', ?, ?)").run("s1", "x".repeat(200), old);

    const result = enforceRetention(db, makePolicy({ toolResultRetentionDays: 30 }));
    expect(result.trimmed).toBeGreaterThanOrEqual(1);

    const msg = db.prepare("SELECT content FROM messages WHERE role = 'tool_result'").get() as { content: string };
    expect(msg.content).toBe("[truncated]");
  });

  it("purges old audit log entries", () => {
    const old = new Date(Date.now() - 120 * 86400000).toISOString();
    db.prepare("INSERT INTO audit_log (timestamp) VALUES (?)").run(old);
    db.prepare("INSERT INTO audit_log (timestamp) VALUES (?)").run(new Date().toISOString());

    const result = enforceRetention(db, makePolicy({ auditLogRetentionDays: 90 }));
    expect(result.auditPurged).toBe(1);

    const remaining = db.prepare("SELECT COUNT(*) as cnt FROM audit_log").get() as { cnt: number };
    expect(remaining.cnt).toBe(1);
  });

  it("handles missing audit_log table gracefully", () => {
    db.exec("DROP TABLE audit_log");
    const old = new Date(Date.now() - 60 * 86400000).toISOString();
    db.prepare("INSERT INTO sessions (id, session_key, status, updated_at, created_at) VALUES (?, ?, 'active', ?, ?)").run("s1", "key:s1", old, old);

    const result = enforceRetention(db, makePolicy());
    expect(result.archived).toBe(1);
    expect(result.auditPurged).toBe(0);
  });
});
