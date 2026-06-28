//! Request and response types for session endpoints.

#[path = "types_dto.rs"]
mod types_dto;
pub use types_dto::{
    CreateSessionRequest, HistoryMessage, HistoryParams, HistoryResponse, ListSessionsParams,
    ListSessionsResponse, RenameSessionRequest, ReplayMessage, ReplaySession,
    ReplayToolAuditRecord, ReplayTurnAttempt, ReplayUsageRecord, SendMessageRequest,
    SessionListItem, SessionReplayResponse, SessionResponse, StreamTurnRequest,
};

fn default_session_key() -> String {
    "main".to_owned()
}

impl SessionResponse {
    pub(super) fn from_mneme(s: &mneme::types::Session) -> Self {
        Self {
            id: s.id.clone(),
            nous_id: s.nous_id.clone(),
            session_key: s.session_key.clone(),
            status: s.status.as_str().to_owned(),
            model: s.model.clone(),
            name: s.origin.display_name.clone(),
            message_count: s.metrics.message_count,
            token_count_estimate: s.metrics.token_count_estimate,
            created_at: s.created_at.clone(),
            updated_at: s.updated_at.clone(),
        }
    }
}
