// Manifest schema for poiesis-verify.
//
// WHY: the manifest is the source-of-truth for what was claimed and what was
// queried. Keeping deserialization in one place makes the schema easy to evolve.

use serde::{Deserialize, Serialize};

/// Top-level verify manifest: maps report claims to their sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyManifest {
    /// Path or name of the report this manifest covers.
    pub report: String,
    /// Every numeric (or categorical) claim made in the report.
    pub claims: Vec<Claim>,
}

/// A single verifiable claim from the report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    /// Unique, stable identifier used by reference sources.
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — claim id is deserialized from external manifest; newtype would break serde compatibility
    /// Verbatim text of the claim as it appears in the report.
    pub text: String,
    /// The numeric value asserted by the claim.
    pub value: f64,
    /// Human-readable unit (e.g. "dollars", "percent", "count").
    pub unit: String,
    /// Location in the report source (e.g. "line 104, h2 heading").
    pub location: String,
    /// One or more sources that back the claim.
    pub sources: Vec<Source>,
    /// Optional arithmetic formula that produces `value` from its components.
    pub arithmetic: Option<Arithmetic>,
    /// Maximum acceptable absolute difference between `value` and the resolved
    /// source value for the claim to PASS.
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    /// Last known status from a previous verification run.
    pub status: Option<String>,
}

/// A single source backing a claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[non_exhaustive]
pub enum Source {
    /// A SQL query (stored for record-keeping; execution not performed by this crate).
    Sql {
        /// Primary table queried (for display purposes).
        table: String,
        /// Full SQL query text.
        query: String,
        /// Last observed result (populated on verify, null in authored manifest).
        result: Option<f64>,
        /// ISO-8601 date the query was last run.
        queried: String,
    },
    /// An arithmetic expression derived from other values.
    Derived {
        /// Formula string (e.g. "106365 / 107784 * 100").
        formula: String,
        /// Last observed result.
        result: Option<f64>,
    },
    /// Pointer to another claim's validated value.
    #[serde(rename = "reference")]
    Ref {
        /// The `id` of the referenced claim.
        #[serde(rename = "ref")]
        ref_id: String,
    },
}

/// Arithmetic formula check for a claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arithmetic {
    /// Formula string (e.g. "78187 + 26558 + 1620").
    pub formula: String,
    /// Expected result of evaluating the formula.
    pub result: f64,
}

fn default_tolerance() -> f64 {
    // WHY: default to strict zero tolerance; callers must opt in to slack.
    0.0
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions against known-length collections"
)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sql_source() {
        let json = r#"
        {
            "report": "test.typ",
            "claims": [{
                "id": "total",
                "text": "$100",
                "value": 100.0,
                "unit": "dollars",
                "location": "line 1",
                "sources": [{
                    "type": "sql",
                    "table": "public.roi",
                    "query": "SELECT SUM(x) FROM public.roi",
                    "result": 100.45,
                    "queried": "2026-04-01"
                }],
                "arithmetic": null,
                "tolerance": 1.0,
                "status": "pass"
            }]
        }"#;

        let manifest: VerifyManifest = serde_json::from_str(json).expect("must parse");
        assert_eq!(manifest.report, "test.typ", "report name must match");
        assert_eq!(manifest.claims.len(), 1, "must have one claim");

        let claim = &manifest.claims[0];
        assert_eq!(claim.id, "total", "id must match");
        assert!((claim.value - 100.0_f64).abs() < 1e-9, "value must match");
        assert!(
            (claim.tolerance - 1.0_f64).abs() < 1e-9,
            "tolerance must match"
        );
    }

    #[test]
    fn roundtrip_derived_source() {
        let json = r#"
        {
            "report": "r.typ",
            "claims": [{
                "id": "pct",
                "text": "98.7%",
                "value": 98.7,
                "unit": "percent",
                "location": "line 5",
                "sources": [{
                    "type": "derived",
                    "formula": "106365 / 107784 * 100",
                    "result": 98.68
                }],
                "arithmetic": null,
                "tolerance": 0.1,
                "status": null
            }]
        }"#;

        let manifest: VerifyManifest = serde_json::from_str(json).expect("must parse");
        assert!(
            matches!(
                &manifest.claims[0].sources[0],
                Source::Derived { formula, .. } if formula == "106365 / 107784 * 100"
            ),
            "source must be Derived with correct formula"
        );
    }

    #[test]
    fn roundtrip_reference_source() {
        let json = r#"
        {
            "report": "r.typ",
            "claims": [{
                "id": "pct",
                "text": "98.7%",
                "value": 98.7,
                "unit": "percent",
                "location": "line 5",
                "sources": [{"type": "reference", "ref": "total_savings"}],
                "arithmetic": null,
                "tolerance": 0.0,
                "status": null
            }]
        }"#;

        let manifest: VerifyManifest = serde_json::from_str(json).expect("must parse");
        assert!(
            matches!(
                &manifest.claims[0].sources[0],
                Source::Ref { ref_id } if ref_id == "total_savings"
            ),
            "source must be Ref with correct ref_id"
        );
    }

    #[test]
    fn default_tolerance_is_zero() {
        let json = r#"
        {
            "report": "r.typ",
            "claims": [{
                "id": "x",
                "text": "1",
                "value": 1.0,
                "unit": "count",
                "location": "line 1",
                "sources": [{"type": "derived", "formula": "1", "result": 1.0}],
                "arithmetic": null,
                "status": null
            }]
        }"#;

        let manifest: VerifyManifest = serde_json::from_str(json).expect("must parse");
        assert!(
            manifest.claims[0].tolerance.abs() < 1e-12,
            "omitting tolerance must default to 0.0"
        );
    }
}
