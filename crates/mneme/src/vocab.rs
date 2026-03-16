//! Controlled relationship type vocabulary for knowledge graph extraction.

/// Result of normalizing a raw relationship type against the controlled vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RelationType {
    /// Matched a known vocabulary type (canonical uppercase form).
    Valid(&'static str),
    /// Matched a rejected type: must not be persisted.
    Rejected,
    /// No match in vocabulary or rejected list. Caller decides whether to persist.
    Unknown(String),
}

/// Relationship types that must never enter the knowledge graph.
/// INVARIANT: `RELATES_TO` eliminated in vocab redesign. See `semantic-invariants.md`
const REJECTED_TYPES: &[&str] = &["RELATES_TO", "IS"];

/// Controlled vocabulary: mirrors Python `vocab.py` `_HARDCODED_VOCAB`.
const CONTROLLED_VOCAB: &[&str] = &[
    "COMMUNICATES_VIA",
    "COMPATIBLE_WITH",
    "CONFIGURED_WITH",
    "CONNECTED_TO",
    "CREATED",
    "DEPENDS_ON",
    "DIAGNOSED_WITH",
    "INSTALLED_ON",
    "INTERESTED_IN",
    "KNOWS",
    "LIVES_IN",
    "LOCATED_IN",
    "MAINTAINS",
    "MANAGES",
    "MEMBER_OF",
    "OWNS",
    "PART_OF",
    "PREFERS",
    "PRESCRIBED",
    "RUNS_ON",
    "SCHEDULED_FOR",
    "SERVES",
    "SKILLED_IN",
    "STUDIES",
    "TREATS",
    "USES",
    "VEHICLE_IS",
    "WORKS_AT",
];

/// Normalize a raw relationship string and classify it.
///
/// 1. Trim, uppercase, replace spaces/hyphens with underscores.
/// 2. Check rejected list → `Rejected`.
/// 3. Check controlled vocabulary → `Valid`.
/// 4. Check alias map → `Valid` (mapped canonical form).
/// 5. Otherwise → `Unknown` (uppercased form returned for caller to decide).
#[expect(
    clippy::expect_used,
    reason = "find() after contains() is guaranteed to succeed — both operate on the same CONTROLLED_VOCAB static"
)]
pub fn normalize_relation(raw: &str) -> RelationType {
    let normalized: String = raw
        .trim()
        .to_uppercase()
        .chars()
        .map(|c| if c == ' ' || c == '-' { '_' } else { c })
        .collect();

    if REJECTED_TYPES.contains(&normalized.as_str()) {
        return RelationType::Rejected;
    }

    if CONTROLLED_VOCAB.contains(&normalized.as_str()) {
        return RelationType::Valid(
            CONTROLLED_VOCAB
                .iter()
                .find(|&&v| v == normalized)
                .expect("just checked contains"),
        );
    }

    if let Some(mapped) = lookup_alias(&normalized) {
        return RelationType::Valid(mapped);
    }

    // Also try the lowercase form against the alias map (handles "works on" → "works_on")
    let lower = normalized.to_lowercase();
    if let Some(mapped) = lookup_alias(&lower) {
        return RelationType::Valid(mapped);
    }

    RelationType::Unknown(normalized)
}

/// Alias map mirroring Python `vocab.py` `TYPE_MAP`.
fn lookup_alias(key: &str) -> Option<&'static str> {
    match key {
        "has" | "HAS" | "has_a" | "HAS_A" => Some("OWNS"),
        "is_part_of" | "IS_PART_OF" | "part_of" | "PART_OF" => Some(vocab_entry("PART_OF")),
        "works_at" | "WORKS_AT" | "works_on" | "WORKS_ON" => Some(vocab_entry("WORKS_AT")),
        "lives_in" | "LIVES_IN" => Some(vocab_entry("LIVES_IN")),
        "located_in" | "LOCATED_IN" | "located_at" | "LOCATED_AT" => {
            Some(vocab_entry("LOCATED_IN"))
        }
        "uses" | "USES" | "used_by" | "USED_BY" | "used_for" | "USED_FOR" => {
            Some(vocab_entry("USES"))
        }
        "runs_on" | "RUNS_ON" | "runs" | "RUNS" => Some(vocab_entry("RUNS_ON")),
        "depends_on" | "DEPENDS_ON" | "requires" | "REQUIRES" => Some(vocab_entry("DEPENDS_ON")),
        "knows" | "KNOWS" | "knows_about" | "KNOWS_ABOUT" | "knows_of" | "KNOWS_OF" => {
            Some(vocab_entry("KNOWS"))
        }
        "prefers" | "PREFERS" | "likes" | "LIKES" => Some(vocab_entry("PREFERS")),
        "interested_in" | "INTERESTED_IN" => Some(vocab_entry("INTERESTED_IN")),
        "studies" | "STUDIES" | "studying" | "STUDYING" => Some(vocab_entry("STUDIES")),
        "created" | "CREATED" | "created_by" | "CREATED_BY" | "built" | "BUILT" | "made"
        | "MADE" => Some(vocab_entry("CREATED")),
        "maintains" | "MAINTAINS" => Some(vocab_entry("MAINTAINS")),
        "managed_by" | "MANAGED_BY" | "manages" | "MANAGES" => Some(vocab_entry("MANAGES")),
        "member_of" | "MEMBER_OF" | "belongs_to" | "BELONGS_TO" => Some(vocab_entry("MEMBER_OF")),
        "skilled_in" | "SKILLED_IN" | "skilled_at" | "SKILLED_AT" => {
            Some(vocab_entry("SKILLED_IN"))
        }
        "owns" | "OWNS" => Some(vocab_entry("OWNS")),
        "installed_on" | "INSTALLED_ON" | "installed" | "INSTALLED" => {
            Some(vocab_entry("INSTALLED_ON"))
        }
        "compatible_with" | "COMPATIBLE_WITH" => Some(vocab_entry("COMPATIBLE_WITH")),
        "connected_to" | "CONNECTED_TO" => Some(vocab_entry("CONNECTED_TO")),
        "communicates_via" | "COMMUNICATES_VIA" => Some(vocab_entry("COMMUNICATES_VIA")),
        "configured_with" | "CONFIGURED_WITH" => Some(vocab_entry("CONFIGURED_WITH")),
        "serves" | "SERVES" => Some(vocab_entry("SERVES")),
        "diagnosed_with" | "DIAGNOSED_WITH" => Some(vocab_entry("DIAGNOSED_WITH")),
        "prescribed" | "PRESCRIBED" => Some(vocab_entry("PRESCRIBED")),
        "treats" | "TREATS" => Some(vocab_entry("TREATS")),
        "scheduled_for" | "SCHEDULED_FOR" => Some(vocab_entry("SCHEDULED_FOR")),
        "vehicle_is" | "VEHICLE_IS" => Some(vocab_entry("VEHICLE_IS")),
        _ => None,
    }
}

/// Return a `&'static str` from `CONTROLLED_VOCAB` for a known key.
#[expect(
    clippy::expect_used,
    reason = "callers only pass keys that exist in CONTROLLED_VOCAB — a panic here is a programming error in the alias table"
)]
fn vocab_entry(key: &str) -> &'static str {
    CONTROLLED_VOCAB
        .iter()
        .find(|&&v| v == key)
        .expect("vocab_entry called with key not in CONTROLLED_VOCAB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relates_to_rejected() {
        assert_eq!(normalize_relation("RELATES_TO"), RelationType::Rejected);
    }

    #[test]
    fn is_rejected() {
        assert_eq!(normalize_relation("IS"), RelationType::Rejected);
    }

    #[test]
    fn relates_to_lowercase_rejected() {
        assert_eq!(normalize_relation("relates_to"), RelationType::Rejected);
    }

    #[test]
    fn relates_to_spaces_rejected() {
        assert_eq!(normalize_relation("relates to"), RelationType::Rejected);
    }

    #[test]
    fn knows_valid() {
        assert_eq!(normalize_relation("KNOWS"), RelationType::Valid("KNOWS"));
    }

    #[test]
    fn works_at_valid() {
        assert_eq!(
            normalize_relation("WORKS_AT"),
            RelationType::Valid("WORKS_AT")
        );
    }

    #[test]
    fn works_on_alias() {
        assert_eq!(
            normalize_relation("works on"),
            RelationType::Valid("WORKS_AT")
        );
    }

    #[test]
    fn has_maps_to_owns() {
        assert_eq!(normalize_relation("has"), RelationType::Valid("OWNS"));
    }

    #[test]
    fn connected_to_valid() {
        assert_eq!(
            normalize_relation("CONNECTED_TO"),
            RelationType::Valid("CONNECTED_TO")
        );
    }

    #[test]
    fn unknown_type() {
        assert_eq!(
            normalize_relation("SOME_NEW_TYPE"),
            RelationType::Unknown("SOME_NEW_TYPE".to_owned())
        );
    }

    #[test]
    fn member_of_alias() {
        assert_eq!(
            normalize_relation("member of"),
            RelationType::Valid("MEMBER_OF")
        );
    }

    #[test]
    fn hyphenated_alias() {
        assert_eq!(
            normalize_relation("works-at"),
            RelationType::Valid("WORKS_AT")
        );
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(normalize_relation("knows"), RelationType::Valid("KNOWS"));
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(
            normalize_relation("  KNOWS  "),
            RelationType::Valid("KNOWS")
        );
    }

    #[test]
    fn controlled_vocab_excludes_relates_to() {
        assert!(!CONTROLLED_VOCAB.contains(&"RELATES_TO"));
    }

    #[test]
    fn all_vocab_entries_are_uppercase() {
        for entry in CONTROLLED_VOCAB {
            assert_eq!(*entry, entry.to_uppercase(), "{entry} is not uppercase");
        }
    }

    #[test]
    fn created_by_alias() {
        assert_eq!(
            normalize_relation("created by"),
            RelationType::Valid("CREATED")
        );
    }

    #[test]
    fn depends_on_alias() {
        assert_eq!(
            normalize_relation("depends on"),
            RelationType::Valid("DEPENDS_ON")
        );
    }

    #[test]
    fn normalize_empty_string() {
        assert_eq!(normalize_relation(""), RelationType::Unknown(String::new()));
    }

    #[test]
    fn normalize_whitespace_only() {
        assert_eq!(
            normalize_relation("   "),
            RelationType::Unknown(String::new())
        );
    }

    #[test]
    fn normalize_all_controlled_types() {
        for &entry in CONTROLLED_VOCAB {
            let result = normalize_relation(entry);
            assert_eq!(
                result,
                RelationType::Valid(entry),
                "{entry} should normalize to Valid"
            );
        }
    }

    #[test]
    fn normalize_case_variations() {
        assert_eq!(normalize_relation("Knows"), RelationType::Valid("KNOWS"));
        assert_eq!(
            normalize_relation("dEpEnDs_On"),
            RelationType::Valid("DEPENDS_ON")
        );
        assert_eq!(normalize_relation("uses"), RelationType::Valid("USES"));
        assert_eq!(
            normalize_relation("Lives In"),
            RelationType::Valid("LIVES_IN")
        );
    }

    #[test]
    fn normalize_owns_alias() {
        assert_eq!(
            normalize_relation("has_a"),
            RelationType::Valid("OWNS"),
            "'has_a' should normalize to OWNS"
        );
    }

    #[test]
    fn normalize_works_at_alias() {
        assert_eq!(
            normalize_relation("WORKS_ON"),
            RelationType::Valid("WORKS_AT"),
            "'WORKS_ON' should normalize to WORKS_AT"
        );
    }

    #[test]
    fn normalize_created_alias() {
        assert_eq!(
            normalize_relation("built"),
            RelationType::Valid("CREATED"),
            "'built' should normalize to CREATED"
        );
    }
}
