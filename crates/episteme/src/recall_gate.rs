//! Read-time confidence floor for recall (aletheia#7163).
//!
//! [`admission`](crate::admission) decides whether a fact enters the
//! knowledge graph at write time. Nothing decided whether a fact already in
//! the graph is trustworthy enough to answer a *given* question at read
//! time -- a fact recorded once at low confidence and never re-verified
//! answered a consequential question exactly as readily as a verified one,
//! and the caller had no way to tell which had happened. This module is that
//! missing read-time policy layer, built the same way `admission` gates
//! writes: a pure decision on top of signal `Fact` already carries.
//!
//! # Shape
//!
//! - A caller declares [`Stakes`] for the question it is asking.
//! - The store never lowers that: [`effective_stakes`] raises it when the
//!   fact's own `sensitivity` or `scope` warrants more caution than the
//!   caller thought to ask for.
//! - [`derive_confidence`] places the fact on the [`RecallConfidence`] total
//!   order -- not a score, an ordering -- from whatever checkable evidence it
//!   carries, falling back to its declared TTL (`valid_to`) only when there
//!   is no stronger signal.
//! - [`evaluate`] refuses a fact below the effective floor with a
//!   [`RecallRefusal`] naming the remedy, instead of silently serving it.
//!
//! No decay curve, no weighting: the ordering is the whole mechanism.

use crate::knowledge::{EpistemicTier, Fact, FactSensitivity, MemoryScope};

/// Minimum self-reported confidence, combined with a [`EpistemicTier::Verified`]
/// tier, for a fact to count as [`RecallConfidence::Verified`].
///
/// WHY: tier alone is a categorical claim ("this was checked"); pairing it
/// with a high confidence score guards against a fact whose tier was set to
/// `Verified` but whose extraction confidence was actually low.
const VERIFIED_CONFIDENCE_MIN: f64 = 0.85;

/// Minimum self-reported confidence for a fact to count as
/// [`RecallConfidence::Declared`] when it is not [`RecallConfidence::Verified`].
///
/// WHY: matches the midpoint a caller would read as "more likely true than
/// not, but not checked" -- the TTL-fallback tier the issue describes.
const DECLARED_CONFIDENCE_MIN: f64 = 0.5;

/// How much is riding on the answer a recalled fact would help produce.
///
/// Ordered `Advisory < Operational < Consequential`. A caller declares this
/// value; the store may only raise it, never lower it -- see
/// [`effective_stakes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[non_exhaustive]
pub enum Stakes {
    /// A casual question. A wrong answer costs a follow-up question, not a
    /// decision.
    #[default]
    Advisory,
    /// The answer feeds something the caller will act on but can still
    /// revise cheaply.
    Operational,
    /// The answer feeds a decision that is expensive, unsafe, or impossible
    /// to reverse.
    Consequential,
}

impl Stakes {
    /// Lowercase string form, stable for wire protocols and logs.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Advisory => "advisory",
            Self::Operational => "operational",
            Self::Consequential => "consequential",
        }
    }

    /// Minimum [`RecallConfidence`] a fact must clear to be served at this
    /// stakes level.
    #[must_use]
    pub fn confidence_floor(self) -> RecallConfidence {
        match self {
            Self::Advisory => RecallConfidence::Unknown,
            Self::Operational => RecallConfidence::Declared,
            Self::Consequential => RecallConfidence::Verified,
        }
    }
}

impl std::fmt::Display for Stakes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Stakes {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "advisory" => Ok(Self::Advisory),
            "operational" => Ok(Self::Operational),
            "consequential" => Ok(Self::Consequential),
            other => Err(format!("unknown stakes level: {other}")),
        }
    }
}

/// Where a stored fact stands on the "can this still be trusted" axis.
///
/// A total order, not a score: `Verified > Declared > Stale > Unknown >
/// Refuted`. See [`derive_confidence`] for how a [`Fact`] is placed on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum RecallConfidence {
    /// Superseded or intentionally forgotten: actively contradicted, not
    /// merely old. Ranks below [`Self::Unknown`] because the graph has
    /// positive evidence *against* it, not merely an absence of evidence.
    Refuted,
    /// No signal strong enough to place it higher: unverified tier and
    /// confidence below [`DECLARED_CONFIDENCE_MIN`].
    Unknown,
    /// Its declared validity window (`valid_to`) has passed. Ranks above
    /// `Unknown` because it once carried a positive assertion of validity
    /// that nothing has actively contradicted -- it has simply expired.
    Stale,
    /// Self-reported confidence clears [`DECLARED_CONFIDENCE_MIN`], within
    /// its declared TTL.
    Declared,
    /// Checked against ground truth ([`EpistemicTier::Verified`]) at or
    /// above [`VERIFIED_CONFIDENCE_MIN`] confidence, within its declared TTL.
    Verified,
}

impl RecallConfidence {
    /// Lowercase string form, stable for wire protocols and logs.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Refuted => "refuted",
            Self::Unknown => "unknown",
            Self::Stale => "stale",
            Self::Declared => "declared",
            Self::Verified => "verified",
        }
    }
}

impl std::fmt::Display for RecallConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A remedy a refused caller can act on to get an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Remedy {
    /// Push the fact's tier to [`EpistemicTier::Verified`] at high confidence
    /// before asking again.
    ReVerifyFact,
    /// The fact's declared validity window has passed; cite a fresher
    /// source instead.
    CiteFresherSource,
    /// Ask again at this lower stakes level; the fact already clears that
    /// floor.
    LowerStakes(Stakes),
}

impl std::fmt::Display for Remedy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReVerifyFact => f.write_str("re-verify this fact"),
            Self::CiteFresherSource => f.write_str("cite a fresher source"),
            Self::LowerStakes(stakes) => write!(f, "lower your stakes to {stakes}"),
        }
    }
}

/// Structured refusal: what was asked for, what was found, what would fix it.
///
/// Not an error. Callers surface this in a success payload -- see
/// [`evaluate`] -- so an agent can act on `remedies` without a failed tool
/// call.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecallRefusal {
    /// The stakes actually enforced: `max(caller-declared, node-derived)`.
    pub effective_stakes: Stakes,
    /// The [`RecallConfidence`] the effective stakes required.
    pub required: RecallConfidence,
    /// The fact's actual [`RecallConfidence`].
    pub actual: RecallConfidence,
    /// What would have to be true for this fact to answer the question.
    pub remedies: Vec<Remedy>,
}

/// Derive `fact`'s position on the [`RecallConfidence`] order as of `now`.
///
/// Priority, first match wins:
///
/// 1. [`RecallConfidence::Refuted`] -- forgotten or superseded.
/// 2. [`RecallConfidence::Stale`] -- declared TTL (`valid_to`) has passed.
/// 3. [`RecallConfidence::Verified`] -- `tier == Verified` and confidence at
///    or above [`VERIFIED_CONFIDENCE_MIN`].
/// 4. [`RecallConfidence::Declared`] -- confidence at or above
///    [`DECLARED_CONFIDENCE_MIN`].
/// 5. [`RecallConfidence::Unknown`] -- everything else.
#[must_use]
pub fn derive_confidence(fact: &Fact, now: jiff::Timestamp) -> RecallConfidence {
    if fact.lifecycle.is_forgotten || fact.lifecycle.superseded_by.is_some() {
        return RecallConfidence::Refuted;
    }
    if now >= fact.temporal.valid_to {
        return RecallConfidence::Stale;
    }
    if fact.provenance.tier == EpistemicTier::Verified
        && fact.provenance.confidence >= VERIFIED_CONFIDENCE_MIN
    {
        return RecallConfidence::Verified;
    }
    if fact.provenance.confidence >= DECLARED_CONFIDENCE_MIN {
        return RecallConfidence::Declared;
    }
    RecallConfidence::Unknown
}

/// Raise the floor on the store's own initiative when `sensitivity` or
/// `scope` warrants more caution than the caller declared.
///
/// WHY these two fields: they are the classification a fact already carries
/// independent of the question being asked -- exactly the "obvious
/// derivation inputs" the issue names. `Confidential`/`Internal` facts
/// require at least `Consequential`/`Operational` handling regardless of how
/// casually they were asked for; `Feedback`-scope facts (behavioral
/// corrections) require at least `Operational` because acting on a wrong one
/// compounds the mistake it was meant to correct.
#[must_use]
pub fn derive_node_stakes(sensitivity: FactSensitivity, scope: Option<MemoryScope>) -> Stakes {
    let from_sensitivity = match sensitivity {
        FactSensitivity::Confidential => Stakes::Consequential,
        FactSensitivity::Internal => Stakes::Operational,
        FactSensitivity::Public => Stakes::Advisory,
    };
    let from_scope = match scope {
        Some(MemoryScope::Feedback) => Stakes::Operational,
        _ => Stakes::Advisory,
    };
    from_sensitivity.max(from_scope)
}

/// Effective stakes for `fact`: `max(caller-declared, node-derived)`.
///
/// The store never lowers a caller's declared stakes, only raises them.
#[must_use]
pub fn effective_stakes(caller: Stakes, fact: &Fact) -> Stakes {
    caller.max(derive_node_stakes(fact.sensitivity, fact.scope))
}

/// The single lower stakes level (if any) at whose floor `actual` already
/// qualifies, so a refusal can name a concrete, working alternative rather
/// than a vague "ask more casually."
fn lower_stakes_that_would_pass(actual: RecallConfidence, effective: Stakes) -> Option<Stakes> {
    [Stakes::Advisory, Stakes::Operational, Stakes::Consequential]
        .into_iter()
        .find(|&candidate| candidate < effective && candidate.confidence_floor() <= actual)
}

/// Remedies for a fact whose `actual` confidence fell below `effective`'s floor.
fn remedies_for(actual: RecallConfidence, effective: Stakes) -> Vec<Remedy> {
    let mut remedies = Vec::new();
    match actual {
        // WHY: a refuted fact has nothing to re-verify -- it has been
        // actively superseded or forgotten. The only working remedy is a
        // different question, which is not one of the three named shapes,
        // so no in-band remedy applies (lowering stakes cannot help either:
        // `Stakes::Advisory`'s floor is `Unknown`, which `Refuted` still
        // fails).
        RecallConfidence::Refuted => {}
        RecallConfidence::Stale => remedies.push(Remedy::CiteFresherSource),
        RecallConfidence::Unknown | RecallConfidence::Declared | RecallConfidence::Verified => {
            remedies.push(Remedy::ReVerifyFact);
        }
    }
    if let Some(lower) = lower_stakes_that_would_pass(actual, effective) {
        remedies.push(Remedy::LowerStakes(lower));
    }
    remedies
}

/// Evaluate whether `fact` clears the effective recall floor for
/// `caller_stakes` as of `now`.
///
/// Returns the fact's [`RecallConfidence`] when it clears the floor. Returns
/// a [`RecallRefusal`] -- not an error -- when it does not; callers should
/// surface this as a structured response naming the remedy, not fail the
/// call.
pub fn evaluate(
    fact: &Fact,
    caller_stakes: Stakes,
    now: jiff::Timestamp,
) -> Result<RecallConfidence, RecallRefusal> {
    let effective = effective_stakes(caller_stakes, fact);
    let required = effective.confidence_floor();
    let actual = derive_confidence(fact, now);
    if actual >= required {
        return Ok(actual);
    }
    Err(RecallRefusal {
        effective_stakes: effective,
        required,
        actual,
        remedies: remedies_for(actual, effective),
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::knowledge::{FactLifecycle, FactProvenance, FactTemporal, Visibility, far_future};

    fn base_fact() -> Fact {
        Fact {
            id: crate::id::FactId::new("f-test").expect("valid id"),
            nous_id: "alice".to_owned(),
            fact_type: "preference".to_owned(),
            content: "prefers dark mode".to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: jiff::Timestamp::now(),
                valid_to: far_future(),
                recorded_at: jiff::Timestamp::now(),
            },
            provenance: FactProvenance {
                confidence: 0.2,
                tier: EpistemicTier::Inferred,
                source_session_id: None,
                stability_hours: 24.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: crate::knowledge::FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        }
    }

    #[test]
    fn stakes_ordering_is_advisory_lt_operational_lt_consequential() {
        assert!(Stakes::Advisory < Stakes::Operational);
        assert!(Stakes::Operational < Stakes::Consequential);
    }

    #[test]
    fn confidence_ordering_matches_the_declared_total_order() {
        assert!(RecallConfidence::Refuted < RecallConfidence::Unknown);
        assert!(RecallConfidence::Unknown < RecallConfidence::Stale);
        assert!(RecallConfidence::Stale < RecallConfidence::Declared);
        assert!(RecallConfidence::Declared < RecallConfidence::Verified);
    }

    #[test]
    fn low_confidence_never_reverified_is_unknown() {
        let fact = base_fact();
        let confidence = derive_confidence(&fact, jiff::Timestamp::now());
        assert_eq!(confidence, RecallConfidence::Unknown);
    }

    #[test]
    fn low_confidence_fact_served_at_advisory_stakes() {
        let fact = base_fact();
        let result = evaluate(&fact, Stakes::Advisory, jiff::Timestamp::now());
        assert!(result.is_ok(), "advisory stakes must serve an Unknown fact");
    }

    #[test]
    fn low_confidence_fact_refused_at_consequential_stakes() {
        let fact = base_fact();
        let result = evaluate(&fact, Stakes::Consequential, jiff::Timestamp::now());
        let refusal = result.expect_err("consequential stakes must refuse an Unknown fact");
        assert_eq!(refusal.required, RecallConfidence::Verified);
        assert_eq!(refusal.actual, RecallConfidence::Unknown);
        assert!(
            refusal.remedies.contains(&Remedy::ReVerifyFact),
            "refusal must name a remedy: {:?}",
            refusal.remedies
        );
    }

    #[test]
    fn low_confidence_fact_refused_at_operational_stakes() {
        let fact = base_fact();
        let refusal = evaluate(&fact, Stakes::Operational, jiff::Timestamp::now())
            .expect_err("operational stakes require at least Declared");
        assert_eq!(refusal.required, RecallConfidence::Declared);
        assert!(refusal.remedies.contains(&Remedy::ReVerifyFact));
        assert!(
            refusal
                .remedies
                .contains(&Remedy::LowerStakes(Stakes::Advisory)),
            "refusal should offer the lower stakes level that would pass: {:?}",
            refusal.remedies
        );
    }

    #[test]
    fn declared_confidence_clears_operational_but_not_consequential() {
        let mut fact = base_fact();
        fact.provenance.confidence = 0.7;
        assert_eq!(
            derive_confidence(&fact, jiff::Timestamp::now()),
            RecallConfidence::Declared
        );
        assert!(evaluate(&fact, Stakes::Operational, jiff::Timestamp::now()).is_ok());
        let refusal = evaluate(&fact, Stakes::Consequential, jiff::Timestamp::now())
            .expect_err("Declared does not clear the Verified floor");
        assert_eq!(refusal.actual, RecallConfidence::Declared);
    }

    #[test]
    fn verified_tier_and_confidence_clears_every_stakes_level() {
        let mut fact = base_fact();
        fact.provenance.tier = EpistemicTier::Verified;
        fact.provenance.confidence = 0.95;
        assert_eq!(
            derive_confidence(&fact, jiff::Timestamp::now()),
            RecallConfidence::Verified
        );
        assert!(evaluate(&fact, Stakes::Consequential, jiff::Timestamp::now()).is_ok());
    }

    #[test]
    fn verified_tier_with_low_confidence_is_not_verified() {
        // WHY: guards against a tier set to Verified with a low confidence
        // score -- tier alone is not sufficient evidence.
        let mut fact = base_fact();
        fact.provenance.tier = EpistemicTier::Verified;
        fact.provenance.confidence = 0.4;
        assert_eq!(
            derive_confidence(&fact, jiff::Timestamp::now()),
            RecallConfidence::Unknown
        );
    }

    #[test]
    fn expired_valid_to_is_stale_even_at_high_confidence() {
        let mut fact = base_fact();
        fact.provenance.tier = EpistemicTier::Verified;
        fact.provenance.confidence = 0.99;
        fact.temporal.valid_to = jiff::Timestamp::now() - std::time::Duration::from_mins(1);
        assert_eq!(
            derive_confidence(&fact, jiff::Timestamp::now()),
            RecallConfidence::Stale
        );
        let refusal = evaluate(&fact, Stakes::Operational, jiff::Timestamp::now())
            .expect_err("Stale does not clear the Declared floor");
        assert!(refusal.remedies.contains(&Remedy::CiteFresherSource));
    }

    #[test]
    fn superseded_fact_is_refuted_with_no_remedy() {
        let mut fact = base_fact();
        fact.lifecycle.superseded_by = Some(crate::id::FactId::new("f-new").expect("valid id"));
        assert_eq!(
            derive_confidence(&fact, jiff::Timestamp::now()),
            RecallConfidence::Refuted
        );
        let refusal = evaluate(&fact, Stakes::Advisory, jiff::Timestamp::now())
            .expect_err("Refuted fails even the Advisory floor");
        assert!(
            refusal.remedies.is_empty(),
            "a refuted fact has no in-band remedy: {:?}",
            refusal.remedies
        );
    }

    #[test]
    fn forgotten_fact_is_refuted() {
        let mut fact = base_fact();
        fact.lifecycle.is_forgotten = true;
        assert_eq!(
            derive_confidence(&fact, jiff::Timestamp::now()),
            RecallConfidence::Refuted
        );
    }

    #[test]
    fn confidential_sensitivity_raises_the_node_derived_floor() {
        let mut fact = base_fact();
        fact.sensitivity = FactSensitivity::Confidential;
        fact.provenance.confidence = 0.7; // Declared
        // Caller declared only Advisory, but Confidential sensitivity raises
        // the effective floor to Consequential regardless.
        let refusal = evaluate(&fact, Stakes::Advisory, jiff::Timestamp::now())
            .expect_err("Confidential sensitivity must raise the floor past Declared");
        assert_eq!(refusal.effective_stakes, Stakes::Consequential);
        assert_eq!(refusal.required, RecallConfidence::Verified);
    }

    #[test]
    fn feedback_scope_raises_the_node_derived_floor_to_operational() {
        let mut fact = base_fact();
        fact.scope = Some(MemoryScope::Feedback);
        // Unknown confidence fails Operational's Declared floor even though
        // the caller only declared Advisory.
        let refusal = evaluate(&fact, Stakes::Advisory, jiff::Timestamp::now())
            .expect_err("Feedback scope must raise the floor to Operational");
        assert_eq!(refusal.effective_stakes, Stakes::Operational);
    }

    #[test]
    fn stakes_from_str_round_trips_display() {
        for stakes in [Stakes::Advisory, Stakes::Operational, Stakes::Consequential] {
            let parsed: Stakes = stakes.to_string().parse().expect("valid stakes string");
            assert_eq!(parsed, stakes);
        }
    }

    #[test]
    fn stakes_from_str_rejects_unknown_value() {
        assert!("catastrophic".parse::<Stakes>().is_err());
    }

    #[test]
    fn remedy_display_matches_the_three_named_shapes() {
        assert_eq!(Remedy::ReVerifyFact.to_string(), "re-verify this fact");
        assert_eq!(
            Remedy::CiteFresherSource.to_string(),
            "cite a fresher source"
        );
        assert_eq!(
            Remedy::LowerStakes(Stakes::Advisory).to_string(),
            "lower your stakes to advisory"
        );
    }
}
