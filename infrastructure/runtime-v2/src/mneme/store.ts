// SQLite session store — better-sqlite3, WAL mode
import Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import { SessionError } from "../koina/errors.js";
import { generateId, generateSessionKey } from "../koina/crypto.js";
import { DDL, SCHEMA_VERSION } from "./schema.js";

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
    if (version < SCHEMA_VERSION) {
      log.info(`Initializing schema v${SCHEMA_VERSION} (was v${version})`);
      this.db.exec(DDL);
      this.db
        .prepare(
          "INSERT OR REPLACE INTO schema_version (version) VALUES (?)",
        )
        .run(SCHEMA_VERSION);
    }
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
  ): Session {
    return (
      this.findSession(nousId, sessionKey) ??
      this.createSession(nousId, sessionKey, undefined, model)
    );
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
        opts?.tokenEstimate ?? 0,
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
      .run(opts?.tokenEstimate ?? 0, sessionId);

    return nextSeq.next;
  }

  getHistory(
    sessionId: string,
    opts?: { limit?: number; excludeDistilled?: boolean },
  ): Message[] {
    let query = "SELECT * FROM messages WHERE session_id = ?";
    if (opts?.excludeDistilled) query += " AND is_distilled = 0";
    query += " ORDER BY seq ASC";
    if (opts?.limit) query += ` LIMIT ${opts.limit}`;

    const rows = this.db.prepare(query).all(sessionId) as Record<
      string,
      unknown
    >[];
    return rows.map((r) => this.mapMessage(r));
  }

  getHistoryWithBudget(
    sessionId: string,
    maxTokens: number,
  ): Message[] {
    const all = this.getHistory(sessionId);
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
    this.db
      .prepare(
        `UPDATE messages SET is_distilled = 1
         WHERE session_id = ? AND seq IN (${placeholders})`,
      )
      .run(sessionId, ...seqs);
    log.debug(`Marked ${seqs.length} messages as distilled in session ${sessionId}`);
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
}
