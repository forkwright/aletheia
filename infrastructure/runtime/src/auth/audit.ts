// Structured audit logging
import { createHash } from "node:crypto";
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";

const log = createLogger("audit");

export interface AuditEntry {
  timestamp: string;
  actor: string;
  role: string;
  action: string;
  target?: string;
  ip: string;
  userAgent?: string;
  status: number;
  durationMs: number;
}

export class AuditLog {
  constructor(private db: Database.Database) {
    this.init();
  }

  private init(): void {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS audit_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        actor TEXT NOT NULL,
        role TEXT NOT NULL,
        action TEXT NOT NULL,
        target TEXT,
        ip TEXT,
        user_agent TEXT,
        status INTEGER NOT NULL,
        duration_ms INTEGER,
        checksum TEXT,
        previous_checksum TEXT
      );
      CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_log(actor);
      CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);
    `);

    // Add hash chain columns (idempotent for existing DBs)
    try { this.db.exec("ALTER TABLE audit_log ADD COLUMN checksum TEXT"); } catch { /* already exists */ }
    try { this.db.exec("ALTER TABLE audit_log ADD COLUMN previous_checksum TEXT"); } catch { /* already exists */ }
  }

  record(entry: AuditEntry): void {
    try {
      const prev = this.db
        .prepare("SELECT checksum FROM audit_log ORDER BY id DESC LIMIT 1")
        .get() as { checksum: string | null } | undefined;
      const previousChecksum = prev?.checksum ?? "GENESIS";

      const payload = [
        entry.timestamp,
        entry.actor,
        entry.role,
        entry.action,
        entry.target ?? "",
        entry.ip,
        entry.status.toString(),
        previousChecksum,
      ].join("|");
      const checksum = createHash("sha256").update(payload).digest("hex");

      this.db
        .prepare(
          `INSERT INTO audit_log (timestamp, actor, role, action, target, ip, user_agent, status, duration_ms, checksum, previous_checksum)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
        )
        .run(
          entry.timestamp,
          entry.actor,
          entry.role,
          entry.action,
          entry.target ?? null,
          entry.ip,
          entry.userAgent ?? null,
          entry.status,
          entry.durationMs,
          checksum,
          previousChecksum,
        );
    } catch (err) {
      log.error(`Failed to write audit entry: ${err}`);
    }
  }

  query(opts?: {
    actor?: string;
    since?: string;
    until?: string;
    limit?: number;
  }): AuditEntry[] {
    const conditions: string[] = [];
    const params: unknown[] = [];

    if (opts?.actor) {
      conditions.push("actor = ?");
      params.push(opts.actor);
    }
    if (opts?.since) {
      conditions.push("timestamp >= ?");
      params.push(opts.since);
    }
    if (opts?.until) {
      conditions.push("timestamp <= ?");
      params.push(opts.until);
    }

    const where =
      conditions.length > 0 ? `WHERE ${conditions.join(" AND ")}` : "";
    const limit = opts?.limit ?? 100;

    const rows = this.db
      .prepare(
        `SELECT * FROM audit_log ${where} ORDER BY timestamp DESC LIMIT ?`,
      )
      .all(...params, limit) as Array<Record<string, unknown>>;

    return rows.map((r) => ({
      timestamp: r["timestamp"] as string,
      actor: r["actor"] as string,
      role: r["role"] as string,
      action: r["action"] as string,
      ...(r["target"] ? { target: r["target"] as string } : {}),
      ip: r["ip"] as string,
      ...(r["user_agent"] ? { userAgent: r["user_agent"] as string } : {}),
      status: r["status"] as number,
      durationMs: r["duration_ms"] as number,
    }));
  }

  cleanup(maxAgeDays = 90): number {
    const cutoff = new Date(
      Date.now() - maxAgeDays * 24 * 60 * 60 * 1000,
    ).toISOString();
    const result = this.db
      .prepare("DELETE FROM audit_log WHERE timestamp < ?")
      .run(cutoff);
    return result.changes;
  }
}
