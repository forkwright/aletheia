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
});
