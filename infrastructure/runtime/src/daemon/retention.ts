// Data retention enforcement — purge distilled messages, truncate tool results
import { createLogger } from "../koina/logger.js";
import type { SessionStore } from "../mneme/store.js";
import type { PrivacySettings } from "../taxis/schema.js";

const log = createLogger("daemon:retention");

export interface RetentionResult {
  distilledMessagesDeleted: number;
  archivedMessagesDeleted: number;
  toolResultsTruncated: number;
  ephemeralSessionsDeleted: number;
}

/**
 * Run one retention cycle against the store.
 * Safe to call multiple times — each operation is idempotent.
 */
export function runRetention(
  store: SessionStore,
  privacy: PrivacySettings,
): RetentionResult {
  const { retention } = privacy;
  const result: RetentionResult = {
    distilledMessagesDeleted: 0,
    archivedMessagesDeleted: 0,
    toolResultsTruncated: 0,
    ephemeralSessionsDeleted: 0,
  };

  try {
    result.distilledMessagesDeleted = store.purgeDistilledMessages(
      retention.distilledMessageMaxAgeDays,
    );
  } catch (err) {
    log.error(`Retention: distilled purge failed: ${err instanceof Error ? err.message : err}`);
  }

  try {
    result.archivedMessagesDeleted = store.purgeArchivedSessionMessages(
      retention.archivedSessionMaxAgeDays,
    );
  } catch (err) {
    log.error(`Retention: archived purge failed: ${err instanceof Error ? err.message : err}`);
  }

  try {
    result.toolResultsTruncated = store.truncateToolResults(
      retention.toolResultMaxChars,
    );
  } catch (err) {
    log.error(`Retention: tool result truncation failed: ${err instanceof Error ? err.message : err}`);
  }

  try {
    result.ephemeralSessionsDeleted = store.deleteEphemeralSessions();
  } catch (err) {
    log.error(`Retention: ephemeral cleanup failed: ${err instanceof Error ? err.message : err}`);
  }

  const total =
    result.distilledMessagesDeleted +
    result.archivedMessagesDeleted +
    result.toolResultsTruncated +
    result.ephemeralSessionsDeleted;
  if (total > 0) {
    log.info(
      `Retention cycle complete: ${result.distilledMessagesDeleted} distilled msgs deleted, ` +
      `${result.archivedMessagesDeleted} archived msgs deleted, ` +
      `${result.toolResultsTruncated} tool results truncated, ` +
      `${result.ephemeralSessionsDeleted} ephemeral sessions deleted`,
    );
  }

  return result;
}
