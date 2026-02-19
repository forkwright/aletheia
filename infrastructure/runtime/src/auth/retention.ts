// TODO(unused): scaffolded for spec 3 (Auth & Updates) — not yet integrated into gateway
// Data retention enforcement
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";

const log = createLogger("retention");

export interface RetentionPolicy {
  activeSessionMaxAgeDays: number;
  archivedRetentionDays: number;
  distilledMessageRetentionDays: number;
  toolResultRetentionDays: number;
  auditLogRetentionDays: number;
}

export function enforceRetention(
  db: Database.Database,
  policy: RetentionPolicy,
): { archived: number; purged: number; trimmed: number; auditPurged: number } {
  let archived = 0;
  let purged = 0;
  let trimmed = 0;
  let auditPurged = 0;

  const tx = db.transaction(() => {
    // 1. Archive stale active sessions
    const archiveCutoff = new Date(
      Date.now() - policy.activeSessionMaxAgeDays * 86400000,
    ).toISOString();
    const archiveResult = db
      .prepare(
        `UPDATE sessions SET status = 'archived',
           session_key = session_key || ':archived:' || id
         WHERE status = 'active' AND updated_at < ?`,
      )
      .run(archiveCutoff);
    archived = archiveResult.changes;

    // 2. Delete old archived sessions entirely (messages + usage + session)
    const purgeCutoff = new Date(
      Date.now() - policy.archivedRetentionDays * 86400000,
    ).toISOString();
    const oldSessions = db
      .prepare(
        "SELECT id FROM sessions WHERE status = 'archived' AND updated_at < ?",
      )
      .all(purgeCutoff) as Array<{ id: string }>;

    if (oldSessions.length > 0) {
      const ids = oldSessions.map((s) => s.id);
      const placeholders = ids.map(() => "?").join(",");
      db.prepare(`DELETE FROM messages WHERE session_id IN (${placeholders})`).run(
        ...ids,
      );
      db.prepare(`DELETE FROM usage WHERE session_id IN (${placeholders})`).run(
        ...ids,
      );
      db.prepare(
        `DELETE FROM distillations WHERE session_id IN (${placeholders})`,
      ).run(...ids);
      db.prepare(`DELETE FROM sessions WHERE id IN (${placeholders})`).run(
        ...ids,
      );
      purged = oldSessions.length;
    }

    // 3. Delete old distilled messages (the summary replaces them)
    const distillCutoff = new Date(
      Date.now() - policy.distilledMessageRetentionDays * 86400000,
    ).toISOString();
    const distillResult = db
      .prepare(
        "DELETE FROM messages WHERE is_distilled = 1 AND created_at < ?",
      )
      .run(distillCutoff);
    trimmed += distillResult.changes;

    // 4. Truncate old tool results (keep tool name, drop content)
    const toolCutoff = new Date(
      Date.now() - policy.toolResultRetentionDays * 86400000,
    ).toISOString();
    const toolResult = db
      .prepare(
        `UPDATE messages SET content = '[truncated]'
         WHERE role = 'tool_result' AND LENGTH(content) > 100 AND created_at < ?`,
      )
      .run(toolCutoff);
    trimmed += toolResult.changes;

    // 5. Purge old audit log entries
    try {
      const auditCutoff = new Date(
        Date.now() - policy.auditLogRetentionDays * 86400000,
      ).toISOString();
      const auditResult = db
        .prepare("DELETE FROM audit_log WHERE timestamp < ?")
        .run(auditCutoff);
      auditPurged = auditResult.changes;
    } catch {
      // audit_log table may not exist
    }
  });

  tx();

  if (archived + purged + trimmed + auditPurged > 0) {
    log.info(
      `Retention: archived=${archived}, purged=${purged} sessions, trimmed=${trimmed} messages, audit=${auditPurged}`,
    );
  }

  return { archived, purged, trimmed, auditPurged };
}
