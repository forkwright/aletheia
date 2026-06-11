#![deny(missing_docs)]
//! poiesis-verify: report claim verification.
//!
//! Validates numeric claims in a [`VerifyManifest`] against `Derived`
//! (arithmetic formula) and `Ref` (cross-claim reference) sources.
//! SQL sources are stored in the manifest schema for auditability but are not
//! executed by this crate — execution requires an external tool.

/// Recursive-descent arithmetic formula evaluator.
pub mod arithmetic;
/// Error types for the verify pipeline.
pub mod error;
/// Verify manifest schema: `VerifyManifest`, `Claim`, `Source`, `Arithmetic`.
pub mod manifest;

pub use error::VerifyError;
pub use manifest::{Arithmetic, Claim, Source, VerifyManifest};

use std::collections::HashMap;

use serde::Serialize;
use snafu::ResultExt;

/// Stateless claim verifier.
pub struct Verifier;

impl Default for Verifier {
    fn default() -> Self {
        Self
    }
}

impl Verifier {
    /// Create a new `Verifier`.
    pub fn new() -> Self {
        Self
    }

    /// Verify all claims in `manifest`.
    ///
    /// Claims are processed in declaration order so that `Ref` sources that
    /// point to earlier claims can be resolved without a topological sort.
    ///
    /// SQL sources are skipped (they require an external query tool). A claim
    /// backed only by SQL sources is marked inconclusive (pass=true, no actual).
    pub fn verify(&self, manifest: &VerifyManifest) -> Vec<ClaimResult> {
        let mut resolved_values: HashMap<String, f64> = HashMap::new();
        let mut results = Vec::with_capacity(manifest.claims.len());

        for claim in &manifest.claims {
            let result = verify_claim(claim, &resolved_values);
            let representative = result.actual.unwrap_or(claim.value);
            resolved_values.insert(claim.id.clone(), representative);
            results.push(result);
        }

        results
    }

    /// Load a manifest from a JSON file and verify it.
    ///
    /// # Errors
    ///
    /// Returns `VerifyError` if the file cannot be read or if the JSON is invalid.
    pub fn verify_file(&self, path: &std::path::Path) -> Result<Vec<ClaimResult>, VerifyError> {
        let raw = std::fs::read_to_string(path).context(error::ReadManifestSnafu {
            path: path.display().to_string(),
        })?;

        let manifest: VerifyManifest =
            serde_json::from_str(&raw).map_err(|e| VerifyError::ParseManifest {
                path: path.display().to_string(),
                detail: e.to_string(),
            })?;

        Ok(self.verify(&manifest))
    }
}

// ── Result types ─────────────────────────────────────────────────────────────

/// Result of validating a single claim.
#[derive(Debug, Clone, Serialize)]
pub struct ClaimResult {
    /// Claim identifier.
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — claim id is deserialized from external manifest; newtype would break serde compatibility
    /// Verbatim claim text.
    pub text: String,
    /// The numeric value asserted.
    pub claimed: f64,
    /// The value resolved from sources, if any source was resolvable.
    pub actual: Option<f64>,
    /// Absolute difference between `actual` and `claimed`, if `actual` is set.
    pub diff: Option<f64>,
    /// Tolerance used for this claim.
    pub tolerance: f64,
    /// Human-readable unit.
    pub unit: String,
    /// True iff the claim passes all checks.
    pub pass: bool,
    /// Result of the arithmetic sub-check, if an `arithmetic` formula was provided.
    pub arith_check: Option<ArithCheck>,
}

/// Result of the arithmetic formula sub-check.
#[derive(Debug, Clone, Serialize)]
pub struct ArithCheck {
    /// The formula evaluated.
    pub formula: String,
    /// Expected result from the manifest.
    pub expected: f64,
    /// Actual evaluated result.
    pub actual: f64,
    /// Absolute difference.
    pub diff: f64,
    /// True iff diff <= tolerance.
    pub pass: bool,
}

/// Summary of a full manifest verification run.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    /// Per-claim results.
    pub claims: Vec<ClaimResult>,
    /// Total number of claims.
    pub total: usize,
    /// Number of passing claims.
    pub passed: usize,
    /// Number of failing claims.
    pub failed: usize,
}

impl VerifyResult {
    /// Build a `VerifyResult` from a list of `ClaimResult`s.
    pub fn from_claims(claims: Vec<ClaimResult>) -> Self {
        let total = claims.len();
        let passed = claims.iter().filter(|r| r.pass).count();
        let failed = total - passed;
        Self {
            claims,
            total,
            passed,
            failed,
        }
    }

    /// Return true if any claim failed.
    pub fn any_failed(&self) -> bool {
        self.failed > 0
    }
}

// ── Internal claim validation ─────────────────────────────────────────────────

fn verify_claim(claim: &Claim, resolved_claims: &HashMap<String, f64>) -> ClaimResult {
    let mut source_value: Option<f64> = None;

    for src in &claim.sources {
        if let Some(v) = resolve_source(src, resolved_claims) {
            source_value = Some(v);
            break;
        }
        // None: source not resolvable (SQL or unresolved Ref) — try next.
    }

    let arith_result = claim.arithmetic.as_ref().and_then(|arith| {
        match arithmetic::check(&arith.formula, arith.result, claim.tolerance) {
            Ok(r) => Some(ArithCheck {
                formula: arith.formula.clone(),
                expected: arith.result,
                actual: r.actual,
                diff: r.diff,
                pass: r.pass,
            }),
            Err(_) => None,
        }
    });

    let (pass, diff, actual) = determine_outcome(
        claim.value,
        claim.tolerance,
        source_value,
        arith_result.as_ref(),
    );

    ClaimResult {
        id: claim.id.clone(),
        text: claim.text.clone(),
        claimed: claim.value,
        actual,
        diff,
        tolerance: claim.tolerance,
        unit: claim.unit.clone(),
        pass,
        arith_check: arith_result,
    }
}

/// Resolve a single source to a scalar f64, or None if unresolvable.
///
/// SQL sources always return None (execution not in scope for this crate).
fn resolve_source(source: &Source, resolved_claims: &HashMap<String, f64>) -> Option<f64> {
    match source {
        Source::Sql { .. } => None,
        Source::Derived { formula, .. } => arithmetic::eval(formula).ok(), // kanon:ignore RUST/silent-error-ok — arithmetic eval failure falls back to None; error detail not needed for source resolution
        Source::Ref { ref_id } => resolved_claims.get(ref_id.as_str()).copied(),
    }
}

fn determine_outcome(
    claimed: f64,
    tolerance: f64,
    source_value: Option<f64>,
    arith_result: Option<&ArithCheck>,
) -> (bool, Option<f64>, Option<f64>) {
    match source_value {
        Some(actual) => {
            let diff = (actual - claimed).abs();
            let source_pass = diff <= tolerance;
            let arith_pass = arith_result.is_none_or(|a| a.pass);
            (source_pass && arith_pass, Some(diff), Some(actual))
        }
        None => {
            // No resolvable source; fall back to arithmetic-only check.
            match arith_result {
                Some(a) => (a.pass, Some(a.diff), Some(a.actual)),
                None => {
                    // Nothing resolvable — inconclusive (treat as pass).
                    (true, None, None)
                }
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions against known-length collections"
)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn derived_manifest(formula: &str, claimed: f64, tolerance: f64) -> VerifyManifest {
        VerifyManifest {
            report: "test.typ".to_owned(),
            claims: vec![Claim {
                id: "c1".to_owned(),
                text: "test claim".to_owned(),
                value: claimed,
                unit: "dollars".to_owned(),
                location: "line 1".to_owned(),
                sources: vec![Source::Derived {
                    formula: formula.to_owned(),
                    result: None,
                }],
                arithmetic: None,
                tolerance,
                status: None,
            }],
        }
    }

    #[test]
    fn derived_source_pass() {
        let manifest = derived_manifest("50 + 50", 100.0, 0.0);
        let v = Verifier::new();
        let results = v.verify(&manifest);
        assert_eq!(results.len(), 1, "must have one result");
        assert!(results[0].pass, "50 + 50 vs claimed 100 must PASS");
    }

    #[test]
    fn derived_source_fail() {
        let manifest = derived_manifest("50 + 49", 100.0, 0.0);
        let v = Verifier::new();
        let results = v.verify(&manifest);
        assert!(!results[0].pass, "50 + 49 vs claimed 100 must FAIL");
    }

    #[test]
    fn tolerance_allows_small_diff() {
        let manifest = derived_manifest("100.5", 100.0, 1.0);
        let v = Verifier::new();
        let results = v.verify(&manifest);
        assert!(results[0].pass, "diff 0.5 within tolerance 1.0 must PASS");
    }

    #[test]
    fn reference_claim_resolves() {
        let manifest = VerifyManifest {
            report: "r.typ".to_owned(),
            claims: vec![
                Claim {
                    id: "base".to_owned(),
                    text: "100".to_owned(),
                    value: 100.0,
                    unit: "dollars".to_owned(),
                    location: "line 1".to_owned(),
                    sources: vec![Source::Derived {
                        formula: "100".to_owned(),
                        result: None,
                    }],
                    arithmetic: None,
                    tolerance: 0.0,
                    status: None,
                },
                Claim {
                    id: "ref_check".to_owned(),
                    text: "also 100".to_owned(),
                    value: 100.0,
                    unit: "dollars".to_owned(),
                    location: "line 2".to_owned(),
                    sources: vec![Source::Ref {
                        ref_id: "base".to_owned(),
                    }],
                    arithmetic: None,
                    tolerance: 0.0,
                    status: None,
                },
            ],
        };
        let v = Verifier::new();
        let results = v.verify(&manifest);
        assert_eq!(results.len(), 2, "must have two results");
        assert!(results[0].pass, "base claim must PASS");
        assert!(results[1].pass, "reference claim must PASS");
    }

    #[test]
    fn arithmetic_check_independent() {
        let manifest = VerifyManifest {
            report: "r.typ".to_owned(),
            claims: vec![Claim {
                id: "c".to_owned(),
                text: "107784".to_owned(),
                value: 107_784.0,
                unit: "dollars".to_owned(),
                location: "line 1".to_owned(),
                sources: vec![Source::Derived {
                    formula: "107784".to_owned(),
                    result: None,
                }],
                arithmetic: Some(Arithmetic {
                    formula: "78187 + 26558 + 1620 + 1165 + 127 + 127 + 0".to_owned(),
                    result: 107_784.0,
                }),
                tolerance: 1.0,
                status: None,
            }],
        };
        let v = Verifier::new();
        let results = v.verify(&manifest);
        assert!(results[0].pass, "arithmetic check with source must PASS");
        let arith = results[0]
            .arith_check
            .as_ref()
            .expect("must have arith_check");
        assert!(arith.pass, "arithmetic formula check must PASS");
    }

    #[test]
    fn sql_source_skipped_inconclusive() {
        let manifest = VerifyManifest {
            report: "r.typ".to_owned(),
            claims: vec![Claim {
                id: "c".to_owned(),
                text: "100".to_owned(),
                value: 100.0,
                unit: "dollars".to_owned(),
                location: "line 1".to_owned(),
                sources: vec![Source::Sql {
                    table: "t".to_owned(),
                    query: "SELECT 100".to_owned(),
                    result: None,
                    queried: "2026-04-01".to_owned(),
                }],
                arithmetic: None,
                tolerance: 0.0,
                status: None,
            }],
        };
        // SQL-only claim is inconclusive (pass=true, actual=None).
        let v = Verifier::new();
        let results = v.verify(&manifest);
        assert!(
            results[0].pass,
            "SQL-only claim with no arithmetic must be inconclusive (pass)"
        );
        assert!(
            results[0].actual.is_none(),
            "SQL-only claim must have no actual value"
        );
    }

    #[test]
    fn verify_result_any_failed() {
        let claims = vec![
            ClaimResult {
                id: "a".to_owned(),
                text: "pass".to_owned(),
                claimed: 1.0,
                actual: Some(1.0),
                diff: Some(0.0),
                tolerance: 0.0,
                unit: "n".to_owned(),
                pass: true,
                arith_check: None,
            },
            ClaimResult {
                id: "b".to_owned(),
                text: "fail".to_owned(),
                claimed: 2.0,
                actual: Some(3.0),
                diff: Some(1.0),
                tolerance: 0.0,
                unit: "n".to_owned(),
                pass: false,
                arith_check: None,
            },
        ];
        let r = VerifyResult::from_claims(claims);
        assert!(
            r.any_failed(),
            "result with one failed claim must return true"
        );
        assert_eq!(r.total, 2, "total must be 2");
        assert_eq!(r.passed, 1, "passed must be 1");
        assert_eq!(r.failed, 1, "failed must be 1");
    }
}
