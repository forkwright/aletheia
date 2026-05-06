//! Typed knowledge-sharing payloads for cross-nous messaging (R716 Phase 3).
//!
//! Payloads ride on the existing [`super::CrossNousMessage`] envelope; the
//! router's [`super::AddressMask`] is enforced at delivery time so private
//! nouses do not auto-receive verification proposals.
//!
//! Builder helpers ([`published_message`], [`verify_message`],
//! [`contest_message`], [`query_message`]) construct messages with the
//! payload set; callers send them through `super::router::CrossNousRouter`
//! exactly like any other message.

use std::time::Duration;

use eidos::id::FactId;
use eidos::knowledge::VerificationVerdict;
use koina::id::NousId;

use super::CrossNousMessage;

/// Typed knowledge-sharing payload variants per R716 Phase 3.
///
/// `#[non_exhaustive]` so future protocol additions (e.g. `KnowledgeRevoke`)
/// don't break match-exhaustiveness in downstream callers.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum KnowledgePayload {
    /// Notification: a fact has been published for cross-nous visibility.
    Published {
        /// Identifier of the published-fact record (typically a
        /// `PublishedFactId` rendered as `String`).
        shared_fact_id: String,
        /// Short human-readable summary for receiver-side display/log.
        summary: String,
    },
    /// Request: please verify this fact (response carries
    /// [`KnowledgeReply::Verified`]).
    Verify {
        /// Fact content to evaluate.
        fact_content: String,
        /// Originating nous so the responder can scope its response.
        requester: NousId,
    },
    /// Notification: this fact has been contested.
    Contest {
        /// Contested fact identifier.
        fact_id: FactId,
        /// Free-text reason recorded in provenance.
        reason: String,
    },
    /// Request: return facts matching this query (response carries
    /// [`KnowledgeReply::QueryResult`]).
    ///
    /// Default scope is the recipient's own cohort plus any explicit-allowlist
    /// cohorts. Recall enforcement is the recipient's responsibility; this
    /// payload carries the request shape.
    Query {
        /// Free-text query string.
        query: String,
        /// Optional filter expressions applied recipient-side.
        filters: Vec<String>,
    },
}

/// Reply payload for knowledge-sharing requests.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum KnowledgeReply {
    /// Verdict from the responding nous.
    Verified {
        /// Verdict cast on the requested fact.
        verdict: VerificationVerdict,
    },
    /// Query results.
    QueryResult {
        /// Matching fact identifiers (recipient-scoped).
        fact_ids: Vec<FactId>,
    },
}

/// Build a fire-and-forget `KnowledgePublished` notification.
#[must_use]
pub fn published_message(
    from: impl Into<String>,
    to: impl Into<String>,
    shared_fact_id: impl Into<String>,
    summary: impl Into<String>,
) -> CrossNousMessage {
    CrossNousMessage::new(from, to, "knowledge:published").with_payload(
        KnowledgePayload::Published {
            shared_fact_id: shared_fact_id.into(),
            summary: summary.into(),
        },
    )
}

/// Build a request-response `KnowledgeVerify` message that expects a reply
/// within the given timeout.
#[must_use]
pub fn verify_message(
    from: impl Into<String>,
    to: impl Into<String>,
    fact_content: impl Into<String>,
    requester: NousId,
    timeout: Duration,
) -> CrossNousMessage {
    CrossNousMessage::new(from, to, "knowledge:verify")
        .with_reply(timeout)
        .with_payload(KnowledgePayload::Verify {
            fact_content: fact_content.into(),
            requester,
        })
}

/// Build a fire-and-forget `KnowledgeContest` notification.
#[must_use]
pub fn contest_message(
    from: impl Into<String>,
    to: impl Into<String>,
    fact_id: FactId,
    reason: impl Into<String>,
) -> CrossNousMessage {
    CrossNousMessage::new(from, to, "knowledge:contest").with_payload(KnowledgePayload::Contest {
        fact_id,
        reason: reason.into(),
    })
}

/// Build a request-response `KnowledgeQuery` message that expects a reply
/// within the given timeout.
#[must_use]
pub fn query_message(
    from: impl Into<String>,
    to: impl Into<String>,
    query: impl Into<String>,
    filters: Vec<String>,
    timeout: Duration,
) -> CrossNousMessage {
    CrossNousMessage::new(from, to, "knowledge:query")
        .with_reply(timeout)
        .with_payload(KnowledgePayload::Query {
            query: query.into(),
            filters,
        })
}
