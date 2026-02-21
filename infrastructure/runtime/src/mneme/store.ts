// SQLite session store — better-sqlite3, WAL mode
import Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import { SessionError } from "../koina/errors.js";
import { generateId, generateSessionKey } from "../koina/crypto.js";
import { DDL, MIGRATIONS, SCHEMA_VERSION } from "./schema.js";

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
  lastInputTokens: number;
  bootstrapHash: string | null;
  distillationCount: number;
  sessionType: "primary" | "background" | "ephemeral";
  lastDistilledAt: string | null;
  computedContextTokens: number;
  workingState: WorkingState | null;
  distillationPriming: DistillationPriming | null;
  createdAt: string;
  updatedAt: string;
}

export interface WorkingState {
  currentTask: string;
  completedSteps: string[];
  nextSteps: string[];
  recentDecisions: string[];
  openFiles: string[];
  updatedAt: string;
}

export interface DistillationPriming {
  facts: string[];
  decisions: string[];
  openItems: string[];
  summary: string;
  distillationNumber: number;
  distilledAt: string;
}

export interface QueuedMessage {
  id: number;
  sessionId: string;
  content: string;
  sender: string | null;
  createdAt: string;
}

export interface AgentNote {
  id: number;
  sessionId: string;
  nousId: string;
  category: "task" | "decision" | "preference" | "correction" | "context";
  content: string;
  createdAt: string;
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

export interface ReflectionLog {
  id: number;
  nousId: string;
  reflectedAt: string;
  sessionsReviewed: number;
  messagesReviewed: number;
  patternsFound: number;
  contradictionsFound: number;
  correctionsFound: number;
  preferencesFound: number;
  relationshipsFound: number;
  unresolvedThreadsFound: number;
  memoriesStored: number;
  tokensUsed: number;
  durationMs: number;
  model: string | null;
  findings: ReflectionFindings;
  errors: string | null;
}

export interface ReflectionFindings {
  patterns: string[];
  contradictions: string[];
  corrections: string[];
  preferences: string[];
  relationships: string[];
  unresolvedThreads: string[];
}

export interface Thread {
  id: string;
  nousId: string;
  identity: string;
  createdAt: string;
  updatedAt: string;
}

export interface TransportBinding {
  id: string;
  threadId: string;
  transport: string;
  channelKey: string;
  lastSeenAt: string;
}

export interface ThreadSummary {
  threadId: string;
  summary: string;
  keyFacts: string[];
  updatedAt: string;
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

    // Auto-classify session type from key pattern
    let sessionType: Session["sessionType"] = "primary";
    if (key.includes("prosoche")) sessionType = "background";
    else if (key.startsWith("ask:") || key.startsWith("spawn:") || key.startsWith("dispatch:") || key.startsWith("ephemeral:")) sessionType = "ephemeral";

    this.db
      .prepare(
        `INSERT INTO sessions (id, nous_id, session_key, parent_session_id, model, session_type)
         VALUES (?, ?, ?, ?, ?, ?)`,
      )
      .run(id, nousId, key, parentSessionId ?? null, model ?? null, sessionType);
    const session = this.findSessionById(id);
    if (!session) throw new SessionError("Failed to create session", {
      code: "SESSION_CORRUPTED", context: { sessionId: id, nousId },
    });
    log.info(`Created session ${id} for nous ${nousId} (key: ${key}, type: ${sessionType})`);
    return session;
  }

  findOrCreateSession(
    nousId: string,
    sessionKey: string,
    model?: string,
    parentSessionId?: string,
  ): Session {
    const active = this.findSession(nousId, sessionKey);
    if (active) return active;

    // Check for archived/distilled session with same key — reactivate instead of
    // creating a duplicate (UNIQUE constraint on nous_id + session_key spans all statuses)
    const archived = this.db
      .prepare(
        "SELECT * FROM sessions WHERE nous_id = ? AND session_key = ? AND status != 'active' ORDER BY updated_at DESC LIMIT 1",
      )
      .get(nousId, sessionKey) as Record<string, unknown> | undefined;

    if (archived) {
      const id = archived["id"] as string;
      this.db
        .prepare("UPDATE sessions SET status = 'active', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .run(id);
      log.info(`Reactivated archived session ${id} for nous ${nousId} (key: ${sessionKey})`);
      return this.findSessionById(id)!;
    }

    return this.createSession(nousId, sessionKey, parentSessionId, model);
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
    const params: (string | number)[] = [sessionId];
    let where = "session_id = ?";
    if (opts?.excludeDistilled) where += " AND is_distilled = 0";

    let query: string;
    if (opts?.limit && opts.limit > 0) {
      // Return the N most recent messages in chronological order
      query = `SELECT * FROM (SELECT * FROM messages WHERE ${where} ORDER BY seq DESC LIMIT ?) ORDER BY seq ASC`;
      params.push(opts.limit);
    } else {
      query = `SELECT * FROM messages WHERE ${where} ORDER BY seq ASC`;
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

  getRecentToolCalls(sessionId: string, limit = 10): string[] {
    const rows = this.db
      .prepare(
        "SELECT DISTINCT tool_name FROM messages WHERE session_id = ? AND tool_name IS NOT NULL AND is_distilled = 0 ORDER BY seq DESC LIMIT ?",
      )
      .all(sessionId, limit) as Array<{ tool_name: string }>;
    return rows.map((r) => r.tool_name);
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

      const total = row['total'] as number;
      const msgCount = row['msg_count'] as number;

      this.db
        .prepare(
          `UPDATE sessions
           SET token_count_estimate = ?,
               message_count = ?,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE id = ?`,
        )
        .run(total, msgCount, sessionId);

      return { total, msgCount };
    });

    const row = tx();
    log.info(
      `Distilled ${seqs.length} messages in session ${sessionId}, ` +
      `token estimate recalculated: ${row.total} tokens, ${row.msgCount} messages`,
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

  updateSessionActualTokens(sessionId: string, inputTokens: number): void {
    this.db
      .prepare(
        `UPDATE sessions SET last_input_tokens = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
      )
      .run(inputTokens, sessionId);
  }

  updateSessionType(sessionId: string, sessionType: Session["sessionType"]): void {
    this.db
      .prepare("UPDATE sessions SET session_type = ? WHERE id = ?")
      .run(sessionType, sessionId);
  }

  updateLastDistilledAt(sessionId: string): void {
    this.db
      .prepare("UPDATE sessions SET last_distilled_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
      .run(sessionId);
  }

  updateComputedContextTokens(sessionId: string, tokens: number): void {
    this.db
      .prepare("UPDATE sessions SET computed_context_tokens = ? WHERE id = ?")
      .run(tokens, sessionId);
  }

  recordDistillationLog(record: {
    sessionId: string;
    nousId: string;
    messagesBefore: number;
    messagesAfter: number;
    tokensBefore: number;
    tokensAfter: number;
    factsExtracted: number;
    decisionsExtracted: number;
    openItemsExtracted: number;
    flushSucceeded: boolean;
    errors?: string;
    distillationNumber: number;
  }): void {
    this.db
      .prepare(
        `INSERT INTO distillation_log
         (session_id, nous_id, messages_before, messages_after, tokens_before, tokens_after,
          facts_extracted, decisions_extracted, open_items_extracted, flush_succeeded, errors, distillation_number)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        record.sessionId, record.nousId,
        record.messagesBefore, record.messagesAfter,
        record.tokensBefore, record.tokensAfter,
        record.factsExtracted, record.decisionsExtracted, record.openItemsExtracted,
        record.flushSucceeded ? 1 : 0, record.errors ?? null, record.distillationNumber,
      );
  }

  getDistillationLog(sessionId: string): Array<{
    id: number; sessionId: string; nousId: string; distilledAt: string;
    messagesBefore: number; messagesAfter: number; tokensBefore: number; tokensAfter: number;
    factsExtracted: number; decisionsExtracted: number; openItemsExtracted: number;
    flushSucceeded: boolean; errors: string | null; distillationNumber: number;
  }> {
    const rows = this.db
      .prepare("SELECT * FROM distillation_log WHERE session_id = ? ORDER BY id DESC")
      .all(sessionId) as Array<Record<string, unknown>>;
    return rows.map((r) => ({
      id: r['id'] as number,
      sessionId: r['session_id'] as string,
      nousId: r['nous_id'] as string,
      distilledAt: r['distilled_at'] as string,
      messagesBefore: r['messages_before'] as number,
      messagesAfter: r['messages_after'] as number,
      tokensBefore: r['tokens_before'] as number,
      tokensAfter: r['tokens_after'] as number,
      factsExtracted: r['facts_extracted'] as number,
      decisionsExtracted: r['decisions_extracted'] as number,
      openItemsExtracted: r['open_items_extracted'] as number,
      flushSucceeded: (r['flush_succeeded'] as number) === 1,
      errors: r['errors'] as string | null,
      distillationNumber: r['distillation_number'] as number,
    }));
  }

  logSubAgentCall(record: {
    sessionId: string;
    parentSessionId: string;
    parentNousId: string;
    role?: string;
    agentId: string;
    task: string;
    model?: string;
    inputTokens: number;
    outputTokens: number;
    toolCalls: number;
    status: string;
    error?: string;
    durationMs: number;
  }): void {
    this.db
      .prepare(
        `INSERT INTO sub_agent_log
         (session_id, parent_session_id, parent_nous_id, role, agent_id, task, model,
          input_tokens, output_tokens, total_cost_tokens, tool_calls, status, error, duration_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        record.sessionId, record.parentSessionId, record.parentNousId,
        record.role ?? null, record.agentId, record.task.slice(0, 2000),
        record.model ?? null, record.inputTokens, record.outputTokens,
        record.inputTokens + record.outputTokens, record.toolCalls,
        record.status, record.error ?? null, record.durationMs,
      );
  }

  getSubAgentLog(parentSessionId: string): Array<{
    id: number; sessionId: string; parentNousId: string; role: string | null;
    agentId: string; task: string; model: string | null;
    inputTokens: number; outputTokens: number; totalCostTokens: number;
    toolCalls: number; status: string; error: string | null; durationMs: number;
    createdAt: string;
  }> {
    const rows = this.db
      .prepare("SELECT * FROM sub_agent_log WHERE parent_session_id = ? ORDER BY id DESC")
      .all(parentSessionId) as Array<Record<string, unknown>>;
    return rows.map((r) => ({
      id: r['id'] as number,
      sessionId: r['session_id'] as string,
      parentNousId: r['parent_nous_id'] as string,
      role: r['role'] as string | null,
      agentId: r['agent_id'] as string,
      task: r['task'] as string,
      model: r['model'] as string | null,
      inputTokens: r['input_tokens'] as number,
      outputTokens: r['output_tokens'] as number,
      totalCostTokens: r['total_cost_tokens'] as number,
      toolCalls: r['tool_calls'] as number,
      status: r['status'] as string,
      error: r['error'] as string | null,
      durationMs: r['duration_ms'] as number,
      createdAt: r['created_at'] as string,
    }));
  }

  recordToolStat(record: {
    nousId: string;
    toolName: string;
    success: boolean;
    errorMessage?: string;
    durationMs?: number;
  }): void {
    this.db
      .prepare(
        `INSERT INTO tool_stats (nous_id, tool_name, success, error_message, duration_ms)
         VALUES (?, ?, ?, ?, ?)`,
      )
      .run(
        record.nousId,
        record.toolName,
        record.success ? 1 : 0,
        record.errorMessage ?? null,
        record.durationMs ?? null,
      );
  }

  getToolStats(opts: {
    nousId?: string;
    windowHours?: number;
  } = {}): Array<{
    toolName: string;
    totalCalls: number;
    successCount: number;
    failureCount: number;
    failureRate: number;
    avgDurationMs: number;
  }> {
    const windowHours = opts.windowHours ?? 168; // 7 days
    const cutoff = new Date(Date.now() - windowHours * 3600_000).toISOString();
    const nousFilter = opts.nousId ? "AND nous_id = ?" : "";
    const params: unknown[] = [cutoff];
    if (opts.nousId) params.push(opts.nousId);

    const rows = this.db
      .prepare(
        `SELECT tool_name,
                count(*) AS total_calls,
                sum(success) AS success_count,
                count(*) - sum(success) AS failure_count,
                ROUND(1.0 - (CAST(sum(success) AS REAL) / count(*)), 3) AS failure_rate,
                ROUND(avg(duration_ms), 0) AS avg_duration_ms
         FROM tool_stats
         WHERE created_at > ? ${nousFilter}
         GROUP BY tool_name
         ORDER BY total_calls DESC`,
      )
      .all(...params) as Array<Record<string, unknown>>;

    return rows.map((r) => ({
      toolName: r['tool_name'] as string,
      totalCalls: r['total_calls'] as number,
      successCount: r['success_count'] as number,
      failureCount: r['failure_count'] as number,
      failureRate: r['failure_rate'] as number,
      avgDurationMs: r['avg_duration_ms'] as number,
    }));
  }

  updateBootstrapHash(sessionId: string, hash: string): void {
    this.db
      .prepare(
        `UPDATE sessions SET bootstrap_hash = ? WHERE id = ?`,
      )
      .run(hash, sessionId);
  }

  incrementDistillationCount(sessionId: string): number {
    this.db
      .prepare(
        `UPDATE sessions SET distillation_count = distillation_count + 1 WHERE id = ?`,
      )
      .run(sessionId);
    const row = this.db
      .prepare("SELECT distillation_count FROM sessions WHERE id = ?")
      .get(sessionId) as { distillation_count: number } | undefined;
    return row?.distillation_count ?? 0;
  }

  updateSessionModel(sessionId: string, model: string): void {
    this.db
      .prepare(
        `UPDATE sessions SET model = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
      )
      .run(model, sessionId);
  }

  getThinkingConfig(sessionId: string): { enabled: boolean; budget: number } {
    const row = this.db
      .prepare("SELECT thinking_enabled, thinking_budget FROM sessions WHERE id = ?")
      .get(sessionId) as { thinking_enabled: number; thinking_budget: number } | undefined;
    return {
      enabled: (row?.thinking_enabled ?? 0) === 1,
      budget: row?.thinking_budget ?? 10_000,
    };
  }

  setThinkingConfig(sessionId: string, enabled: boolean, budget: number): void {
    this.db
      .prepare(
        `UPDATE sessions SET thinking_enabled = ?, thinking_budget = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?`,
      )
      .run(enabled ? 1 : 0, budget, sessionId);
  }

  getLastBootstrapHash(nousId: string): string | null {
    const row = this.db
      .prepare(
        `SELECT bootstrap_hash FROM sessions
         WHERE nous_id = ? AND bootstrap_hash IS NOT NULL
         ORDER BY updated_at DESC LIMIT 1`,
      )
      .get(nousId) as { bootstrap_hash: string } | undefined;
    return row?.bootstrap_hash ?? null;
  }

  archiveSession(sessionId: string): void {
    this.db
      .prepare(
        "UPDATE sessions SET status = 'archived', session_key = session_key || ':archived:' || id WHERE id = ? AND status = 'active'",
      )
      .run(sessionId);
  }

  recordCrossAgentCall(record: {
    sourceSessionId: string;
    sourceNousId?: string;
    targetNousId: string;
    targetSessionId?: string;
    kind: "send" | "ask" | "spawn";
    content: string;
    idempotencyWindowMs?: number;
  }): number {
    const hash = this.hashCrossAgentContent(
      record.sourceNousId ?? "",
      record.targetNousId,
      record.kind,
      record.content,
    );

    // Dedup: check for identical call within window (default 5 minutes)
    const windowMs = record.idempotencyWindowMs ?? 300_000;
    const cutoff = new Date(Date.now() - windowMs).toISOString();
    const existing = this.db
      .prepare(
        `SELECT id FROM cross_agent_messages
         WHERE content_hash = ? AND created_at > ?
         LIMIT 1`,
      )
      .get(hash, cutoff) as { id: number } | undefined;

    if (existing) return existing.id;

    const result = this.db
      .prepare(
        `INSERT INTO cross_agent_messages (source_session_id, source_nous_id, target_nous_id, target_session_id, kind, content, status, content_hash)
         VALUES (?, ?, ?, ?, ?, ?, 'pending', ?)`,
      )
      .run(
        record.sourceSessionId,
        record.sourceNousId ?? null,
        record.targetNousId,
        record.targetSessionId ?? null,
        record.kind,
        record.content,
        hash,
      );
    return Number(result.lastInsertRowid);
  }

  private hashCrossAgentContent(source: string, target: string, kind: string, content: string): string {
    const input = `${source}:${target}:${kind}:${content}`;
    let h1 = 0xdeadbeef;
    let h2 = 0x41c6ce57;
    for (let i = 0; i < input.length; i++) {
      const ch = input.charCodeAt(i);
      h1 = Math.imul(h1 ^ ch, 2654435761);
      h2 = Math.imul(h2 ^ ch, 1597334677);
    }
    h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507);
    h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909);
    h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507);
    h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909);
    return (4294967296 * (2097151 & h2) + (h1 >>> 0)).toString(36);
  }

  getUnsurfacedMessages(nousId: string): Array<{
    id: number;
    sourceNousId: string | null;
    content: string;
    response: string | null;
    kind: string;
    createdAt: string;
  }> {
    const rows = this.db
      .prepare(
        `SELECT id, source_nous_id, content, response, kind, created_at
         FROM cross_agent_messages
         WHERE target_nous_id = ? AND surfaced_in_session IS NULL
           AND status IN ('delivered', 'responded')
         ORDER BY created_at ASC`,
      )
      .all(nousId) as Array<Record<string, unknown>>;
    return rows.map((row) => ({
      id: row['id'] as number,
      sourceNousId: row['source_nous_id'] as string | null,
      content: row['content'] as string,
      response: row['response'] as string | null,
      kind: row['kind'] as string,
      createdAt: row['created_at'] as string,
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

  deleteEphemeralSessions(maxAgeMs: number = 24 * 60 * 60 * 1000): number {
    const cutoff = new Date(Date.now() - maxAgeMs).toISOString();
    const tx = this.db.transaction(() => {
      const ids = this.db
        .prepare(
          `SELECT id FROM sessions
           WHERE session_type = 'ephemeral'
             AND status = 'active'
             AND updated_at < ?`,
        )
        .all(cutoff) as Array<{ id: string }>;

      if (ids.length === 0) return 0;

      const placeholders = ids.map(() => "?").join(",");
      const sessionIds = ids.map((r) => r.id);

      this.db.prepare(`DELETE FROM messages WHERE session_id IN (${placeholders})`).run(...sessionIds);
      this.db.prepare(`DELETE FROM usage WHERE session_id IN (${placeholders})`).run(...sessionIds);
      this.db.prepare(`DELETE FROM agent_notes WHERE session_id IN (${placeholders})`).run(...sessionIds);
      this.db.prepare(`DELETE FROM distillations WHERE session_id IN (${placeholders})`).run(...sessionIds);
      const result = this.db.prepare(`DELETE FROM sessions WHERE id IN (${placeholders})`).run(...sessionIds);
      return result.changes;
    });

    const count = tx();
    if (count > 0) {
      log.info(`Deleted ${count} ephemeral sessions (older than ${Math.round(maxAgeMs / 3600000)}h)`);
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
      perNous[row['nous_id'] as string] = {
        activeSessions: row['active_sessions'] as number,
        totalMessages: row['total_messages'] as number,
        totalTokens: row['total_tokens'] as number,
        lastActivity: row['last_activity'] as string | null,
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
      .get() as { input: number; output: number; cache_read: number; cache_write: number; turns: number };

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
      usageByNous[row['nous_id'] as string] = {
        inputTokens: row['input'] as number,
        outputTokens: row['output'] as number,
        cacheReadTokens: row['cache_read'] as number,
        cacheWriteTokens: row['cache_write'] as number,
        turns: row['turns'] as number,
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

  getCostsBySession(sessionId: string): Array<{
    turnSeq: number;
    inputTokens: number;
    outputTokens: number;
    cacheReadTokens: number;
    cacheWriteTokens: number;
    model: string | null;
    createdAt: string;
  }> {
    const rows = this.db
      .prepare(
        `SELECT turn_seq, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, model, created_at
         FROM usage WHERE session_id = ? ORDER BY turn_seq ASC`,
      )
      .all(sessionId) as Array<Record<string, unknown>>;
    return rows.map((row) => ({
      turnSeq: row['turn_seq'] as number,
      inputTokens: row['input_tokens'] as number,
      outputTokens: row['output_tokens'] as number,
      cacheReadTokens: row['cache_read_tokens'] as number,
      cacheWriteTokens: row['cache_write_tokens'] as number,
      model: row['model'] as string | null,
      createdAt: row['created_at'] as string,
    }));
  }

  getCostsByAgent(nousId: string): Array<{
    inputTokens: number;
    outputTokens: number;
    cacheReadTokens: number;
    cacheWriteTokens: number;
    model: string | null;
    turns: number;
  }> {
    const rows2 = this.db
      .prepare(
        `SELECT u.model,
                SUM(u.input_tokens) AS input_tokens,
                SUM(u.output_tokens) AS output_tokens,
                SUM(u.cache_read_tokens) AS cache_read_tokens,
                SUM(u.cache_write_tokens) AS cache_write_tokens,
                COUNT(*) AS turns
         FROM usage u
         JOIN sessions s ON u.session_id = s.id
         WHERE s.nous_id = ?
         GROUP BY u.model`,
      )
      .all(nousId) as Array<Record<string, unknown>>;
    return rows2.map((row) => ({
      inputTokens: row['input_tokens'] as number,
      outputTokens: row['output_tokens'] as number,
      cacheReadTokens: row['cache_read_tokens'] as number,
      cacheWriteTokens: row['cache_write_tokens'] as number,
      model: row['model'] as string | null,
      turns: row['turns'] as number,
    }));
  }

  // --- Retention / Data Lifecycle ---

  /**
   * Delete messages in fully-distilled sessions older than `days`.
   * The session rows and their metadata are preserved; only the raw message
   * content is removed to free space while keeping audit trails intact.
   * Returns count of messages deleted.
   */
  purgeDistilledMessages(days: number): number {
    if (days <= 0) return 0;
    const cutoff = new Date(Date.now() - days * 86_400_000).toISOString();
    const result = this.db
      .prepare(
        `DELETE FROM messages
         WHERE session_id IN (
           SELECT id FROM sessions
           WHERE status = 'distilled' AND updated_at < ?
         )`,
      )
      .run(cutoff);
    if (result.changes > 0) {
      log.info(`Retention: purged ${result.changes} messages from distilled sessions older than ${days}d`);
    }
    return result.changes;
  }

  /**
   * Delete messages in archived sessions older than `days`.
   * Archived sessions are ones that were manually archived (not distilled),
   * e.g. stale spawn sessions. Returns count of messages deleted.
   */
  purgeArchivedSessionMessages(days: number): number {
    if (days <= 0) return 0;
    const cutoff = new Date(Date.now() - days * 86_400_000).toISOString();
    const result = this.db
      .prepare(
        `DELETE FROM messages
         WHERE session_id IN (
           SELECT id FROM sessions
           WHERE status = 'archived' AND updated_at < ?
         )`,
      )
      .run(cutoff);
    if (result.changes > 0) {
      log.info(`Retention: purged ${result.changes} messages from archived sessions older than ${days}d`);
    }
    return result.changes;
  }

  /**
   * Truncate oversized tool results stored in the messages table.
   * Operates on `role = 'tool_result'` rows whose content exceeds maxChars.
   * Returns count of rows truncated.
   */
  truncateToolResults(maxChars: number): number {
    if (maxChars <= 0) return 0;
    const result = this.db
      .prepare(
        `UPDATE messages
         SET content = substr(content, 1, ?) || '…[truncated]'
         WHERE role = 'tool_result' AND length(content) > ?`,
      )
      .run(maxChars, maxChars);
    if (result.changes > 0) {
      log.info(`Retention: truncated ${result.changes} oversized tool results to ${maxChars} chars`);
    }
    return result.changes;
  }

  close(): void {
    this.db.close();
    log.info("Session store closed");
  }

  private mapSession(row: Record<string, unknown>): Session {
    return {
      id: row['id'] as string,
      nousId: row['nous_id'] as string,
      sessionKey: row['session_key'] as string,
      parentSessionId: row['parent_session_id'] as string | null,
      status: row['status'] as Session["status"],
      model: row['model'] as string | null,
      tokenCountEstimate: row['token_count_estimate'] as number,
      messageCount: row['message_count'] as number,
      lastInputTokens: (row['last_input_tokens'] as number) ?? 0,
      bootstrapHash: (row['bootstrap_hash'] as string) ?? null,
      distillationCount: (row['distillation_count'] as number) ?? 0,
      sessionType: (row['session_type'] as Session["sessionType"]) ?? "primary",
      lastDistilledAt: (row['last_distilled_at'] as string) ?? null,
      computedContextTokens: (row['computed_context_tokens'] as number) ?? 0,
      workingState: this.parseWorkingState(row['working_state'] as string | null),
      distillationPriming: this.parseJSON<DistillationPriming>(row['distillation_priming'] as string | null),
      createdAt: row['created_at'] as string,
      updatedAt: row['updated_at'] as string,
    };
  }

  private parseWorkingState(raw: string | null): WorkingState | null {
    if (!raw) return null;
    try {
      return JSON.parse(raw) as WorkingState;
    } catch {
      return null;
    }
  }

  private parseJSON<T>(raw: string | null): T | null {
    if (!raw) return null;
    try {
      return JSON.parse(raw) as T;
    } catch {
      return null;
    }
  }

  private mapMessage(row: Record<string, unknown>): Message {
    return {
      id: row['id'] as number,
      sessionId: row['session_id'] as string,
      seq: row['seq'] as number,
      role: row['role'] as Message["role"],
      content: row['content'] as string,
      toolCallId: row['tool_call_id'] as string | null,
      toolName: row['tool_name'] as string | null,
      tokenEstimate: row['token_estimate'] as number,
      isDistilled: (row['is_distilled'] as number) === 1,
      createdAt: row['created_at'] as string,
    };
  }

  // --- Interaction Signals ---

  recordSignal(signal: {
    sessionId: string;
    nousId: string;
    turnSeq: number;
    signal: string;
    confidence: number;
  }): void {
    try {
      this.db
        .prepare(
          "INSERT INTO interaction_signals (session_id, nous_id, turn_seq, signal, confidence) VALUES (?, ?, ?, ?, ?)",
        )
        .run(signal.sessionId, signal.nousId, signal.turnSeq, signal.signal, signal.confidence);
    } catch (err) {
      log.warn(`recordSignal failed (non-fatal): ${err instanceof Error ? err.message : err}`);
    }
  }

  getSignalHistory(nousId: string, limit = 50): Array<{
    sessionId: string;
    turnSeq: number;
    signal: string;
    confidence: number;
    createdAt: string;
  }> {
    try {
      const rows = this.db
        .prepare(
          "SELECT session_id, turn_seq, signal, confidence, created_at FROM interaction_signals WHERE nous_id = ? ORDER BY created_at DESC LIMIT ?",
        )
        .all(nousId, limit) as Array<Record<string, unknown>>;
      return rows.map((r) => ({
        sessionId: r["session_id"] as string,
        turnSeq: r["turn_seq"] as number,
        signal: r["signal"] as string,
        confidence: r["confidence"] as number,
        createdAt: r["created_at"] as string,
      }));
    } catch (err) {
      log.warn(`getSignalHistory failed (non-fatal): ${err instanceof Error ? err.message : err}`);
      return [];
    }
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
      id: r['id'] as number,
      sender: r['sender'] as string,
      senderName: r['sender_name'] as string,
      channel: r['channel'] as string,
      code: r['challenge_code'] as string,
      createdAt: r['created_at'] as string,
    }));
  }

  // --- Blackboard (cross-agent shared state) ---

  blackboardWrite(key: string, value: string, authorNousId: string, ttlSeconds = 3600): string {
    const id = generateId();
    const expiresAt = new Date(Date.now() + ttlSeconds * 1000).toISOString();

    // Upsert by key + author — each agent can update their own entries
    this.db
      .prepare(
        `INSERT INTO blackboard (id, key, value, author_nous_id, ttl_seconds, expires_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO NOTHING`,
      )
      .run(id, key, value, authorNousId, ttlSeconds, expiresAt);

    // Also delete any stale entries for same key by same author
    this.db
      .prepare(
        "DELETE FROM blackboard WHERE key = ? AND author_nous_id = ? AND id != ?",
      )
      .run(key, authorNousId, id);

    return id;
  }

  blackboardRead(key: string): Array<{ id: string; key: string; value: string; author: string; createdAt: string; expiresAt: string | null }> {
    this.blackboardExpire();
    const rows = this.db
      .prepare(
        "SELECT id, key, value, author_nous_id, created_at, expires_at FROM blackboard WHERE key = ? ORDER BY created_at DESC",
      )
      .all(key) as Array<Record<string, unknown>>;
    return rows.map((r) => ({
      id: r["id"] as string,
      key: r["key"] as string,
      value: r["value"] as string,
      author: r["author_nous_id"] as string,
      createdAt: r["created_at"] as string,
      expiresAt: r["expires_at"] as string | null,
    }));
  }

  blackboardReadPrefix(prefix: string): Array<{ id: string; key: string; value: string; author: string; createdAt: string; expiresAt: string | null }> {
    this.blackboardExpire();
    const rows = this.db
      .prepare(
        "SELECT id, key, value, author_nous_id, created_at, expires_at FROM blackboard WHERE key LIKE ? ORDER BY created_at DESC LIMIT 10",
      )
      .all(`${prefix}%`) as Array<Record<string, unknown>>;
    return rows.map((r) => ({
      id: r["id"] as string,
      key: r["key"] as string,
      value: r["value"] as string,
      author: r["author_nous_id"] as string,
      createdAt: r["created_at"] as string,
      expiresAt: r["expires_at"] as string | null,
    }));
  }

  blackboardList(): Array<{ key: string; count: number; authors: string[] }> {
    this.blackboardExpire();
    const rows = this.db
      .prepare(
        "SELECT key, COUNT(*) as cnt, GROUP_CONCAT(DISTINCT author_nous_id) as authors FROM blackboard GROUP BY key ORDER BY key",
      )
      .all() as Array<Record<string, unknown>>;
    return rows.map((r) => ({
      key: r["key"] as string,
      count: r["cnt"] as number,
      authors: (r["authors"] as string).split(","),
    }));
  }

  blackboardDelete(key: string, authorNousId: string): number {
    const result = this.db
      .prepare("DELETE FROM blackboard WHERE key = ? AND author_nous_id = ?")
      .run(key, authorNousId);
    return result.changes;
  }

  blackboardExpire(): number {
    const result = this.db
      .prepare("DELETE FROM blackboard WHERE expires_at IS NOT NULL AND expires_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now')")
      .run();
    return result.changes;
  }

  // --- Export / Analytics ---

  getUsageForSession(sessionId: string): Array<UsageRecord & { createdAt: string }> {
    const rows = this.db
      .prepare("SELECT * FROM usage WHERE session_id = ? ORDER BY turn_seq ASC")
      .all(sessionId) as Array<Record<string, unknown>>;
    return rows.map((r) => ({
      sessionId: r["session_id"] as string,
      turnSeq: r["turn_seq"] as number,
      inputTokens: r["input_tokens"] as number,
      outputTokens: r["output_tokens"] as number,
      cacheReadTokens: r["cache_read_tokens"] as number,
      cacheWriteTokens: r["cache_write_tokens"] as number,
      model: r["model"] as string | null,
      createdAt: r["created_at"] as string,
    }));
  }

  getExportStats(opts?: { nousId?: string; since?: string }): {
    totalSessions: number;
    totalMessages: number;
    userMessages: number;
    assistantMessages: number;
    toolMessages: number;
    totalDistillations: number;
    totalInputTokens: number;
    totalOutputTokens: number;
  } {
    const nousId = opts?.nousId ?? null;
    const since = opts?.since ?? null;
    const row = this.db
      .prepare(`
        SELECT
          COUNT(DISTINCT m.session_id) AS total_sessions,
          COUNT(*) AS total_messages,
          SUM(CASE WHEN m.role = 'user' THEN 1 ELSE 0 END) AS user_messages,
          SUM(CASE WHEN m.role = 'assistant' THEN 1 ELSE 0 END) AS assistant_messages,
          SUM(CASE WHEN m.role = 'tool_result' THEN 1 ELSE 0 END) AS tool_messages
        FROM messages m
        JOIN sessions s ON m.session_id = s.id
        WHERE (? IS NULL OR s.nous_id = ?)
          AND (? IS NULL OR m.created_at >= ?)
      `)
      .get(nousId, nousId, since, since) as Record<string, number>;

    const usageRow = this.db
      .prepare(`
        SELECT
          COALESCE(SUM(u.input_tokens), 0) AS total_input,
          COALESCE(SUM(u.output_tokens), 0) AS total_output
        FROM usage u
        JOIN sessions s ON u.session_id = s.id
        WHERE (? IS NULL OR s.nous_id = ?)
          AND (? IS NULL OR u.created_at >= ?)
      `)
      .get(nousId, nousId, since, since) as Record<string, number>;

    const distillRow = this.db
      .prepare(`
        SELECT COUNT(*) AS total
        FROM distillations d
        JOIN sessions s ON d.session_id = s.id
        WHERE (? IS NULL OR s.nous_id = ?)
          AND (? IS NULL OR d.created_at >= ?)
      `)
      .get(nousId, nousId, since, since) as Record<string, number>;

    return {
      totalSessions: row["total_sessions"] ?? 0,
      totalMessages: row["total_messages"] ?? 0,
      userMessages: row["user_messages"] ?? 0,
      assistantMessages: row["assistant_messages"] ?? 0,
      toolMessages: row["tool_messages"] ?? 0,
      totalDistillations: distillRow["total"] ?? 0,
      totalInputTokens: usageRow["total_input"] ?? 0,
      totalOutputTokens: usageRow["total_output"] ?? 0,
    };
  }

  listSessionsFiltered(opts?: { nousId?: string; since?: string; until?: string }): Session[] {
    const nousId = opts?.nousId ?? null;
    const since = opts?.since ?? null;
    const until = opts?.until ?? null;
    const rows = this.db
      .prepare(`
        SELECT * FROM sessions
        WHERE (? IS NULL OR nous_id = ?)
          AND (? IS NULL OR created_at >= ?)
          AND (? IS NULL OR created_at <= ?)
        ORDER BY updated_at DESC
      `)
      .all(nousId, nousId, since, since, until, until) as Array<Record<string, unknown>>;
    return rows.map((r) => this.mapSession(r));
  }

  /**
   * For a given agent, find the canonical DM session key.
   * This enables webchat to converge with Signal DM into a single session
   * rather than creating isolated parallel conversations.
   *
   * Priority: Signal DM session with most distillations (deepest context),
   * then most messages, then most recently active.
   */
  getCanonicalSessionKey(nousId: string): string | null {
    // Find DM bindings for this agent (Signal DMs, not groups)
    const row = this.db
      .prepare(
        `SELECT s.session_key
         FROM sessions s
         WHERE s.nous_id = ?
           AND s.session_key LIKE 'signal:%'
           AND s.status = 'active'
           AND s.session_key NOT IN (
             SELECT 'signal:' || rc.peer_id FROM routing_cache rc
             WHERE rc.channel = 'signal' AND rc.peer_kind = 'group'
           )
         ORDER BY s.distillation_count DESC, s.message_count DESC, s.updated_at DESC
         LIMIT 1`,
      )
      .get(nousId) as { session_key: string } | undefined;
    return row?.session_key ?? null;
  }

  // --- Thread Model (Phase 1 + 2) ---

  resolveThread(nousId: string, identity: string): Thread {
    const existing = this.db
      .prepare("SELECT * FROM threads WHERE nous_id = ? AND identity = ?")
      .get(nousId, identity) as Record<string, unknown> | undefined;
    if (existing) return this.mapThread(existing);

    const id = generateId("thr");
    this.db
      .prepare("INSERT INTO threads (id, nous_id, identity) VALUES (?, ?, ?)")
      .run(id, nousId, identity);
    log.info(`Created thread ${id} for ${identity} <-> ${nousId}`);
    return this.mapThread(
      this.db.prepare("SELECT * FROM threads WHERE id = ?").get(id) as Record<string, unknown>,
    );
  }

  resolveBinding(threadId: string, transport: string, channelKey: string): TransportBinding {
    const now = new Date().toISOString();
    this.db
      .prepare(
        `INSERT INTO transport_bindings (id, thread_id, transport, channel_key, last_seen_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(transport, channel_key)
         DO UPDATE SET last_seen_at = excluded.last_seen_at`,
      )
      .run(generateId("tbnd"), threadId, transport, channelKey, now);
    const row = this.db
      .prepare("SELECT * FROM transport_bindings WHERE transport = ? AND channel_key = ?")
      .get(transport, channelKey) as Record<string, unknown>;
    return this.mapBinding(row);
  }

  getIdentityForSignalSender(sender: string, accountId?: string): string {
    const row = this.db
      .prepare(
        `SELECT sender_name FROM contact_requests
         WHERE sender = ? AND channel = 'signal' AND (account_id = ? OR account_id IS NULL)
         AND status = 'approved'
         ORDER BY resolved_at DESC LIMIT 1`,
      )
      .get(sender, accountId ?? null) as { sender_name: string | null } | undefined;
    return row?.sender_name?.trim() || sender;
  }

  linkSessionToThread(sessionId: string, threadId: string, transport: string): void {
    this.db
      .prepare("UPDATE sessions SET thread_id = ?, transport = ? WHERE id = ?")
      .run(threadId, transport, sessionId);
  }

  migrateSessionsToThreads(): number {
    const unlinked = this.db
      .prepare(
        "SELECT id, nous_id, session_key FROM sessions WHERE thread_id IS NULL AND status != 'archived'",
      )
      .all() as Array<{ id: string; nous_id: string; session_key: string }>;

    let migrated = 0;
    for (const session of unlinked) {
      const { nous_id: nousId, session_key: sessionKey, id: sessionId } = session;
      let transport: string;
      let identity: string;
      let channelKey: string;

      if (sessionKey.startsWith("signal:")) {
        transport = "signal";
        channelKey = sessionKey;
        identity = this.getIdentityForSignalSender(sessionKey.slice("signal:".length));
      } else if (sessionKey.startsWith("cron:")) {
        transport = "cron";
        channelKey = sessionKey;
        identity = sessionKey;
      } else if (sessionKey.startsWith("spawn:")) {
        transport = "agent";
        channelKey = sessionKey;
        identity = sessionKey;
      } else {
        transport = "webchat";
        channelKey = `web:anonymous:${nousId}`;
        identity = "anonymous";
      }

      try {
        const thread = this.resolveThread(nousId, identity);
        this.resolveBinding(thread.id, transport, channelKey);
        this.linkSessionToThread(sessionId, thread.id, transport);
        migrated++;
      } catch (err) {
        log.warn(`Failed to migrate session ${sessionId} to thread: ${err instanceof Error ? err.message : err}`);
      }
    }

    if (migrated > 0) log.info(`Migrated ${migrated} sessions to thread model`);
    return migrated;
  }

  getThreadSummary(threadId: string): ThreadSummary | null {
    const row = this.db
      .prepare("SELECT * FROM thread_summaries WHERE thread_id = ?")
      .get(threadId) as Record<string, unknown> | undefined;
    if (!row) return null;
    let keyFacts: string[] = [];
    try { keyFacts = JSON.parse(row["key_facts"] as string) as string[]; } catch (err) { log.warn(`Malformed key_facts JSON in thread ${threadId}: ${err instanceof Error ? err.message : err}`); }
    return {
      threadId: row["thread_id"] as string,
      summary: row["summary"] as string,
      keyFacts,
      updatedAt: row["updated_at"] as string,
    };
  }

  updateThreadSummary(threadId: string, summary: string, keyFacts: string[]): void {
    this.db
      .prepare(
        `INSERT INTO thread_summaries (thread_id, summary, key_facts)
         VALUES (?, ?, ?)
         ON CONFLICT(thread_id)
         DO UPDATE SET summary = excluded.summary, key_facts = excluded.key_facts,
                       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`,
      )
      .run(threadId, summary, JSON.stringify(keyFacts));
  }

  getThreadForSession(sessionId: string): Thread | null {
    const row = this.db
      .prepare(
        `SELECT t.* FROM threads t
         JOIN sessions s ON s.thread_id = t.id
         WHERE s.id = ?`,
      )
      .get(sessionId) as Record<string, unknown> | undefined;
    return row ? this.mapThread(row) : null;
  }

  getSessionsByThread(threadId: string): Session[] {
    const rows = this.db
      .prepare(
        "SELECT * FROM sessions WHERE thread_id = ? ORDER BY created_at ASC",
      )
      .all(threadId) as Record<string, unknown>[];
    return rows.map((r) => this.mapSession(r));
  }

  listThreads(nousId?: string): Array<Thread & { sessionCount: number; messageCount: number; lastActivity: string | null; summary: string | null }> {
    const query = nousId
      ? `SELECT t.*,
           COUNT(DISTINCT s.id) AS session_count,
           COALESCE(SUM(s.message_count), 0) AS message_count,
           MAX(s.updated_at) AS last_activity,
           ts.summary AS summary
         FROM threads t
         LEFT JOIN sessions s ON s.thread_id = t.id
         LEFT JOIN thread_summaries ts ON ts.thread_id = t.id
         WHERE t.nous_id = ?
         GROUP BY t.id
         ORDER BY last_activity DESC`
      : `SELECT t.*,
           COUNT(DISTINCT s.id) AS session_count,
           COALESCE(SUM(s.message_count), 0) AS message_count,
           MAX(s.updated_at) AS last_activity,
           ts.summary AS summary
         FROM threads t
         LEFT JOIN sessions s ON s.thread_id = t.id
         LEFT JOIN thread_summaries ts ON ts.thread_id = t.id
         GROUP BY t.id
         ORDER BY last_activity DESC`;
    const rows = (nousId ? this.db.prepare(query).all(nousId) : this.db.prepare(query).all()) as Record<string, unknown>[];
    return rows.map((r) => ({
      ...this.mapThread(r),
      sessionCount: r["session_count"] as number,
      messageCount: r["message_count"] as number,
      lastActivity: r["last_activity"] as string | null,
      summary: r["summary"] as string | null,
    }));
  }

  getThreadHistory(
    threadId: string,
    opts?: { before?: string; limit?: number },
  ): Message[] {
    const limit = opts?.limit ?? 50;
    const params: (string | number)[] = [threadId];
    let where = "s.thread_id = ? AND m.is_distilled = 0";
    if (opts?.before) {
      where += " AND m.created_at < ?";
      params.push(opts.before);
    }
    // Return up to `limit` messages across all sessions in this thread, ordered by creation time
    const rows = this.db
      .prepare(
        `SELECT m.* FROM messages m
         JOIN sessions s ON m.session_id = s.id
         WHERE ${where}
         ORDER BY m.created_at DESC, m.seq DESC
         LIMIT ?`,
      )
      .all(...params, limit) as Record<string, unknown>[];
    // Return in chronological order
    return rows.reverse().map((r) => this.mapMessage(r));
  }

  // --- Working State ---

  getWorkingState(sessionId: string): WorkingState | null {
    const row = this.db
      .prepare("SELECT working_state FROM sessions WHERE id = ?")
      .get(sessionId) as { working_state: string | null } | undefined;
    if (!row?.working_state) return null;
    return this.parseWorkingState(row.working_state);
  }

  updateWorkingState(sessionId: string, state: WorkingState): void {
    this.db
      .prepare("UPDATE sessions SET working_state = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
      .run(JSON.stringify(state), sessionId);
  }

  clearWorkingState(sessionId: string): void {
    this.db
      .prepare("UPDATE sessions SET working_state = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
      .run(sessionId);
  }

  // --- Distillation Priming ---

  setDistillationPriming(sessionId: string, priming: DistillationPriming): void {
    this.db
      .prepare("UPDATE sessions SET distillation_priming = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
      .run(JSON.stringify(priming), sessionId);
  }

  getDistillationPriming(sessionId: string): DistillationPriming | null {
    const row = this.db
      .prepare("SELECT distillation_priming FROM sessions WHERE id = ?")
      .get(sessionId) as { distillation_priming: string | null } | undefined;
    if (!row?.distillation_priming) return null;
    return this.parseJSON<DistillationPriming>(row.distillation_priming);
  }

  clearDistillationPriming(sessionId: string): void {
    this.db
      .prepare("UPDATE sessions SET distillation_priming = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
      .run(sessionId);
  }

  // --- Agent Notes ---

  addNote(sessionId: string, nousId: string, category: AgentNote["category"], content: string): number {
    const result = this.db
      .prepare(
        "INSERT INTO agent_notes (session_id, nous_id, category, content) VALUES (?, ?, ?, ?)",
      )
      .run(sessionId, nousId, category, content);
    return result.lastInsertRowid as number;
  }

  getNotes(sessionId: string, opts?: { limit?: number; category?: AgentNote["category"] }): AgentNote[] {
    const limit = opts?.limit ?? 20;
    const conditions = ["session_id = ?"];
    const params: (string | number)[] = [sessionId];

    if (opts?.category) {
      conditions.push("category = ?");
      params.push(opts.category);
    }

    const rows = this.db
      .prepare(
        `SELECT * FROM agent_notes WHERE ${conditions.join(" AND ")} ORDER BY created_at DESC LIMIT ?`,
      )
      .all(...params, limit) as Record<string, unknown>[];

    return rows.reverse().map((r) => ({
      id: r["id"] as number,
      sessionId: r["session_id"] as string,
      nousId: r["nous_id"] as string,
      category: r["category"] as AgentNote["category"],
      content: r["content"] as string,
      createdAt: r["created_at"] as string,
    }));
  }

  deleteNote(noteId: number, nousId: string): boolean {
    const result = this.db
      .prepare("DELETE FROM agent_notes WHERE id = ? AND nous_id = ?")
      .run(noteId, nousId);
    return result.changes > 0;
  }

  getNotesForNous(nousId: string, opts?: { limit?: number }): AgentNote[] {
    const limit = opts?.limit ?? 20;
    const rows = this.db
      .prepare(
        "SELECT * FROM agent_notes WHERE nous_id = ? ORDER BY created_at DESC LIMIT ?",
      )
      .all(nousId, limit) as Record<string, unknown>[];

    return rows.reverse().map((r) => ({
      id: r["id"] as number,
      sessionId: r["session_id"] as string,
      nousId: r["nous_id"] as string,
      category: r["category"] as AgentNote["category"],
      content: r["content"] as string,
      createdAt: r["created_at"] as string,
    }));
  }

  // --- Message Queue ---

  queueMessage(sessionId: string, content: string, sender?: string): number {
    const result = this.db
      .prepare("INSERT INTO message_queue (session_id, content, sender) VALUES (?, ?, ?)")
      .run(sessionId, content, sender ?? null);
    return result.lastInsertRowid as number;
  }

  drainQueue(sessionId: string): QueuedMessage[] {
    const rows = this.db
      .prepare("SELECT * FROM message_queue WHERE session_id = ? ORDER BY created_at ASC")
      .all(sessionId) as Record<string, unknown>[];

    if (rows.length === 0) return [];

    this.db
      .prepare("DELETE FROM message_queue WHERE session_id = ?")
      .run(sessionId);

    return rows.map((r) => ({
      id: r["id"] as number,
      sessionId: r["session_id"] as string,
      content: r["content"] as string,
      sender: r["sender"] as string | null,
      createdAt: r["created_at"] as string,
    }));
  }

  getQueueLength(sessionId: string): number {
    const row = this.db
      .prepare("SELECT COUNT(*) as count FROM message_queue WHERE session_id = ?")
      .get(sessionId) as { count: number } | undefined;
    return row?.count ?? 0;
  }

  private mapThread(r: Record<string, unknown>): Thread {
    return {
      id: r["id"] as string,
      nousId: r["nous_id"] as string,
      identity: r["identity"] as string,
      createdAt: r["created_at"] as string,
      updatedAt: r["updated_at"] as string,
    };
  }

  private mapBinding(r: Record<string, unknown>): TransportBinding {
    return {
      id: r["id"] as string,
      threadId: r["thread_id"] as string,
      transport: r["transport"] as string,
      channelKey: r["channel_key"] as string,
      lastSeenAt: r["last_seen_at"] as string,
    };
  }

  /**
   * Get distillation summaries for a nous within a time range.
   * Summaries are assistant messages containing "Distillation #" in their content.
   */
  getDistillationSummaries(
    nousId: string,
    since: string,
    limit = 50,
  ): Array<{ sessionId: string; summary: string; createdAt: string }> {
    const rows = this.db
      .prepare(
        `SELECT m.session_id, m.content, m.created_at
         FROM messages m
         JOIN sessions s ON m.session_id = s.id
         WHERE s.nous_id = ? AND m.role = 'assistant'
           AND m.content LIKE '%Distillation #%'
           AND m.created_at >= ?
         ORDER BY m.id DESC
         LIMIT ?`,
      )
      .all(nousId, since, limit) as Record<string, unknown>[];

    return rows.map((r) => ({
      sessionId: r["session_id"] as string,
      summary: r["content"] as string,
      createdAt: r["created_at"] as string,
    }));
  }

  // --- Reflection Log ---

  recordReflection(record: {
    nousId: string;
    sessionsReviewed: number;
    messagesReviewed: number;
    findings: ReflectionFindings;
    memoriesStored: number;
    tokensUsed: number;
    durationMs: number;
    model: string;
    errors?: string;
  }): number {
    const result = this.db
      .prepare(
        `INSERT INTO reflection_log
         (nous_id, sessions_reviewed, messages_reviewed,
          patterns_found, contradictions_found, corrections_found,
          preferences_found, relationships_found, unresolved_threads_found,
          memories_stored, tokens_used, duration_ms, model, findings, errors)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        record.nousId,
        record.sessionsReviewed,
        record.messagesReviewed,
        record.findings.patterns.length,
        record.findings.contradictions.length,
        record.findings.corrections.length,
        record.findings.preferences.length,
        record.findings.relationships.length,
        record.findings.unresolvedThreads.length,
        record.memoriesStored,
        record.tokensUsed,
        record.durationMs,
        record.model,
        JSON.stringify(record.findings),
        record.errors ?? null,
      );
    return result.lastInsertRowid as number;
  }

  getReflectionLog(nousId: string, opts?: { limit?: number }): ReflectionLog[] {
    const limit = opts?.limit ?? 30;
    const rows = this.db
      .prepare(
        "SELECT * FROM reflection_log WHERE nous_id = ? ORDER BY id DESC LIMIT ?",
      )
      .all(nousId, limit) as Record<string, unknown>[];

    return rows.map((r) => this.mapReflectionLog(r));
  }

  getLastReflection(nousId: string): ReflectionLog | null {
    const row = this.db
      .prepare(
        "SELECT * FROM reflection_log WHERE nous_id = ? ORDER BY id DESC LIMIT 1",
      )
      .get(nousId) as Record<string, unknown> | undefined;
    return row ? this.mapReflectionLog(row) : null;
  }

  /**
   * Get sessions with meaningful human activity since a given time.
   * "Meaningful" = at least minMessages human messages, primary or standard sessions only.
   */
  getActiveSessionsSince(
    nousId: string,
    since: string,
    minMessages: number,
  ): Session[] {
    const rows = this.db
      .prepare(
        `SELECT s.*, COUNT(m.id) AS human_msg_count
         FROM sessions s
         JOIN messages m ON m.session_id = s.id AND m.role = 'user' AND m.is_distilled = 0 AND m.created_at >= ?
         WHERE s.nous_id = ? AND s.session_type = 'primary'
         GROUP BY s.id
         HAVING human_msg_count >= ?
         ORDER BY s.updated_at DESC`,
      )
      .all(since, nousId, minMessages) as Record<string, unknown>[];
    return rows.map((r) => this.mapSession(r));
  }

  private mapReflectionLog(r: Record<string, unknown>): ReflectionLog {
    let findings: ReflectionFindings = {
      patterns: [], contradictions: [], corrections: [],
      preferences: [], relationships: [], unresolvedThreads: [],
    };
    try {
      findings = JSON.parse(r["findings"] as string) as ReflectionFindings;
    } catch {
      log.warn(`Malformed findings JSON in reflection ${r["id"]}`);
    }
    return {
      id: r["id"] as number,
      nousId: r["nous_id"] as string,
      reflectedAt: r["reflected_at"] as string,
      sessionsReviewed: r["sessions_reviewed"] as number,
      messagesReviewed: r["messages_reviewed"] as number,
      patternsFound: r["patterns_found"] as number,
      contradictionsFound: r["contradictions_found"] as number,
      correctionsFound: r["corrections_found"] as number,
      preferencesFound: r["preferences_found"] as number,
      relationshipsFound: r["relationships_found"] as number,
      unresolvedThreadsFound: r["unresolved_threads_found"] as number,
      memoriesStored: r["memories_stored"] as number,
      tokensUsed: r["tokens_used"] as number,
      durationMs: r["duration_ms"] as number,
      model: r["model"] as string | null,
      findings,
      errors: r["errors"] as string | null,
    };
  }
}
