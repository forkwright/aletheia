import { describe, it, expect, beforeEach } from "vitest";
import Database from "better-sqlite3";
import { AuditLog } from "./audit.js";
import { verifyAuditChain } from "./audit-verify.js";

describe("AuditLog hash chain", () => {
  let db: Database.Database;
  let audit: AuditLog;

  beforeEach(() => {
    db = new Database(":memory:");
    audit = new AuditLog(db);
  });

  const entry = (action: string) => ({
    timestamp: new Date().toISOString(),
    actor: "test",
    role: "admin",
    action,
    ip: "127.0.0.1",
    status: 200,
    durationMs: 10,
  });

  it("records entries with checksums", () => {
    audit.record(entry("login"));
    const rows = db.prepare("SELECT checksum, previous_checksum FROM audit_log").all() as Array<Record<string, unknown>>;
    expect(rows).toHaveLength(1);
    expect(rows[0]!["checksum"]).toBeTruthy();
    expect(rows[0]!["previous_checksum"]).toBe("GENESIS");
  });

  it("chains checksums across entries", () => {
    audit.record(entry("login"));
    audit.record(entry("logout"));
    const rows = db.prepare("SELECT checksum, previous_checksum FROM audit_log ORDER BY id ASC").all() as Array<Record<string, unknown>>;
    expect(rows).toHaveLength(2);
    expect(rows[1]!["previous_checksum"]).toBe(rows[0]!["checksum"]);
  });

  it("verifies valid chain", () => {
    audit.record(entry("login"));
    audit.record(entry("read"));
    audit.record(entry("logout"));
    const result = verifyAuditChain(db);
    expect(result.valid).toBe(true);
    expect(result.checkedEntries).toBe(3);
  });

  it("detects tampered entry", () => {
    audit.record(entry("login"));
    audit.record(entry("secret_action"));
    audit.record(entry("logout"));

    db.prepare("UPDATE audit_log SET action = 'normal_action' WHERE id = 2").run();

    const result = verifyAuditChain(db);
    expect(result.valid).toBe(false);
    expect(result.tamperIndex).toBe(2);
  });

  it("detects chain break (deleted entry)", () => {
    audit.record(entry("login"));
    audit.record(entry("secret"));
    audit.record(entry("logout"));

    db.prepare("DELETE FROM audit_log WHERE id = 2").run();

    const result = verifyAuditChain(db);
    expect(result.valid).toBe(false);
  });

  it("handles empty audit log", () => {
    const result = verifyAuditChain(db);
    expect(result.valid).toBe(true);
    expect(result.totalEntries).toBe(0);
  });

  it("verifies large chain (100 entries)", () => {
    for (let i = 0; i < 100; i++) {
      audit.record(entry(`action_${i}`));
    }
    const result = verifyAuditChain(db);
    expect(result.valid).toBe(true);
    expect(result.checkedEntries).toBe(100);
  });
});

describe("AuditLog.query", () => {
  let db: Database.Database;
  let audit: AuditLog;

  beforeEach(() => {
    db = new Database(":memory:");
    audit = new AuditLog(db);
  });

  const entry = (action: string, actor = "test") => ({
    timestamp: new Date().toISOString(),
    actor,
    role: "admin",
    action,
    ip: "127.0.0.1",
    status: 200,
    durationMs: 10,
  });

  it("returns all entries with no filters", () => {
    audit.record(entry("login"));
    audit.record(entry("read"));
    const results = audit.query();
    expect(results).toHaveLength(2);
  });

  it("filters by actor", () => {
    audit.record(entry("login", "alice"));
    audit.record(entry("login", "bob"));
    const results = audit.query({ actor: "alice" });
    expect(results).toHaveLength(1);
    expect(results[0]!.actor).toBe("alice");
  });

  it("filters by since", () => {
    const old = new Date(Date.now() - 86400000).toISOString();
    audit.record({ ...entry("old_action"), timestamp: old });
    audit.record(entry("new_action"));

    const results = audit.query({ since: new Date(Date.now() - 3600000).toISOString() });
    expect(results).toHaveLength(1);
    expect(results[0]!.action).toBe("new_action");
  });

  it("respects limit", () => {
    for (let i = 0; i < 10; i++) audit.record(entry(`action_${i}`));
    const results = audit.query({ limit: 3 });
    expect(results).toHaveLength(3);
  });

  it("returns entries ordered by timestamp descending", () => {
    const t1 = "2025-01-01T00:00:00.000Z";
    const t2 = "2025-01-02T00:00:00.000Z";
    audit.record({ ...entry("first"), timestamp: t1 });
    audit.record({ ...entry("second"), timestamp: t2 });
    const results = audit.query();
    expect(results[0]!.action).toBe("second");
    expect(results[1]!.action).toBe("first");
  });
});

describe("AuditLog.cleanup", () => {
  let db: Database.Database;
  let audit: AuditLog;

  beforeEach(() => {
    db = new Database(":memory:");
    audit = new AuditLog(db);
  });

  it("removes entries older than maxAgeDays", () => {
    const old = new Date(Date.now() - 120 * 86400000).toISOString();
    audit.record({ timestamp: old, actor: "test", role: "admin", action: "old", ip: "127.0.0.1", status: 200, durationMs: 10 });
    audit.record({ timestamp: new Date().toISOString(), actor: "test", role: "admin", action: "new", ip: "127.0.0.1", status: 200, durationMs: 10 });

    const deleted = audit.cleanup(90);
    expect(deleted).toBe(1);

    const remaining = audit.query();
    expect(remaining).toHaveLength(1);
    expect(remaining[0]!.action).toBe("new");
  });

  it("returns 0 when nothing to clean", () => {
    audit.record({ timestamp: new Date().toISOString(), actor: "test", role: "admin", action: "recent", ip: "127.0.0.1", status: 200, durationMs: 10 });
    expect(audit.cleanup(90)).toBe(0);
  });
});
