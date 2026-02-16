// SQLite session store — better-sqlite3, WAL mode
import Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import { SessionError } from "../koina/errors.js";
import { generateId, generateSessionKey } from "../koina/crypto.js";
import { DDL, SCHEMA_VERSION, MIGRATIONS } from "./schema.js";

const log = createLogger("mneme");

export interface Session {
  id: string;
  nousId: string;
  sessionKey: string;
  parentSessionId: string | null;
  status: "active" | "archived" | "distilled";
  model: string | null;
  tokenCountEstimate: number;
  messageCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface Message {
  id: number;
  sessionId: string;
  seq: number;
  role: "system" | "user" | "assistant" | "tool_result";
  content: string;
  toolCallId: string | null;
  toolName: string | null;
  tokenEstimate: number;
  isDistilled: boolean;
  createdAt: string;
}

export interface UsageRecord {
  sessionId: string;
  turnSeq: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  model: string | null;
}

export class SessionStore {
  private db: Database.Database;

  constructor(dbPath: string) {
    log.info(`Opening session store at ${dbPath}`);
    this.db = new Database(dbPath);
    this.db.pragma("journal_mode = WAL");
    this.db.pragma("synchronous = NORMAL");
    this.db.pragma("foreign_keys = ON");
    this.init();
  }

  private init(): void {
    const version = this.getSchemaVersion();

    if (version === 0) {
      log.info(`Initializing schema v${SCHEMA_VERSION} (fresh database)`);
      this.db.exec(DDL);
      this.db
        .prepare(
          "INSERT OR REPLACE INTO schema_version (version) VALUES (?)",
        )
        .run(SCHEMA_VERSION);

      // Apply any migrations beyond the base DDL version
      for (const m of MIGRATIONS) {
        if (m.version > SCHEMA_VERSION) {
          this.applyMigration(m.version, m.sql);
        }
      }
    } else {
      // Incremental migrations for existing databases
      const pending = MIGRATIONS.filter((m) => m.version > version).sort(
        (a, b) => a.version - b.version,
      );
      for (const m of pending) {
        this.applyMigration(m.version, m.sql);
      }
    }
  }

  private applyMigration(version: number, sql: string): void {
    log.info(`Applying migration v${version}`);
    const migrate = this.db.transaction(() => {
      this.db.exec(sql);
      this.db
        .prepare(
          "INSERT OR REPLACE INTO schema_version (version) VALUES (?)",
        )
        .run(version);
    });
    migrate();
  }

  private getSchemaVersion(): number {
    try {
      const row = this.db
        .prepare(
          "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
        )
        .get() as { version: number } | undefined;
      return row?.version ?? 0;
    } catch {
      return 0;
    }
  }

  findSession(nousId: string, sessionKey: string): Session | null {
    const row = this.db
      .prepare(
        "SELECT * FROM sessions WHERE nous_id = ? AND session_key = ? AND status = 'active'",
      )
      .get(nousId, sessionKey) as Record<string, unknown> | undefined;
    return row ? this.mapSession(row) : null;
  }

  createSession(
    nousId: string,
    sessionKey?: string,
    parentSessionId?: string,
    model?: string,
  ): Session {
    const id = generateId("ses");
    const key = sessionKey ?? generateSessionKey();
    this.db
      .prepare(
        `INSERT INTO sessions (id, nous_id, session_key, parent_session_id, model)
         VALUES (?, ?, ?, ?, ?)`,
      )
      .run(id, nousId, key, parentSessionId ?? null, model ?? null);
    const session = this.findSessionById(id);
    if (!session) throw new SessionError("Failed to create session");
    log.info(`Created session ${id} for nous ${nousId} (key: ${key})`);
    return session;
  }

  findOrCreateSession(
    nousId: string,
    sessionKey: string,
    model?: string,
    parentSessionId?: string,
  ): Session {
    return (
      this.findSession(nousId, sessionKey) ??
      this.createSession(nousId, sessionKey, parentSessionId, model)
    );
  }

  findSessionsByKey(sessionKey: string): Session[] {
    const rows = this.db
      .prepare(
        "SELECT * FROM sessions WHERE session_key = ? AND status = 'active' ORDER BY updated_at DESC",
      )
      .all(sessionKey) as Record<string, unknown>[];
    return rows.map((r) => this.mapSession(r));
  }

  findSessionById(id: string): Session | null {
    const row = this.db
      .prepare("SELECT * FROM sessions WHERE id = ?")
      .get(id) as Record<string, unknown> | undefined;
    return row ? this.mapSession(row) : null;
  }

  listSessions(nousId?: string): Session[] {
    const query = nousId
      ? "SELECT * FROM sessions WHERE nous_id = ? ORDER BY updated_at DESC"
      : "SELECT * FROM sessions ORDER BY updated_at DESC";
    const rows = (
      nousId
        ? this.db.prepare(query).all(nousId)
        : this.db.prepare(query).all()
    ) as Record<string, unknown>[];
    return rows.map((r) => this.mapSession(r));
  }

  appendMessage(
    sessionId: string,
    role: Message["role"],
    content: string,
    opts?: {
      toolCallId?: string;
      toolName?: string;
      tokenEstimate?: number;
      isDistilled?: boolean;
    },
  ): number {
    // Atomic: SELECT + INSERT + UPDATE in a single transaction
    const tokenEstimate = opts?.tokenEstimate ?? 0;
    const appendTx = this.db.transaction(() => {
      const nextSeq = this.db
        .prepare(
          "SELECT COALESCE(MAX(seq), 0) + 1 AS next FROM messages WHERE session_id = ?",
        )
        .get(sessionId) as { next: number };

      this.db
        .prepare(
          `INSERT INTO messages (session_id, seq, role, content, tool_call_id, tool_name, token_estimate, is_distilled)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
        )
        .run(
          sessionId,
          nextSeq.next,
          role,
          content,
          opts?.toolCallId ?? null,
          opts?.toolName ?? null,
          tokenEstimate,
          opts?.isDistilled ? 1 : 0,
        );

      this.db
        .prepare(
          `UPDATE sessions
           SET message_count = message_count + 1,
               token_count_estimate = token_count_estimate + ?,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE id = ?`,
        )
        .run(tokenEstimate, sessionId);

      return nextSeq.next;
    });

    return appendTx();
  }

  getHistory(
    sessionId: string,
    opts?: { limit?: number; excludeDistilled?: boolean },
  ): Message[] {
    let query = "SELECT * FROM messages WHERE session_id = ?";
    const params: (string | number)[] = [sessionId];

    if (opts?.excludeDistilled) query += " AND is_distilled = 0";
    query += " ORDER BY seq ASC";
    if (opts?.limit && opts.limit > 0) {
      query += " LIMIT ?";
      params.push(opts.limit);
    }

    const rows = this.db.prepare(query).all(...params) as Record<
      string,
      unknown
    >[];
    return rows.map((r) => this.mapMessage(r));
  }

  getHistoryWithBudget(
    sessionId: string,
    maxTokens: number,
  ): Message[] {
    // Exclude messages that have been distilled (summarized) — the summary replaces them
    const all = this.getHistory(sessionId, { excludeDistilled: true });
    let total = 0;
    const result: Message[] = [];
    for (let i = all.length - 1; i >= 0; i--) {
      const msg = all[i]!;
      if (total + msg.tokenEstimate > maxTokens && result.length > 0) {
        break;
      }
      total += msg.tokenEstimate;
      result.unshift(msg);
    }
    return result;
  }

  recordUsage(record: UsageRecord): void {
    this.db
      .prepare(
        `INSERT INTO usage (session_id, turn_seq, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, model)
         VALUES (?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        record.sessionId,
        record.turnSeq,
        record.inputTokens,
        record.outputTokens,
        record.cacheReadTokens,
        record.cacheWriteTokens,
        record.model,
      );
  }

  markMessagesDistilled(sessionId: string, seqs: number[]): void {
    if (seqs.length === 0) return;

    const placeholders = seqs.map(() => "?").join(",");
    const tx = this.db.transaction(() => {
      this.db
        .prepare(
          `UPDATE messages SET is_distilled = 1
           WHERE session_id = ? AND seq IN (${placeholders})`,
        )
        .run(sessionId, ...seqs);

      // Recalculate token estimate from undistilled messages only
      const row = this.db
        .prepare(
          `SELECT COALESCE(SUM(token_estimate), 0) AS total,
                  COUNT(*) AS msg_count
           FROM messages
           WHERE session_id = ? AND is_distilled = 0`,
        )
        .get(sessionId) as Record<string, number>;

      this.db
        .prepare(
          `UPDATE sessions
           SET token_count_estimate = ?,
               message_count = ?,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE id = ?`,
        )
        .run(row.total, row.msg_count, sessionId);

      return row;
    });

    const row = tx();
    log.info(
      `Distilled ${seqs.length} messages in session ${sessionId}, ` +
      `token estimate recalculated: ${row.total} tokens, ${row.msg_count} messages`,
    );
  }

  recordDistillation(record: {
    sessionId: string;
    messagesBefore: number;
    messagesAfter: number;
    tokensBefore: number;
    tokensAfter: number;
    factsExtracted: number;
    model: string;
  }): void {
    this.db
      .prepare(
        `INSERT INTO distillations
         (session_id, messages_before, messages_after, tokens_before, tokens_after, facts_extracted, model)
         VALUES (?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        record.sessionId,
        record.messagesBefore,
        record.messagesAfter,
        record.tokensBefore,
        record.tokensAfter,
        record.factsExtracted,
        record.model,
      );
    log.info(
      `Distillation recorded: ${record.messagesBefore}→${record.messagesAfter} msgs, ${record.tokensBefore}→${record.tokensAfter} tokens`,
    );
  }

  archiveSession(sessionId: string): void {
    this.db
      .prepare("UPDATE sessions SET status = 'archived' WHERE id = ?")
      .run(sessionId);
  }

  recordCrossAgentCall(record: {
    sourceSessionId: string;
    sourceNousId?: string;
    targetNousId: string;
    targetSessionId?: string;
    kind: "send" | "ask" | "spawn";
    content: string;
  }): number {
    const result = this.db
      .prepare(
        `INSERT INTO cross_agent_messages (source_session_id, source_nous_id, target_nous_id, target_session_id, kind, content, status)
         VALUES (?, ?, ?, ?, ?, ?, 'pending')`,
      )
      .run(
        record.sourceSessionId,
        record.sourceNousId ?? null,
        record.targetNousId,
        record.targetSessionId ?? null,
        record.kind,
        record.content,
      );
    return Number(result.lastInsertRowid);
  }

  getUnsurfacedMessages(nousId: string): Array<{
    id: number;
    sourceNousId: string | null;
    content: string;
    response: string | null;
    kind: string;
    createdAt: string;
  }> {
    return this.db
      .prepare(
        `SELECT id, source_nous_id, content, response, kind, created_at
         FROM cross_agent_messages
         WHERE target_nous_id = ? AND surfaced_in_session IS NULL
           AND status IN ('delivered', 'responded')
         ORDER BY created_at ASC`,
      )
      .all(nousId)
      .map((row: Record<string, unknown>) => ({
        id: row.id as number,
        sourceNousId: row.source_nous_id as string | null,
        content: row.content as string,
        response: row.response as string | null,
        kind: row.kind as string,
        createdAt: row.created_at as string,
      }));
  }

  markMessagesSurfaced(ids: number[], sessionId: string): void {
    if (ids.length === 0) return;
    const placeholders = ids.map(() => "?").join(",");
    this.db
      .prepare(
        `UPDATE cross_agent_messages SET surfaced_in_session = ?
         WHERE id IN (${placeholders})`,
      )
      .run(sessionId, ...ids);
  }

  updateCrossAgentCall(
    id: number,
    update: {
      targetSessionId?: string;
      status: "delivered" | "responded" | "timeout" | "error";
      response?: string;
    },
  ): void {
    const parts: string[] = ["status = ?"];
    const params: (string | number | null)[] = [update.status];

    if (update.targetSessionId) {
      parts.push("target_session_id = ?");
      params.push(update.targetSessionId);
    }
    if (update.response) {
      parts.push("response = ?");
      params.push(update.response.slice(0, 2000));
    }
    if (update.status === "responded") {
      parts.push("responded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
    }

    params.push(id);
    this.db
      .prepare(`UPDATE cross_agent_messages SET ${parts.join(", ")} WHERE id = ?`)
      .run(...params);
  }

  archiveStaleSpawnSessions(maxAgeMs: number = 24 * 60 * 60 * 1000): number {
    const cutoff = new Date(Date.now() - maxAgeMs).toISOString();
    const result = this.db
      .prepare(
        `UPDATE sessions SET status = 'archived'
         WHERE status = 'active'
           AND session_key LIKE 'spawn:%'
           AND updated_at < ?`,
      )
      .run(cutoff);
    const count = result.changes;
    if (count > 0) {
      log.info(`Archived ${count} stale spawn sessions (older than ${Math.round(maxAgeMs / 3600000)}h)`);
    }
    return count;
  }

  rebuildRoutingCache(
    bindings: Array<{
      channel: string;
      peerKind?: string;
      peerId?: string;
      accountId?: string;
      nousId: string;
      priority?: number;
    }>,
  ): void {
    const tx = this.db.transaction(() => {
      this.db.prepare("DELETE FROM routing_cache").run();
      const insert = this.db.prepare(
        `INSERT INTO routing_cache (channel, peer_kind, peer_id, account_id, nous_id, priority)
         VALUES (?, ?, ?, ?, ?, ?)`,
      );
      for (const b of bindings) {
        insert.run(
          b.channel,
          b.peerKind ?? null,
          b.peerId ?? null,
          b.accountId ?? null,
          b.nousId,
          b.priority ?? 0,
        );
      }
    });
    tx();
    log.info(`Rebuilt routing cache with ${bindings.length} entries`);
  }

  resolveRoute(
    channel: string,
    peerKind?: string,
    peerId?: string,
    accountId?: string,
  ): string | null {
    const row = this.db
      .prepare(
        `SELECT nous_id FROM routing_cache
         WHERE channel = ?
           AND (peer_kind IS NULL OR peer_kind = ?)
           AND (peer_id IS NULL OR peer_id = ?)
           AND (account_id IS NULL OR account_id = ?)
         ORDER BY priority DESC, peer_id DESC
         LIMIT 1`,
      )
      .get(
        channel,
        peerKind ?? null,
        peerId ?? null,
        accountId ?? null,
      ) as { nous_id: string } | undefined;
    return row?.nous_id ?? null;
  }

  getMetrics(): {
    perNous: Record<string, {
      activeSessions: number;
      totalMessages: number;
      totalTokens: number;
      lastActivity: string | null;
    }>;
    usage: {
      totalInputTokens: number;
      totalOutputTokens: number;
      totalCacheReadTokens: number;
      totalCacheWriteTokens: number;
      turnCount: number;
    };
    usageByNous: Record<string, {
      inputTokens: number;
      outputTokens: number;
      cacheReadTokens: number;
      cacheWriteTokens: number;
      turns: number;
    }>;
  } {
    const perNous: Record<string, {
      activeSessions: number;
      totalMessages: number;
      totalTokens: number;
      lastActivity: string | null;
    }> = {};

    const sessionRows = this.db
      .prepare(
        `SELECT nous_id,
                COUNT(*) AS active_sessions,
                SUM(message_count) AS total_messages,
                SUM(token_count_estimate) AS total_tokens,
                MAX(updated_at) AS last_activity
         FROM sessions
         WHERE status = 'active'
         GROUP BY nous_id`,
      )
      .all() as Array<Record<string, unknown>>;

    for (const row of sessionRows) {
      perNous[row.nous_id as string] = {
        activeSessions: row.active_sessions as number,
        totalMessages: row.total_messages as number,
        totalTokens: row.total_tokens as number,
        lastActivity: row.last_activity as string | null,
      };
    }

    const usageRow = this.db
      .prepare(
        `SELECT COALESCE(SUM(input_tokens), 0) AS input,
                COALESCE(SUM(output_tokens), 0) AS output,
                COALESCE(SUM(cache_read_tokens), 0) AS cache_read,
                COALESCE(SUM(cache_write_tokens), 0) AS cache_write,
                COUNT(*) AS turns
         FROM usage`,
      )
      .get() as Record<string, number>;

    const usageByNous: Record<string, {
      inputTokens: number;
      outputTokens: number;
      cacheReadTokens: number;
      cacheWriteTokens: number;
      turns: number;
    }> = {};

    const nousUsageRows = this.db
      .prepare(
        `SELECT s.nous_id,
                COALESCE(SUM(u.input_tokens), 0) AS input,
                COALESCE(SUM(u.output_tokens), 0) AS output,
                COALESCE(SUM(u.cache_read_tokens), 0) AS cache_read,
                COALESCE(SUM(u.cache_write_tokens), 0) AS cache_write,
                COUNT(*) AS turns
         FROM usage u
         JOIN sessions s ON u.session_id = s.id
         GROUP BY s.nous_id`,
      )
      .all() as Array<Record<string, unknown>>;

    for (const row of nousUsageRows) {
      usageByNous[row.nous_id as string] = {
        inputTokens: row.input as number,
        outputTokens: row.output as number,
        cacheReadTokens: row.cache_read as number,
        cacheWriteTokens: row.cache_write as number,
        turns: row.turns as number,
      };
    }

    return {
      perNous,
      usage: {
        totalInputTokens: usageRow.input,
        totalOutputTokens: usageRow.output,
        totalCacheReadTokens: usageRow.cache_read,
        totalCacheWriteTokens: usageRow.cache_write,
        turnCount: usageRow.turns,
      },
      usageByNous,
    };
  }

  close(): void {
    this.db.close();
    log.info("Session store closed");
  }

  private mapSession(row: Record<string, unknown>): Session {
    return {
      id: row.id as string,
      nousId: row.nous_id as string,
      sessionKey: row.session_key as string,
      parentSessionId: row.parent_session_id as string | null,
      status: row.status as Session["status"],
      model: row.model as string | null,
      tokenCountEstimate: row.token_count_estimate as number,
      messageCount: row.message_count as number,
      createdAt: row.created_at as string,
      updatedAt: row.updated_at as string,
    };
  }

  private mapMessage(row: Record<string, unknown>): Message {
    return {
      id: row.id as number,
      sessionId: row.session_id as string,
      seq: row.seq as number,
      role: row.role as Message["role"],
      content: row.content as string,
      toolCallId: row.tool_call_id as string | null,
      toolName: row.tool_name as string | null,
      tokenEstimate: row.token_estimate as number,
      isDistilled: (row.is_distilled as number) === 1,
      createdAt: row.created_at as string,
    };
  }

  // --- Contact Management ---

  isApprovedContact(sender: string, channel: string, accountId?: string): boolean {
    const row = this.db
      .prepare(
        "SELECT 1 FROM approved_contacts WHERE sender = ? AND channel = ? AND (account_id = ? OR account_id IS NULL) LIMIT 1",
      )
      .get(sender, channel, accountId ?? null) as unknown;
    return !!row;
  }

  createContactRequest(
    sender: string,
    senderName: string,
    channel: string,
    accountId?: string,
  ): { id: number; challengeCode: string } {
    const code = String(Math.floor(1000 + Math.random() * 9000));

    // Upsert — if already pending, update the code
    this.db
      .prepare(
        `INSERT INTO contact_requests (sender, sender_name, channel, account_id, challenge_code)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(sender, channel, account_id)
         DO UPDATE SET challenge_code = ?, status = 'pending', resolved_at = NULL`,
      )
      .run(sender, senderName, channel, accountId ?? null, code, code);

    const row = this.db
      .prepare("SELECT id FROM contact_requests WHERE sender = ? AND channel = ? AND account_id IS ?")
      .get(sender, channel, accountId ?? null) as { id: number };

    return { id: row.id, challengeCode: code };
  }

  approveContactByCode(code: string): { sender: string; channel: string } | null {
    const row = this.db
      .prepare(
        "SELECT id, sender, channel, account_id FROM contact_requests WHERE challenge_code = ? AND status = 'pending'",
      )
      .get(code) as { id: number; sender: string; channel: string; account_id: string | null } | undefined;

    if (!row) return null;

    const txn = this.db.transaction(() => {
      this.db
        .prepare("UPDATE contact_requests SET status = 'approved', resolved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .run(row.id);

      this.db
        .prepare(
          `INSERT OR IGNORE INTO approved_contacts (sender, channel, account_id) VALUES (?, ?, ?)`,
        )
        .run(row.sender, row.channel, row.account_id);
    });
    txn();

    return { sender: row.sender, channel: row.channel };
  }

  denyContactByCode(code: string): boolean {
    const result = this.db
      .prepare("UPDATE contact_requests SET status = 'denied', resolved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE challenge_code = ? AND status = 'pending'")
      .run(code);
    return result.changes > 0;
  }

  getPendingRequests(): Array<{ id: number; sender: string; senderName: string; channel: string; code: string; createdAt: string }> {
    const rows = this.db
      .prepare("SELECT id, sender, sender_name, channel, challenge_code, created_at FROM contact_requests WHERE status = 'pending' ORDER BY created_at DESC")
      .all() as Array<Record<string, unknown>>;
    return rows.map((r) => ({
      id: r.id as number,
      sender: r.sender as string,
      senderName: r.sender_name as string,
      channel: r.channel as string,
      code: r.challenge_code as string,
      createdAt: r.created_at as string,
    }));
  }
}
