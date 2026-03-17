//! Message operations, distillation pipeline, and usage recording.

use snafu::ResultExt;
use tracing::{debug, info, instrument};

use super::{SessionStore, map_message};
use crate::error::{self, Result};
use crate::types::{Message, Role, UsageRecord};

impl SessionStore {
    // --- Messages ---

    /// Append a message to a session. Returns the sequence number.
    #[instrument(skip(self, content))]
    pub fn append_message(
        &self,
        session_id: &str,
        role: Role,
        content: &str,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        token_estimate: i64,
    ) -> Result<i64> {
        self.check_disk("append_message");
        self.require_writable()?;
        let tx = self
            .conn
            .unchecked_transaction()
            .context(error::DatabaseSnafu)?;

        // WHY: INSERT...SELECT computes the next seq within the INSERT statement,
        // eliminating the TOCTOU window that existed when a separate SELECT was
        // followed by a separate INSERT. The aggregate MAX() always returns one row
        // even when no messages exist yet, so COALESCE handles the empty-session case.
        tx.execute(
            "INSERT INTO messages (session_id, seq, role, content, tool_call_id, tool_name, token_estimate, is_distilled)
             SELECT ?1, COALESCE(MAX(seq), 0) + 1, ?2, ?3, ?4, ?5, ?6, 0
             FROM messages WHERE session_id = ?1",
            rusqlite::params![session_id, role.as_str(), content, tool_call_id, tool_name, token_estimate],
        )
        .context(error::DatabaseSnafu)?;

        let next_seq: i64 = tx
            .query_row(
                "SELECT seq FROM messages WHERE id = last_insert_rowid()",
                [],
                |row| row.get(0),
            )
            .context(error::DatabaseSnafu)?;

        tx.execute(
            "UPDATE sessions SET message_count = message_count + 1, token_count_estimate = token_count_estimate + ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2",
            rusqlite::params![token_estimate, session_id],
        )
        .context(error::DatabaseSnafu)?;

        tx.commit().context(error::DatabaseSnafu)?;

        debug!(session_id, seq = next_seq, %role, token_estimate, "appended message");
        Ok(next_seq)
    }

    /// Get message history for a session with optional `seq < before_seq` filter.
    ///
    /// The `before_seq` filter is applied at the SQL level so the database
    /// only returns rows that satisfy `seq < before_seq`: the LIMIT clause
    /// then operates on that already-filtered set.
    #[instrument(skip(self))]
    pub fn get_history_filtered(
        &self,
        session_id: &str,
        limit: Option<i64>,
        before_seq: Option<i64>,
    ) -> Result<Vec<Message>> {
        let mut messages = Vec::new();

        match (limit, before_seq) {
            (Some(limit), Some(before)) => {
                let mut stmt = self
                    .conn
                    .prepare_cached(
                        "SELECT * FROM (\
                           SELECT * FROM messages \
                           WHERE session_id = ?1 AND is_distilled = 0 AND seq < ?3 \
                           ORDER BY seq DESC LIMIT ?2\
                         ) ORDER BY seq ASC",
                    )
                    .context(error::DatabaseSnafu)?;
                let rows = stmt
                    .query_map(rusqlite::params![session_id, limit, before], map_message)
                    .context(error::DatabaseSnafu)?;
                for row in rows {
                    messages.push(row.context(error::DatabaseSnafu)?);
                }
            }
            (Some(limit), None) => {
                let mut stmt = self
                    .conn
                    .prepare_cached(
                        "SELECT * FROM (\
                           SELECT * FROM messages \
                           WHERE session_id = ?1 AND is_distilled = 0 \
                           ORDER BY seq DESC LIMIT ?2\
                         ) ORDER BY seq ASC",
                    )
                    .context(error::DatabaseSnafu)?;
                let rows = stmt
                    .query_map(rusqlite::params![session_id, limit], map_message)
                    .context(error::DatabaseSnafu)?;
                for row in rows {
                    messages.push(row.context(error::DatabaseSnafu)?);
                }
            }
            (None, Some(before)) => {
                let mut stmt = self
                    .conn
                    .prepare_cached(
                        "SELECT * FROM messages \
                         WHERE session_id = ?1 AND is_distilled = 0 AND seq < ?2 \
                         ORDER BY seq ASC",
                    )
                    .context(error::DatabaseSnafu)?;
                let rows = stmt
                    .query_map(rusqlite::params![session_id, before], map_message)
                    .context(error::DatabaseSnafu)?;
                for row in rows {
                    messages.push(row.context(error::DatabaseSnafu)?);
                }
            }
            (None, None) => {
                let mut stmt = self
                    .conn
                    .prepare_cached(
                        "SELECT * FROM messages \
                         WHERE session_id = ?1 AND is_distilled = 0 \
                         ORDER BY seq ASC",
                    )
                    .context(error::DatabaseSnafu)?;
                let rows = stmt
                    .query_map([session_id], map_message)
                    .context(error::DatabaseSnafu)?;
                for row in rows {
                    messages.push(row.context(error::DatabaseSnafu)?);
                }
            }
        }

        Ok(messages)
    }

    /// Get message history for a session.
    #[instrument(skip(self))]
    pub fn get_history(&self, session_id: &str, limit: Option<i64>) -> Result<Vec<Message>> {
        let mut messages = Vec::new();

        if let Some(limit) = limit {
            // Most recent N messages in chronological order
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT * FROM (SELECT * FROM messages WHERE session_id = ?1 AND is_distilled = 0 ORDER BY seq DESC LIMIT ?2) ORDER BY seq ASC",
                )
                .context(error::DatabaseSnafu)?;
            let rows = stmt
                .query_map(rusqlite::params![session_id, limit], map_message)
                .context(error::DatabaseSnafu)?;
            for row in rows {
                messages.push(row.context(error::DatabaseSnafu)?);
            }
        } else {
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT * FROM messages WHERE session_id = ?1 AND is_distilled = 0 ORDER BY seq ASC",
                )
                .context(error::DatabaseSnafu)?;
            let rows = stmt
                .query_map([session_id], map_message)
                .context(error::DatabaseSnafu)?;
            for row in rows {
                messages.push(row.context(error::DatabaseSnafu)?);
            }
        }

        Ok(messages)
    }

    /// Get message history within a token budget (most recent first, working backward).
    ///
    /// Iterates messages newest-first at the SQL level and stops once the
    /// budget is exhausted, so only the necessary rows are loaded into memory.
    /// At least one message is always returned (even if it alone exceeds the budget).
    ///
    /// # Errors
    ///
    /// Returns `Database` if the query fails.
    #[instrument(skip(self), level = "debug")]
    pub fn get_history_with_budget(
        &self,
        session_id: &str,
        max_tokens: i64,
    ) -> Result<Vec<Message>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT * FROM messages \
                 WHERE session_id = ?1 AND is_distilled = 0 \
                 ORDER BY seq DESC",
            )
            .context(error::DatabaseSnafu)?;

        let mut rows = stmt.query([session_id]).context(error::DatabaseSnafu)?;

        let mut result = Vec::new();
        let mut total: i64 = 0;

        while let Some(row) = rows.next().context(error::DatabaseSnafu)? {
            let msg = map_message(row).context(error::DatabaseSnafu)?;
            if total + msg.token_estimate > max_tokens && !result.is_empty() {
                break;
            }
            total += msg.token_estimate;
            result.push(msg);
        }

        result.reverse();
        Ok(result)
    }

    // --- Distillation ---

    /// Mark messages as distilled and recalculate session token count.
    #[instrument(skip(self, seqs), fields(count = seqs.len()))]
    pub fn mark_messages_distilled(&self, session_id: &str, seqs: &[i64]) -> Result<()> {
        self.require_writable()?;
        if seqs.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .unchecked_transaction()
            .context(error::DatabaseSnafu)?;

        // Mark each seq as distilled
        let mut stmt = tx
            .prepare_cached(
                "UPDATE messages SET is_distilled = 1 WHERE session_id = ?1 AND seq = ?2",
            )
            .context(error::DatabaseSnafu)?;
        for seq in seqs {
            stmt.execute(rusqlite::params![session_id, seq])
                .context(error::DatabaseSnafu)?;
        }
        drop(stmt);

        // Recalculate
        let (total_tokens, msg_count): (i64, i64) = tx
            .query_row(
                "SELECT COALESCE(SUM(token_estimate), 0), COUNT(*) FROM messages WHERE session_id = ?1 AND is_distilled = 0",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context(error::DatabaseSnafu)?;

        tx.execute(
            "UPDATE sessions SET token_count_estimate = ?1, message_count = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?3",
            rusqlite::params![total_tokens, msg_count, session_id],
        )
        .context(error::DatabaseSnafu)?;

        tx.commit().context(error::DatabaseSnafu)?;

        info!(
            session_id,
            distilled = seqs.len(),
            total_tokens,
            msg_count,
            "distilled messages"
        );
        Ok(())
    }

    /// Insert a distillation summary as a system message and remove distilled messages.
    ///
    /// In a single transaction:
    /// 1. Delete any existing summary at seq 0 (previous distillation result, distilled or not)
    /// 2. Delete all messages marked `is_distilled = 1`
    /// 3. Insert summary at seq 0: safe because remaining undistilled messages have seq ≥ 1
    /// 4. Recalculate session token and message counts
    ///
    /// WHY: The former approach shifted undistilled seq values up by 1 before inserting at seq 0.
    /// That caused a `UNIQUE(session_id, seq)` violation whenever consecutive undistilled messages
    /// existed (e.g. seq \[3,4,5\]: shifting 3→4 conflicts with existing 4). The shift is also
    /// unnecessary because the UNIQUE constraint is only violated if seq 0 already exists.
    /// Deleting the old summary first makes seq 0 available without any renumbering.
    #[instrument(skip(self, content))]
    pub fn insert_distillation_summary(&self, session_id: &str, content: &str) -> Result<()> {
        self.require_writable()?;
        let tx = self
            .conn
            .unchecked_transaction()
            .context(error::DatabaseSnafu)?;

        // Delete any previous distillation summary sitting at seq 0.
        // This covers two cases: (a) the old summary was never marked distilled because
        // the distillation pipeline only marks conversation messages, not its own summary,
        // and (b) the old summary was explicitly marked distilled before this call.
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND seq = 0",
            [session_id],
        )
        .context(error::DatabaseSnafu)?;

        // Delete all messages that have been marked for distillation.
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND is_distilled = 1",
            [session_id],
        )
        .context(error::DatabaseSnafu)?;

        // Insert summary at seq 0. Remaining undistilled messages always have seq ≥ 1,
        // so no UNIQUE(session_id, seq) conflict is possible here.
        #[expect(clippy::cast_possible_wrap, reason = "summary length fits in i64")]
        let token_estimate = (content.len() as i64 + 3) / 4;
        tx.execute(
            "INSERT INTO messages (session_id, seq, role, content, is_distilled, token_estimate, created_at)
             VALUES (?1, 0, 'system', ?2, 0, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            rusqlite::params![session_id, content, token_estimate],
        )
        .context(error::DatabaseSnafu)?;

        // Recalculate session counts
        let (total_tokens, msg_count): (i64, i64) = tx
            .query_row(
                "SELECT COALESCE(SUM(token_estimate), 0), COUNT(*) FROM messages WHERE session_id = ?1 AND is_distilled = 0",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context(error::DatabaseSnafu)?;

        tx.execute(
            "UPDATE sessions SET token_count_estimate = ?1, message_count = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?3",
            rusqlite::params![total_tokens, msg_count, session_id],
        )
        .context(error::DatabaseSnafu)?;

        tx.commit().context(error::DatabaseSnafu)?;

        info!(
            session_id,
            msg_count, total_tokens, "inserted distillation summary"
        );
        Ok(())
    }

    /// Record a distillation event: insert into distillations table, update session counters.
    #[instrument(skip(self))]
    pub fn record_distillation(
        &self,
        session_id: &str,
        messages_before: i64,
        messages_after: i64,
        tokens_before: i64,
        tokens_after: i64,
        model: Option<&str>,
    ) -> Result<()> {
        self.require_writable()?;
        let tx = self
            .conn
            .unchecked_transaction()
            .context(error::DatabaseSnafu)?;

        tx.execute(
            "INSERT INTO distillations (session_id, messages_before, messages_after, tokens_before, tokens_after, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![session_id, messages_before, messages_after, tokens_before, tokens_after, model],
        )
        .context(error::DatabaseSnafu)?;

        tx.execute(
            "UPDATE sessions SET distillation_count = distillation_count + 1, last_distilled_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            [session_id],
        )
        .context(error::DatabaseSnafu)?;

        tx.commit().context(error::DatabaseSnafu)?;

        info!(
            session_id,
            messages_before, messages_after, tokens_before, tokens_after, "recorded distillation"
        );
        Ok(())
    }

    // --- Usage ---

    /// Check if usage has already been recorded for a given session + turn.
    #[instrument(skip(self), level = "debug")]
    pub fn usage_exists_for_turn(&self, session_id: &str, turn_seq: i64) -> Result<bool> {
        let exists: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM usage WHERE session_id = ?1 AND turn_seq = ?2)",
                rusqlite::params![session_id, turn_seq],
                |row| row.get(0),
            )
            .context(error::DatabaseSnafu)?;
        Ok(exists)
    }

    /// Record token usage for a turn.
    #[instrument(skip(self, record), level = "debug")]
    pub fn record_usage(&self, record: &UsageRecord) -> Result<()> {
        self.require_writable()?;
        self.conn
            .execute(
                "INSERT INTO usage (session_id, turn_seq, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    record.session_id,
                    record.turn_seq,
                    record.input_tokens,
                    record.output_tokens,
                    record.cache_read_tokens,
                    record.cache_write_tokens,
                    record.model,
                ],
            )
            .context(error::DatabaseSnafu)?;
        Ok(())
    }
}
