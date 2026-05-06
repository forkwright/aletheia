//! Multi-agent verification protocol per R716 Phase 3.
//!
//! Adds publish + vote + conflict-resolution operations on top of `eidos`'s
//! `PublishedFact` / `VerificationProposal` / `ConflictResolution` types
//! (landed in W8 PR1, #55).
//!
//! Vector-similarity `detect_conflict` requires plumbing a `KnowledgeStore`
//! reference into the extraction engine; that wiring is its own change.
//! This module ships pure-data + scoring primitives plus the persistence
//! schema.

pub mod conflict;
pub mod proposal;

pub use conflict::{Conflict, ConflictKind, ResolveError, resolve_conflict};
pub use proposal::{
    DEFAULT_VERIFICATION_THRESHOLD, VerificationOutcome, publish_fact, vote_on_proposal,
};

#[cfg(test)]
mod tests;
