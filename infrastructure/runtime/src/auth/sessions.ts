// Auth session CRUD — SQLite-backed
import type Database from "better-sqlite3";
import { createHash } from "node:crypto";
import { generateRefreshToken, generateSessionId } from "./tokens.js";

export interface AuthSession {
  id: string;
  username: string;
  role: string;
  refreshTokenHash: string;
  createdAt: string;
  lastUsedAt: string;
  expiresAt: string;
  revoked: boolean;
  ipAddress: string | null;
  userAgent: string | null;
}

function hashRefreshToken(token: string): string {
  return createHash("sha256").update(token).digest("hex");
}

export class AuthSessionStore {
  constructor(private db: Database.Database) {
    this.init();
  }

  private init(): void {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS auth_sessions (
        id TEXT PRIMARY KEY,
        username TEXT NOT NULL,
        role TEXT NOT NULL,
        refresh_token_hash TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        last_used_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        expires_at TEXT NOT NULL,
        revoked INTEGER NOT NULL DEFAULT 0,
        ip_address TEXT,
        user_agent TEXT
      );
      CREATE INDEX IF NOT EXISTS idx_auth_sessions_username ON auth_sessions(username);
      CREATE INDEX IF NOT EXISTS idx_auth_sessions_expires ON auth_sessions(expires_at);
    `);
  }

  create(opts: {
    username: string;
    role: string;
    refreshTokenTtl: number;
    ipAddress?: string;
    userAgent?: string;
    maxSessions?: number;
  }): { sessionId: string; refreshToken: string } {
    const sessionId = generateSessionId();
    const refreshToken = generateRefreshToken();
    const tokenHash = hashRefreshToken(refreshToken);
    const expiresAt = new Date(
      Date.now() + opts.refreshTokenTtl * 1000,
    ).toISOString();

    // Evict oldest sessions if over limit
    if (opts.maxSessions && opts.maxSessions > 0) {
      const count = (
        this.db
          .prepare(
            "SELECT COUNT(*) as cnt FROM auth_sessions WHERE username = ? AND revoked = 0",
          )
          .get(opts.username) as { cnt: number }
      ).cnt;

      if (count >= opts.maxSessions) {
        this.db
          .prepare(
            `DELETE FROM auth_sessions
             WHERE id IN (
               SELECT id FROM auth_sessions
               WHERE username = ? AND revoked = 0
               ORDER BY last_used_at ASC
               LIMIT ?
             )`,
          )
          .run(opts.username, count - opts.maxSessions + 1);
      }
    }

    this.db
      .prepare(
        `INSERT INTO auth_sessions (id, username, role, refresh_token_hash, expires_at, ip_address, user_agent)
         VALUES (?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        sessionId,
        opts.username,
        opts.role,
        tokenHash,
        expiresAt,
        opts.ipAddress ?? null,
        opts.userAgent ?? null,
      );

    return { sessionId, refreshToken };
  }

  validateRefresh(refreshToken: string): AuthSession | null {
    const tokenHash = hashRefreshToken(refreshToken);
    const row = this.db
      .prepare(
        `SELECT * FROM auth_sessions
         WHERE refresh_token_hash = ?
           AND revoked = 0
           AND expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`,
      )
      .get(tokenHash) as Record<string, unknown> | undefined;

    if (!row) return null;

    // Update last_used_at
    this.db
      .prepare(
        "UPDATE auth_sessions SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
      )
      .run(row["id"]);

    return this.mapSession(row);
  }

  rotate(
    oldRefreshToken: string,
    refreshTokenTtl: number,
  ): { sessionId: string; refreshToken: string } | null {
    const tokenHash = hashRefreshToken(oldRefreshToken);
    const row = this.db
      .prepare(
        `SELECT * FROM auth_sessions
         WHERE refresh_token_hash = ?
           AND revoked = 0
           AND expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`,
      )
      .get(tokenHash) as Record<string, unknown> | undefined;

    if (!row) return null;

    const sessionId = row["id"] as string;
    const newRefresh = generateRefreshToken();
    const newHash = hashRefreshToken(newRefresh);
    const newExpiry = new Date(
      Date.now() + refreshTokenTtl * 1000,
    ).toISOString();

    this.db
      .prepare(
        `UPDATE auth_sessions
         SET refresh_token_hash = ?,
             expires_at = ?,
             last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?`,
      )
      .run(newHash, newExpiry, sessionId);

    return { sessionId, refreshToken: newRefresh };
  }

  revoke(sessionId: string): boolean {
    const result = this.db
      .prepare("UPDATE auth_sessions SET revoked = 1 WHERE id = ?")
      .run(sessionId);
    return result.changes > 0;
  }

  revokeAllForUser(username: string): number {
    const result = this.db
      .prepare(
        "UPDATE auth_sessions SET revoked = 1 WHERE username = ? AND revoked = 0",
      )
      .run(username);
    return result.changes;
  }

  listForUser(username: string): AuthSession[] {
    const rows = this.db
      .prepare(
        "SELECT * FROM auth_sessions WHERE username = ? AND revoked = 0 ORDER BY last_used_at DESC",
      )
      .all(username) as Array<Record<string, unknown>>;
    return rows.map((r) => this.mapSession(r));
  }

  cleanup(): number {
    const result = this.db
      .prepare(
        "DELETE FROM auth_sessions WHERE revoked = 1 OR expires_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
      )
      .run();
    return result.changes;
  }

  private mapSession(row: Record<string, unknown>): AuthSession {
    return {
      id: row["id"] as string,
      username: row["username"] as string,
      role: row["role"] as string,
      refreshTokenHash: row["refresh_token_hash"] as string,
      createdAt: row["created_at"] as string,
      lastUsedAt: row["last_used_at"] as string,
      expiresAt: row["expires_at"] as string,
      revoked: (row["revoked"] as number) === 1,
      ipAddress: row["ip_address"] as string | null,
      userAgent: row["user_agent"] as string | null,
    };
  }
}
