//! Session CRUD operations.

use snafu::ResultExt;
use tracing::{info, instrument};

use super::{map_session, OptionalExt, SessionStore};
use crate::error::{self, Result};
use crate::types::{Session, SessionStatus, SessionType};

impl SessionStore {
    // --- Sessions ---

    /// Find an active session by nous ID and session key.
    #[instrument(skip(self))]
    pub fn find_session(&self, nous_id: &str, session_key: &str) -> Result<Option<Session>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT * FROM sessions WHERE nous_id = ?1 AND session_key = ?2 AND status = 'active'",
            )
            .context(error::DatabaseSnafu)?;

        let session = stmt
            .query_row([nous_id, session_key], map_session)
            .optional()
            .context(error::DatabaseSnafu)?;

        Ok(session)
    }

    /// Find a session by ID (any status).
    #[instrument(skip(self))]
    pub fn find_session_by_id(&self, id: &str) -> Result<Option<Session>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT * FROM sessions WHERE id = ?1")
            .context(error::DatabaseSnafu)?;

        let session = stmt
            .query_row([id], map_session)
            .optional()
            .context(error::DatabaseSnafu)?;

        Ok(session)
    }

    /// Create a new session.
    #[instrument(skip(self))]
    pub fn create_session(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        parent_session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<Session> {
        let session_type = SessionType::from_key(session_key);

        self.conn
            .execute(
                "INSERT INTO sessions (id, nous_id, session_key, parent_session_id, model, session_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, nous_id, session_key, parent_session_id, model, session_type.as_str()],
            )
            .context(error::DatabaseSnafu)?;

        info!(id, nous_id, session_key, %session_type, "created session");

        self.find_session_by_id(id)?.ok_or_else(|| {
            error::SessionCreateSnafu {
                nous_id: nous_id.to_owned(),
            }
            .build()
        })
    }

    /// Find or create an active session. Reactivates archived sessions if found.
    #[instrument(skip(self))]
    pub fn find_or_create_session(
        &self,
        id: &str,
        nous_id: &str,
        session_key: &str,
        model: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> Result<Session> {
        // Check for active session
        if let Some(session) = self.find_session(nous_id, session_key)? {
            return Ok(session);
        }

        // Check for archived/distilled session — reactivate
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id FROM sessions WHERE nous_id = ?1 AND session_key = ?2 AND status != 'active' ORDER BY updated_at DESC LIMIT 1",
            )
            .context(error::DatabaseSnafu)?;

        let archived_id: Option<String> = stmt
            .query_row([nous_id, session_key], |row| row.get(0))
            .optional()
            .context(error::DatabaseSnafu)?;

        if let Some(archived_id) = archived_id {
            self.conn
                .execute(
                    "UPDATE sessions SET status = 'active', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                    [&archived_id],
                )
                .context(error::DatabaseSnafu)?;
            info!(
                id = archived_id,
                nous_id, session_key, "reactivated archived session"
            );
            return self.find_session_by_id(&archived_id)?.ok_or_else(|| {
                error::SessionCreateSnafu {
                    nous_id: nous_id.to_owned(),
                }
                .build()
            });
        }

        // Create new
        self.create_session(id, nous_id, session_key, parent_session_id, model)
    }

    /// List sessions, optionally filtered by nous ID.
    #[instrument(skip(self))]
    pub fn list_sessions(&self, nous_id: Option<&str>) -> Result<Vec<Session>> {
        let mut sessions = Vec::new();

        if let Some(nous_id) = nous_id {
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT * FROM sessions WHERE nous_id = ?1 ORDER BY updated_at DESC",
                )
                .context(error::DatabaseSnafu)?;
            let rows = stmt
                .query_map([nous_id], map_session)
                .context(error::DatabaseSnafu)?;
            for row in rows {
                sessions.push(row.context(error::DatabaseSnafu)?);
            }
        } else {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT * FROM sessions ORDER BY updated_at DESC")
                .context(error::DatabaseSnafu)?;
            let rows = stmt
                .query_map([], map_session)
                .context(error::DatabaseSnafu)?;
            for row in rows {
                sessions.push(row.context(error::DatabaseSnafu)?);
            }
        }

        Ok(sessions)
    }

    /// Update session status.
    #[instrument(skip(self))]
    pub fn update_session_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET status = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2",
                rusqlite::params![status.as_str(), id],
            )
            .context(error::DatabaseSnafu)?;
        Ok(())
    }

    /// Update session display name.
    #[instrument(skip(self))]
    pub fn update_display_name(&self, id: &str, display_name: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE sessions SET display_name = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2",
                rusqlite::params![display_name, id],
            )
            .context(error::DatabaseSnafu)?;
        Ok(())
    }
}
