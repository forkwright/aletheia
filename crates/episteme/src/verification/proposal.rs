//! Publish-and-vote primitives for multi-agent verification.

use eidos::knowledge::{
    EpistemicTier, Fact, PublishedFact, PublishedFactId, VerificationProposal, VerificationVerdict,
    VerificationVote,
};
use serde::{Deserialize, Serialize};

/// Default Accept-vote threshold that triggers auto-promotion.
///
/// Per R716 Phase 3: when N≥3 distinct nouses cast Accept, the proposal
/// promotes the fact to the proposed tier.
pub const DEFAULT_VERIFICATION_THRESHOLD: u32 = 3;

/// Outcome of casting a vote on a verification proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum VerificationOutcome {
    /// Vote recorded; threshold not yet met and no contest.
    Pending,
    /// Threshold met (N≥3 distinct Accepts); fact should be promoted.
    Promoted {
        /// New epistemic tier the proposal targets.
        new_tier: EpistemicTier,
    },
    /// At least one Contest vote present; resolution required.
    Contested {
        /// Free-text reason for surfacing — placeholder for richer semantics.
        reason: String,
    },
}

/// Build a `PublishedFact` from a private `Fact`.
///
/// Copy-on-publish: the original fact stays scoped to the publisher; this
/// returns a new published-fact record ready for cross-nous visibility.
/// Persistence (write to the `published_facts` Datalog relation) is a
/// concern of the caller — this function is pure.
#[must_use]
pub fn publish_fact(fact: &Fact, publisher: &koina::id::NousId) -> PublishedFact {
    PublishedFact {
        id: PublishedFactId(koina::ulid::Ulid::new().to_string()),
        original_fact_id: fact.id.clone(),
        published_by: publisher.clone(),
        published_at: jiff::Timestamp::now(),
        verification_count: 0,
        contested_by: Vec::new(),
        contest_reason: None,
    }
}

/// Append a vote to a proposal and compute the resulting outcome.
///
/// Counts Accept votes from DISTINCT voters (dedupes by voter `NousId`).
/// Any Contest vote short-circuits the outcome to `Contested`.
pub fn vote_on_proposal(
    proposal: &mut VerificationProposal,
    vote: VerificationVote,
    threshold: u32,
) -> VerificationOutcome {
    proposal.votes.push(vote);

    let mut has_contest = false;
    let mut accept_voters: Vec<&koina::id::NousId> = Vec::new();
    for v in &proposal.votes {
        match v.verdict {
            VerificationVerdict::Contest => has_contest = true,
            VerificationVerdict::Accept
                if !accept_voters
                    .iter()
                    .any(|existing| existing.as_str() == v.voter.as_str()) =>
            {
                accept_voters.push(&v.voter);
            }
            _ => {
                // Abstain (no-op for vote tally) and any future variants
                // added under #[non_exhaustive] are deliberately ignored
                // until the verification semantics extend.
            }
        }
    }

    if has_contest {
        return VerificationOutcome::Contested {
            reason: "vote-contested".to_owned(),
        };
    }

    let accept_count = u32::try_from(accept_voters.len()).unwrap_or(u32::MAX);
    if accept_count >= threshold {
        VerificationOutcome::Promoted {
            new_tier: proposal.proposed_tier,
        }
    } else {
        VerificationOutcome::Pending
    }
}
