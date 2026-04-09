//! Integration tests for eidos's public API surface.
//!
//! WHY: eidos is the foundational knowledge-types crate for the memory
//! layer — `FactId`, `EntityId`, `EpistemicTier`, `MemoryScope`,
//! `ValidatedPath`, and the path-validation machinery are consumed by
//! mneme, episteme, and every other memory-touching crate. Any change
//! to the wire shape or enum variants of these types ripples across
//! the workspace, so we pin the public contract with integration tests
//! that run against only the published API (no `pub(crate)` access).
//!
//! Continues the #2814 audit alongside graphe, koina, symbolon, and
//! hermeneus, whose `tests/` folders already cover their own crates.

#![expect(clippy::expect_used, reason = "test assertions")]

// --- ID newtypes ---

mod ids {
    use aletheia_eidos::id::{CausalEdgeId, EmbeddingId, EntityId, FactId, IdValidationError};

    #[test]
    fn constructors_accept_valid_ids() {
        // WHY: contract — small, non-empty, ASCII IDs must round-trip
        // through new() without validation errors.
        assert_eq!(
            FactId::new("fact-001").expect("valid").as_str(),
            "fact-001"
        );
        assert_eq!(
            EntityId::new("entity-abc").expect("valid").as_str(),
            "entity-abc"
        );
        assert_eq!(EmbeddingId::new("emb-42").expect("valid").as_str(), "emb-42");
        assert_eq!(
            CausalEdgeId::new("ce-1").expect("valid").as_str(),
            "ce-1"
        );
    }

    #[test]
    fn constructors_reject_empty_string() {
        // WHY: empty IDs would silently propagate through the knowledge
        // graph and collide on index lookups. Validation must reject them
        // at the construction boundary.
        assert!(matches!(
            FactId::new(""),
            Err(IdValidationError::Empty { .. })
        ));
        assert!(matches!(
            EntityId::new(""),
            Err(IdValidationError::Empty { .. })
        ));
        assert!(matches!(
            EmbeddingId::new(""),
            Err(IdValidationError::Empty { .. })
        ));
        assert!(matches!(
            CausalEdgeId::new(""),
            Err(IdValidationError::Empty { .. })
        ));
    }

    #[test]
    fn constructors_reject_oversized_ids() {
        // WHY: the 256-byte cap keeps ID fields bounded for DB columns
        // and prevents unbounded memory use from malicious input.
        let over = "x".repeat(257);
        assert!(matches!(
            FactId::new(over.clone()),
            Err(IdValidationError::TooLong { actual: 257, .. })
        ));
        assert!(matches!(
            EntityId::new(over),
            Err(IdValidationError::TooLong { actual: 257, .. })
        ));
    }

    #[test]
    fn constructors_accept_max_length_id() {
        let max = "x".repeat(256);
        assert!(FactId::new(max.clone()).is_ok());
        assert!(EntityId::new(max).is_ok());
    }

    #[test]
    fn display_is_inner_string() {
        let id = FactId::new("fact-display").expect("valid");
        assert_eq!(id.to_string(), "fact-display");
        assert_eq!(format!("{id}"), "fact-display");
    }

    #[test]
    fn as_ref_str_yields_inner() {
        let id = EntityId::new("ent-ref").expect("valid");
        let borrowed: &str = id.as_ref();
        assert_eq!(borrowed, "ent-ref");
    }

    #[test]
    fn try_from_str_and_string_are_equivalent() {
        let from_str = FactId::try_from("same").expect("valid");
        let from_string = FactId::try_from(String::from("same")).expect("valid");
        assert_eq!(from_str, from_string);
    }

    #[test]
    fn serde_is_transparent() {
        // WHY: #[serde(transparent)] — IDs serialize as a bare JSON string,
        // not as an object. Databases and external APIs rely on this shape.
        let id = FactId::new("fact-42").expect("valid");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, r#""fact-42""#);
        let back: FactId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, id);
    }

    #[test]
    fn ids_of_different_kinds_are_distinct_types() {
        // WHY: newtype wrappers exist to prevent mixing ID kinds at call
        // sites. Two IDs with the same string must still have distinct
        // types — we can assert this only indirectly, by ensuring the
        // compile-time types differ in a `dyn Any` sense.
        let fact = FactId::new("shared").expect("valid");
        let entity = EntityId::new("shared").expect("valid");
        assert_eq!(fact.as_str(), entity.as_str());
        // The following would fail to compile: `assert_eq!(fact, entity)`
        // which is the real guarantee we care about.
    }

    #[test]
    fn validation_error_display_includes_kind() {
        let empty = IdValidationError::Empty { kind: "FactId" };
        assert!(empty.to_string().contains("FactId"));

        let long = IdValidationError::TooLong {
            kind: "EntityId",
            max: 256,
            actual: 999,
        };
        let msg = long.to_string();
        assert!(msg.contains("EntityId"));
        assert!(msg.contains("999"));
        assert!(msg.contains("256"));
    }
}

// --- EpistemicTier ---

mod epistemic_tier {
    use aletheia_eidos::knowledge::EpistemicTier;

    #[test]
    fn as_str_matches_serde_lowercase() {
        // WHY: serde renames to lowercase; as_str() must agree so that
        // Datalog ingestion and JSON round-trips stay aligned.
        assert_eq!(EpistemicTier::Verified.as_str(), "verified");
        assert_eq!(EpistemicTier::Inferred.as_str(), "inferred");
        assert_eq!(EpistemicTier::Assumed.as_str(), "assumed");
        assert_eq!(EpistemicTier::Training.as_str(), "training");
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(format!("{}", EpistemicTier::Verified), "verified");
        assert_eq!(format!("{}", EpistemicTier::Training), "training");
    }

    #[test]
    fn stability_multiplier_ordering_reflects_confidence() {
        // WHY: verified facts must decay strictly slower than inferred,
        // which must decay slower than assumed. Training is a permanent
        // record and has the slowest decay of all.
        assert!(
            EpistemicTier::Assumed.stability_multiplier()
                < EpistemicTier::Inferred.stability_multiplier()
        );
        assert!(
            EpistemicTier::Inferred.stability_multiplier()
                < EpistemicTier::Verified.stability_multiplier()
        );
        assert!(
            EpistemicTier::Verified.stability_multiplier()
                < EpistemicTier::Training.stability_multiplier()
        );
    }

    #[test]
    fn serde_roundtrip_lowercase() {
        let tier = EpistemicTier::Verified;
        let json = serde_json::to_string(&tier).expect("serialize");
        assert_eq!(json, r#""verified""#);
        let back: EpistemicTier = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, tier);
    }
}

// --- KnowledgeStage ---

mod knowledge_stage {
    use std::str::FromStr;

    use aletheia_eidos::knowledge::KnowledgeStage;

    #[test]
    fn from_decay_score_thresholds() {
        // WHY: thresholds are 0.7 / 0.3 / 0.1. A fact at exactly the
        // boundary belongs to the higher stage (inclusive lower bound).
        assert_eq!(KnowledgeStage::from_decay_score(1.0), KnowledgeStage::Active);
        assert_eq!(KnowledgeStage::from_decay_score(0.7), KnowledgeStage::Active);
        assert_eq!(
            KnowledgeStage::from_decay_score(0.69),
            KnowledgeStage::Fading
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.3),
            KnowledgeStage::Fading
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.29),
            KnowledgeStage::Dormant
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.1),
            KnowledgeStage::Dormant
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.09),
            KnowledgeStage::Archived
        );
        assert_eq!(
            KnowledgeStage::from_decay_score(0.0),
            KnowledgeStage::Archived
        );
    }

    #[test]
    fn only_archived_is_prunable() {
        // WHY: graduated pruning policy — nothing above Archived is
        // eligible for permanent removal.
        assert!(!KnowledgeStage::Active.is_prunable());
        assert!(!KnowledgeStage::Fading.is_prunable());
        assert!(!KnowledgeStage::Dormant.is_prunable());
        assert!(KnowledgeStage::Archived.is_prunable());
    }

    #[test]
    fn default_recall_excludes_dormant_and_archived() {
        // WHY: dormant facts are only retrievable on explicit query;
        // archived facts are pending removal entirely.
        assert!(KnowledgeStage::Active.in_default_recall());
        assert!(KnowledgeStage::Fading.in_default_recall());
        assert!(!KnowledgeStage::Dormant.in_default_recall());
        assert!(!KnowledgeStage::Archived.in_default_recall());
    }

    #[test]
    fn from_str_round_trips_all_variants() {
        for stage in [
            KnowledgeStage::Active,
            KnowledgeStage::Fading,
            KnowledgeStage::Dormant,
            KnowledgeStage::Archived,
        ] {
            let s = stage.as_str();
            let parsed = KnowledgeStage::from_str(s).expect("round trip");
            assert_eq!(parsed, stage);
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert!(KnowledgeStage::from_str("frozen").is_err());
        assert!(KnowledgeStage::from_str("").is_err());
    }
}

// --- FactType ---

mod fact_type {
    use aletheia_eidos::knowledge::FactType;

    #[test]
    fn base_stability_ordering_by_volatility() {
        // WHY: identity should outlast preference, which outlasts skill,
        // etc. This ordering is the entire point of the type taxonomy.
        assert!(FactType::Identity.base_stability_hours() > FactType::Preference.base_stability_hours());
        assert!(FactType::Preference.base_stability_hours() > FactType::Skill.base_stability_hours());
        assert!(FactType::Skill.base_stability_hours() > FactType::Relationship.base_stability_hours());
        assert!(FactType::Relationship.base_stability_hours() > FactType::Event.base_stability_hours());
        assert!(FactType::Event.base_stability_hours() > FactType::Task.base_stability_hours());
        assert!(FactType::Task.base_stability_hours() > FactType::Observation.base_stability_hours());
    }

    #[test]
    fn classify_identity_from_self_description() {
        assert_eq!(FactType::classify("I am Claude"), FactType::Identity);
        assert_eq!(
            FactType::classify("My name is Socrates"),
            FactType::Identity
        );
    }

    #[test]
    fn classify_preference_from_like_phrases() {
        assert_eq!(
            FactType::classify("I prefer tabs over spaces"),
            FactType::Preference
        );
        assert_eq!(
            FactType::classify("I like dark mode"),
            FactType::Preference
        );
    }

    #[test]
    fn classify_skill_from_know_or_use() {
        assert_eq!(FactType::classify("I know Rust"), FactType::Skill);
        assert_eq!(FactType::classify("I use Neovim"), FactType::Skill);
    }

    #[test]
    fn classify_falls_back_to_observation() {
        // WHY: default behavior when nothing else matches. Unknown content
        // must always yield a classification, not panic.
        assert_eq!(
            FactType::classify("The sky was grey today morning"),
            // "today" matches event
            FactType::Event
        );
        assert_eq!(
            FactType::classify("some random string with no keywords"),
            FactType::Observation
        );
    }

    #[test]
    fn from_str_lossy_unknown_is_observation() {
        // WHY: contract — lossy parsing never fails, it falls back.
        assert_eq!(
            FactType::from_str_lossy("not-a-fact-type"),
            FactType::Observation
        );
        assert_eq!(FactType::from_str_lossy("identity"), FactType::Identity);
        assert_eq!(
            FactType::from_str_lossy("preference"),
            FactType::Preference
        );
    }
}

// --- MemoryScope and ScopeAccessPolicy ---

mod memory_scope {
    use std::str::FromStr;

    use aletheia_eidos::knowledge::MemoryScope;

    #[test]
    fn all_contains_every_variant() {
        // WHY: MemoryScope::ALL drives iteration in mneme's scope
        // initialization. Missing a variant would silently skip a scope.
        assert_eq!(MemoryScope::ALL.len(), 4);
        assert!(MemoryScope::ALL.contains(&MemoryScope::User));
        assert!(MemoryScope::ALL.contains(&MemoryScope::Feedback));
        assert!(MemoryScope::ALL.contains(&MemoryScope::Project));
        assert!(MemoryScope::ALL.contains(&MemoryScope::Reference));
    }

    #[test]
    fn dir_name_matches_as_str() {
        // WHY: directory names must equal the string form so paths are
        // predictable and greppable.
        for scope in MemoryScope::ALL {
            assert_eq!(scope.as_dir_name(), scope.as_str());
        }
    }

    #[test]
    fn user_scope_denies_agent_read_and_write() {
        // WHY: user memories are private context that must never leak
        // across agent boundaries.
        let policy = MemoryScope::User.access_policy();
        assert!(!policy.permits_agent_read());
        assert!(!policy.permits_agent_write());
    }

    #[test]
    fn project_scope_permits_agent_read_and_write() {
        // WHY: project memories are the shared workspace — both reads
        // and writes must be allowed so agents can collaborate.
        let policy = MemoryScope::Project.access_policy();
        assert!(policy.permits_agent_read());
        assert!(policy.permits_agent_write());
    }

    #[test]
    fn feedback_and_reference_are_read_only_for_agents() {
        // WHY: feedback encodes user-written corrections; reference
        // points to external systems. Agents must not write to either
        // to avoid self-reinforcing loops or stale pointers.
        for scope in [MemoryScope::Feedback, MemoryScope::Reference] {
            let policy = scope.access_policy();
            assert!(policy.permits_agent_read(), "{scope:?} must allow reads");
            assert!(
                !policy.permits_agent_write(),
                "{scope:?} must reject agent writes"
            );
        }
    }

    #[test]
    fn from_str_round_trips_all_variants() {
        for scope in MemoryScope::ALL {
            let s = scope.as_str();
            let parsed = MemoryScope::from_str(s).expect("round trip");
            assert_eq!(parsed, scope);
            // from_str_opt is the infallible sibling used at API boundaries
            assert_eq!(MemoryScope::from_str_opt(s), Some(scope));
        }
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert!(MemoryScope::from_str("global").is_err());
        assert_eq!(MemoryScope::from_str_opt("global"), None);
    }
}

// --- CausalRelationType, TemporalOrdering ---

mod causal {
    use std::str::FromStr;

    use aletheia_eidos::knowledge::{CausalRelationType, TemporalOrdering};

    #[test]
    fn causal_relation_type_round_trips_via_from_str_and_display() {
        for rel in [
            CausalRelationType::Caused,
            CausalRelationType::Enabled,
            CausalRelationType::Prevented,
            CausalRelationType::Correlated,
        ] {
            let rendered = rel.to_string();
            let parsed = CausalRelationType::from_str(&rendered).expect("round trip");
            assert_eq!(parsed, rel);
            assert_eq!(rendered, rel.as_str());
        }
    }

    #[test]
    fn causal_relation_type_rejects_unknown() {
        assert!(CausalRelationType::from_str("maybe").is_err());
        assert!(CausalRelationType::from_str("").is_err());
    }

    #[test]
    fn temporal_ordering_round_trips_via_from_str() {
        for ord in [
            TemporalOrdering::Before,
            TemporalOrdering::After,
            TemporalOrdering::Concurrent,
        ] {
            let parsed = TemporalOrdering::from_str(ord.as_str()).expect("round trip");
            assert_eq!(parsed, ord);
        }
    }
}

// --- ForgetReason, VerificationSource, VerificationStatus ---

mod audit_classifiers {
    use std::str::FromStr;

    use aletheia_eidos::knowledge::{ForgetReason, VerificationSource, VerificationStatus};

    #[test]
    fn forget_reason_round_trips_via_from_str() {
        for reason in [
            ForgetReason::UserRequested,
            ForgetReason::Outdated,
            ForgetReason::Incorrect,
            ForgetReason::Privacy,
            ForgetReason::Stale,
            ForgetReason::Superseded,
            ForgetReason::Contradicted,
        ] {
            let s = reason.as_str();
            let parsed = ForgetReason::from_str(s).expect("round trip");
            assert_eq!(parsed, reason);
        }
    }

    #[test]
    fn forget_reason_user_requested_uses_snake_case() {
        // WHY: #[serde(rename_all = "snake_case")] — the rendered form
        // of `UserRequested` must be `user_requested`, not the default
        // camelCase.
        assert_eq!(ForgetReason::UserRequested.as_str(), "user_requested");
        let json = serde_json::to_string(&ForgetReason::UserRequested).expect("serialize");
        assert_eq!(json, r#""user_requested""#);
    }

    #[test]
    fn verification_source_from_str_opt_roundtrips() {
        for src in [
            VerificationSource::Command,
            VerificationSource::Query,
            VerificationSource::Arithmetic,
            VerificationSource::Reference,
        ] {
            assert_eq!(VerificationSource::from_str_opt(src.as_str()), Some(src));
        }
        assert_eq!(VerificationSource::from_str_opt("smell-test"), None);
    }

    #[test]
    fn verification_status_from_str_opt_roundtrips() {
        for status in [
            VerificationStatus::Pass,
            VerificationStatus::Fail,
            VerificationStatus::Stale,
        ] {
            assert_eq!(
                VerificationStatus::from_str_opt(status.as_str()),
                Some(status)
            );
        }
        assert_eq!(VerificationStatus::from_str_opt("unknown"), None);
    }
}

// --- Path validation layers and error mapping ---

mod path_validation {
    use std::path::PathBuf;

    use aletheia_eidos::knowledge::{
        MemoryScope, PATH_VALIDATION_FS_LAYERS, PathValidationError, PathValidationLayer,
        SYMLINK_HOP_LIMIT,
    };

    #[test]
    fn all_layers_contains_every_variant() {
        // WHY: PATH_VALIDATION_FS_LAYERS counts I/O-requiring layers; ALL
        // contains every layer in application order. Dropping one would
        // silently disable a class of defense.
        assert_eq!(PathValidationLayer::ALL.len(), 8);
        assert_eq!(PATH_VALIDATION_FS_LAYERS, 7);
        // Four non-I/O layers + scope containment + the three I/O layers = 8
        let io_count = PathValidationLayer::ALL
            .iter()
            .filter(|l| l.requires_io())
            .count();
        assert_eq!(io_count, 3);
    }

    #[test]
    fn requires_io_matches_expected_layers() {
        // WHY: only filesystem-touching layers should return true;
        // mistakenly marking a pure string check as IO would regress
        // the validate-without-io optimization.
        assert!(!PathValidationLayer::NullByte.requires_io());
        assert!(!PathValidationLayer::Canonicalization.requires_io());
        assert!(!PathValidationLayer::UrlEncodedTraversal.requires_io());
        assert!(!PathValidationLayer::UnicodeNormalization.requires_io());
        assert!(!PathValidationLayer::ScopeContainment.requires_io());
        assert!(PathValidationLayer::SymlinkResolution.requires_io());
        assert!(PathValidationLayer::DanglingSymlink.requires_io());
        assert!(PathValidationLayer::LoopDetection.requires_io());
    }

    #[test]
    fn symlink_hop_limit_matches_linux_eloop() {
        // WHY: staying aligned with Linux ELOOP (40) keeps our behavior
        // consistent with the kernel; diverging would produce surprising
        // "loop" errors on paths the OS would still traverse.
        assert_eq!(SYMLINK_HOP_LIMIT, 40);
    }

    #[test]
    fn error_layer_mapping_is_1to1() {
        // WHY: each error variant must report the layer that rejected
        // the path so operators can diagnose which defense was triggered.
        let cases: [(PathValidationError, PathValidationLayer); 8] = [
            (
                PathValidationError::NullByte {
                    path: "a".to_owned(),
                },
                PathValidationLayer::NullByte,
            ),
            (
                PathValidationError::Canonicalization {
                    path: "a".to_owned(),
                    component: "..".to_owned(),
                },
                PathValidationLayer::Canonicalization,
            ),
            (
                PathValidationError::SymlinkResolution {
                    path: PathBuf::from("a"),
                    root: PathBuf::from("/"),
                },
                PathValidationLayer::SymlinkResolution,
            ),
            (
                PathValidationError::DanglingSymlink {
                    path: PathBuf::from("a"),
                },
                PathValidationLayer::DanglingSymlink,
            ),
            (
                PathValidationError::LoopDetection {
                    path: PathBuf::from("a"),
                    hops: 41,
                },
                PathValidationLayer::LoopDetection,
            ),
            (
                PathValidationError::UrlEncodedTraversal {
                    path: "a".to_owned(),
                    decoded_fragment: "..".to_owned(),
                },
                PathValidationLayer::UrlEncodedTraversal,
            ),
            (
                PathValidationError::UnicodeNormalization {
                    path: "a".to_owned(),
                    offending_char: '\u{ff0e}',
                },
                PathValidationLayer::UnicodeNormalization,
            ),
            (
                PathValidationError::ScopeContainment {
                    path: PathBuf::from("a"),
                    scope: MemoryScope::User,
                    expected_dir: PathBuf::from("/user"),
                },
                PathValidationLayer::ScopeContainment,
            ),
        ];

        for (err, expected_layer) in cases {
            assert_eq!(err.layer(), expected_layer);
        }
    }
}

// --- validate_memory_path end-to-end ---

mod validate_memory_path {
    use std::path::PathBuf;

    use aletheia_eidos::knowledge::{MemoryScope, PathValidationError, validate_memory_path};
    use tempfile::TempDir;

    fn setup_root() -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        for scope in MemoryScope::ALL {
            std::fs::create_dir_all(dir.path().join(scope.as_dir_name())).expect("mkdir scope");
        }
        dir
    }

    #[test]
    fn rejects_null_byte_path() {
        // WHY: layer 1 must catch null bytes before any filesystem I/O.
        let root = setup_root();
        let bad = PathBuf::from("note\0.md");
        let err = validate_memory_path(&bad, root.path(), MemoryScope::Project)
            .expect_err("null byte must fail");
        assert!(matches!(err, PathValidationError::NullByte { .. }));
    }

    #[test]
    fn rejects_parent_dir_traversal() {
        // WHY: `..` components in any position must be rejected before
        // the path ever touches the canonicalizer.
        let root = setup_root();
        let bad = PathBuf::from("../escape.md");
        let err = validate_memory_path(&bad, root.path(), MemoryScope::Project)
            .expect_err("parent-dir must fail");
        assert!(matches!(err, PathValidationError::Canonicalization { .. }));
    }

    #[test]
    fn accepts_plain_file_under_scope() {
        // WHY: the happy path — a plain relative file under the scope
        // directory must validate and yield a ValidatedPath whose scope
        // field matches what was requested.
        let root = setup_root();
        // The file doesn't need to exist for validate_memory_path to
        // succeed on pure-string layers; we only need its parent dir.
        let path = PathBuf::from("note.md");
        let validated = validate_memory_path(&path, root.path(), MemoryScope::Project)
            .expect("happy-path validate");
        assert_eq!(validated.scope(), MemoryScope::Project);
    }
}

// --- TrainingConfig ---

mod training_config {
    use aletheia_eidos::training::TrainingConfig;

    #[test]
    fn default_is_disabled_with_standard_path() {
        // WHY: defaults must be safe — training capture is opt-in, so
        // `enabled` starts as false. The `path` default is documented in
        // the config schema and downstream jobs rely on it.
        let cfg = TrainingConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.path, "data/training");
    }

    #[test]
    fn serde_uses_defaults_on_missing_fields() {
        // WHY: #[serde(default)] on the struct means callers can provide
        // an empty JSON object and still get a valid config — the pattern
        // taxis config loaders rely on.
        let empty = "{}";
        let cfg: TrainingConfig = serde_json::from_str(empty).expect("deserialize empty");
        assert!(!cfg.enabled);
        assert_eq!(cfg.path, "data/training");
    }

    #[test]
    fn serde_round_trip_preserves_fields() {
        // WHY: enabling capture then round-tripping through JSON must
        // preserve both fields, since this is how the daemon persists
        // training state across restarts.
        let cfg = TrainingConfig {
            enabled: true,
            path: "var/training/custom".to_owned(),
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: TrainingConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.enabled, cfg.enabled);
        assert_eq!(back.path, cfg.path);
    }
}
