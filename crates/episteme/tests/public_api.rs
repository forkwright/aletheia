//! Integration tests for aletheia-episteme's public API surface.
//!
//! WHY: episteme had zero `crates/episteme/tests/` integration tests prior
//! to this. The crate has 790+ inline lib tests but they're allowed to
//! reach into `pub(crate)` internals. These tests run against the
//! published API surface only — what kanon, nous, and the steward
//! pipeline can actually consume.
//!
//! Scope (part of #2814 audit): four pure, dependency-free public APIs
//! that don't require the `mneme-engine` cargo feature. LLM-driven
//! extraction, krites-backed queries, and the knowledge store are
//! intentionally out of scope here — those need their own feature-gated
//! suites.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test assertions on fixed-size vectors")]

// ---------------------------------------------------------------------------
// OpsFactExtractor
// ---------------------------------------------------------------------------

mod ops_facts {
    use episteme::knowledge::{EpistemicTier, FactType, MemoryScope};
    use episteme::ops_facts::{OpsFactExtractor, OpsSnapshot};

    /// A complete snapshot where every category of fact should be emitted.
    fn full_snapshot() -> OpsSnapshot {
        OpsSnapshot {
            nous_id: String::from("int-test-nous"),
            active_session_count: 4,
            tool_call_total: 50,
            tool_call_successes: 47,
            error_count: 1,
            avg_task_latency_ms: 120,
            task_sample_count: 8,
        }
    }

    #[test]
    fn full_snapshot_produces_all_four_fact_categories() {
        // WHY: a snapshot with tool_call_total >= MIN_TOOL_CALLS (5) and
        // task_sample_count > 0 must emit every category — sessions,
        // tool success rate, error count, task latency — so the knowledge
        // graph gets the full picture of system health at this tick.
        let facts = OpsFactExtractor::extract(&full_snapshot(), episteme::ops_facts::DEFAULT_MIN_TOOL_CALLS).expect("extraction");
        assert_eq!(facts.len(), 4, "full snapshot should yield 4 facts");

        let contents: Vec<&str> = facts.iter().map(|f| f.fact.content.as_str()).collect();
        assert!(contents.iter().any(|c| c.contains("active sessions: 4")));
        assert!(contents.iter().any(|c| c.contains("tool success rate")));
        assert!(contents.iter().any(|c| c.contains("error count: 1")));
        assert!(contents.iter().any(|c| c.contains("avg task latency: 120ms")));
    }

    #[test]
    fn every_fact_carries_standard_metadata() {
        // WHY: downstream consumers rely on ops facts having a stable
        // shape — operational fact type, project scope, inferred tier,
        // non-empty nous_id. Drift here would silently break dashboards.
        let facts = OpsFactExtractor::extract(&full_snapshot(), episteme::ops_facts::DEFAULT_MIN_TOOL_CALLS).expect("extraction");
        for ops_fact in &facts {
            let f = &ops_fact.fact;
            assert_eq!(f.fact_type, "operational");
            assert_eq!(f.nous_id, "int-test-nous");
            assert_eq!(f.scope, Some(MemoryScope::Project));
            assert_eq!(f.provenance.tier, EpistemicTier::Inferred);
            assert!(
                (0.0..=1.0).contains(&f.provenance.confidence),
                "confidence out of range: {}",
                f.provenance.confidence
            );
            assert!(
                (f.provenance.stability_hours - FactType::Operational.base_stability_hours())
                    .abs()
                    < f64::EPSILON,
                "operational facts must inherit the operational stability window"
            );
        }
    }

    #[test]
    fn insufficient_tool_calls_skips_success_rate() {
        // WHY: below MIN_TOOL_CALLS the ratio is noise, so the extractor
        // deliberately omits the tool rate fact. Sessions + errors remain.
        let snapshot = OpsSnapshot {
            nous_id: String::from("int-test-nous"),
            active_session_count: 1,
            tool_call_total: 3,
            tool_call_successes: 3,
            error_count: 0,
            avg_task_latency_ms: 0,
            task_sample_count: 0,
        };
        let facts = OpsFactExtractor::extract(&snapshot, episteme::ops_facts::DEFAULT_MIN_TOOL_CALLS).expect("extraction");
        let contents: Vec<&str> = facts.iter().map(|f| f.fact.content.as_str()).collect();
        assert_eq!(facts.len(), 2, "should have only sessions + errors");
        assert!(
            !contents.iter().any(|c| c.contains("tool success rate")),
            "tool success rate should be omitted below MIN_TOOL_CALLS"
        );
    }

    #[test]
    fn zero_task_samples_skips_latency_fact() {
        // WHY: avg_task_latency_ms is meaningless when no tasks ran.
        // The extractor signals this by omitting the fact entirely
        // rather than emitting a zero-confidence latency record.
        let snapshot = OpsSnapshot {
            nous_id: String::from("int-test-nous"),
            active_session_count: 0,
            tool_call_total: 10,
            tool_call_successes: 10,
            error_count: 0,
            avg_task_latency_ms: 0,
            task_sample_count: 0,
        };
        let facts = OpsFactExtractor::extract(&snapshot, episteme::ops_facts::DEFAULT_MIN_TOOL_CALLS).expect("extraction");
        let contents: Vec<&str> = facts.iter().map(|f| f.fact.content.as_str()).collect();
        assert!(
            !contents.iter().any(|c| c.contains("avg task latency")),
            "latency fact should be skipped when task_sample_count is 0"
        );
    }

    #[test]
    fn default_snapshot_yields_baseline_facts() {
        // WHY: an empty snapshot still emits sessions + errors so the
        // knowledge graph always has *some* heartbeat signal — a missing
        // baseline fact would be indistinguishable from a broken extractor.
        let snapshot = OpsSnapshot {
            nous_id: String::from("int-test-nous"),
            ..Default::default()
        };
        let facts = OpsFactExtractor::extract(&snapshot, episteme::ops_facts::DEFAULT_MIN_TOOL_CALLS).expect("extraction");
        assert_eq!(facts.len(), 2, "baseline = sessions + errors");
    }

    #[test]
    fn each_fact_gets_a_unique_id() {
        // WHY: fact IDs collide across categories would cause upserts to
        // overwrite each other in the knowledge store. Each category must
        // mint its own ULID-suffixed id.
        let facts = OpsFactExtractor::extract(&full_snapshot(), episteme::ops_facts::DEFAULT_MIN_TOOL_CALLS).expect("extraction");
        let ids: std::collections::HashSet<_> =
            facts.iter().map(|f| f.fact.id.as_str().to_owned()).collect();
        assert_eq!(ids.len(), facts.len(), "fact ids must be unique per snapshot");
    }

    #[test]
    fn confidence_drops_when_error_count_is_high() {
        // WHY: error_count directly drives the confidence of the
        // ops.error_count fact — more errors = lower confidence in
        // system health. This is load-bearing for the steward's
        // "system is healthy" inference.
        let low_err = OpsSnapshot {
            nous_id: String::from("int-test-nous"),
            error_count: 0,
            ..Default::default()
        };
        let high_err = OpsSnapshot {
            nous_id: String::from("int-test-nous"),
            error_count: 100,
            ..Default::default()
        };
        let low_facts = OpsFactExtractor::extract(&low_err, episteme::ops_facts::DEFAULT_MIN_TOOL_CALLS).expect("low");
        let high_facts = OpsFactExtractor::extract(&high_err, episteme::ops_facts::DEFAULT_MIN_TOOL_CALLS).expect("high");
        let low_conf = low_facts
            .iter()
            .find(|f| f.fact.content.starts_with("error count"))
            .expect("error fact")
            .fact
            .provenance
            .confidence;
        let high_conf = high_facts
            .iter()
            .find(|f| f.fact.content.starts_with("error count"))
            .expect("error fact")
            .fact
            .provenance
            .confidence;
        assert!(
            low_conf > high_conf,
            "0 errors ({low_conf}) should score higher confidence than 100 errors ({high_conf})"
        );
    }
}

// ---------------------------------------------------------------------------
// ObservationType::classify
// ---------------------------------------------------------------------------

mod observation_type {
    use episteme::extract::observation::ObservationType;

    #[test]
    fn bug_keywords_take_priority_over_debt() {
        // WHY: docs guarantee Bug > MissingTest > DocGap > Debt > Idea.
        // A single sentence with both a bug marker ("crash") and a debt
        // marker ("refactor") must resolve to Bug — mis-classifying a
        // bug as debt delays urgent fixes.
        assert_eq!(
            ObservationType::classify("refactor the parser — it crashes on empty input"),
            ObservationType::Bug
        );
    }

    #[test]
    fn classify_is_case_insensitive() {
        // WHY: PR descriptions freely mix case. A sentence with "PANIC"
        // in uppercase must still classify as Bug.
        assert_eq!(
            ObservationType::classify("PANIC observed in the dispatch worker"),
            ObservationType::Bug
        );
    }

    #[test]
    fn missing_test_wins_over_debt() {
        // WHY: "add tests for the legacy parser" contains "legacy" (debt)
        // and "add test" (missing test). Priority order says MissingTest.
        assert_eq!(
            ObservationType::classify("add tests for the legacy parser"),
            ObservationType::MissingTest
        );
    }

    #[test]
    fn doc_gap_wins_over_debt() {
        // WHY: "undocumented deprecated helper" matches both DocGap
        // ("undocumented") and Debt ("deprecated"). DocGap should win
        // per the declared priority order.
        assert_eq!(
            ObservationType::classify("undocumented deprecated helper in legacy code"),
            ObservationType::DocGap
        );
    }

    #[test]
    fn unclassified_text_defaults_to_idea() {
        // WHY: everything without a known keyword is a forward-looking
        // suggestion. Default-to-Idea keeps the classifier total.
        assert_eq!(
            ObservationType::classify("we should extract a reusable builder here"),
            ObservationType::Idea
        );
    }

    #[test]
    fn from_str_lossy_roundtrips_all_known_values() {
        for ty in [
            ObservationType::Bug,
            ObservationType::Debt,
            ObservationType::Idea,
            ObservationType::MissingTest,
            ObservationType::DocGap,
        ] {
            let round_tripped = ObservationType::from_str_lossy(ty.as_str());
            assert_eq!(round_tripped, ty, "roundtrip failed for {ty}");
        }
    }

    #[test]
    fn from_str_lossy_maps_unknown_to_idea() {
        // WHY: `from_str_lossy` is lossy by design — unknown tags from
        // old database rows default to Idea rather than erroring.
        assert_eq!(
            ObservationType::from_str_lossy("not-a-real-type"),
            ObservationType::Idea
        );
        assert_eq!(ObservationType::from_str_lossy(""), ObservationType::Idea);
    }

    #[test]
    fn display_matches_as_str() {
        for ty in [
            ObservationType::Bug,
            ObservationType::Debt,
            ObservationType::Idea,
            ObservationType::MissingTest,
            ObservationType::DocGap,
        ] {
            assert_eq!(ty.to_string(), ty.as_str());
        }
    }
}

// ---------------------------------------------------------------------------
// CausalStore (causal edge graph)
// ---------------------------------------------------------------------------

mod causal_store {
    use episteme::causal::{CausalError, CausalStore};
    use episteme::id::{CausalEdgeId, FactId};
    use episteme::knowledge::{CausalEdge, CausalRelationType, TemporalOrdering};

    fn fact_id(s: &str) -> FactId {
        FactId::new(s).expect("valid fact id")
    }

    fn edge_id(s: &str) -> CausalEdgeId {
        CausalEdgeId::new(s).expect("valid edge id")
    }

    /// Build a simple cause→effect edge for testing.
    fn make_edge(id: &str, src: &str, tgt: &str, confidence: f64) -> CausalEdge {
        CausalEdge {
            id: edge_id(id),
            source_id: fact_id(src),
            target_id: fact_id(tgt),
            relationship_type: CausalRelationType::Caused,
            ordering: TemporalOrdering::Before,
            confidence,
            evidence_session_id: None,
            timestamp: jiff::Timestamp::now(),
        }
    }

    #[test]
    fn new_store_is_empty() {
        let store = CausalStore::new();
        assert_eq!(store.all_edges().count(), 0);
        assert!(store.get_edge(&edge_id("never-inserted")).is_none());
    }

    #[test]
    fn add_edge_then_get_edge_round_trips() {
        let mut store = CausalStore::new();
        let edge = make_edge("e1", "fact-a", "fact-b", 0.75);
        store.add_edge(edge).expect("first insert ok");

        let retrieved = store.get_edge(&edge_id("e1")).expect("edge exists");
        assert_eq!(retrieved.source_id, fact_id("fact-a"));
        assert_eq!(retrieved.target_id, fact_id("fact-b"));
        assert!((retrieved.confidence - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn add_edge_rejects_duplicate_ids() {
        // WHY: duplicate insertions must fail loudly. Silently overwriting
        // would let bad extractions clobber good ones.
        let mut store = CausalStore::new();
        let first = make_edge("e1", "fact-a", "fact-b", 0.8);
        let dup = make_edge("e1", "fact-c", "fact-d", 0.4);
        store.add_edge(first).expect("first ok");

        let err = store.add_edge(dup).expect_err("duplicate must error");
        assert!(
            matches!(err, CausalError::DuplicateEdge { .. }),
            "expected DuplicateEdge, got {err:?}"
        );
    }

    #[test]
    fn all_edges_yields_every_inserted_edge() {
        let mut store = CausalStore::new();
        store
            .add_edge(make_edge("e1", "fact-a", "fact-b", 0.6))
            .expect("e1");
        store
            .add_edge(make_edge("e2", "fact-b", "fact-c", 0.7))
            .expect("e2");
        store
            .add_edge(make_edge("e3", "fact-c", "fact-d", 0.8))
            .expect("e3");

        let ids: std::collections::HashSet<_> =
            store.all_edges().map(|e| e.id.as_str().to_owned()).collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains("e1"));
        assert!(ids.contains("e2"));
        assert!(ids.contains("e3"));
    }

    #[test]
    fn direct_causes_returns_only_inbound_edges() {
        let mut store = CausalStore::new();
        // fact-a and fact-b both cause fact-c; fact-c also causes fact-d.
        store
            .add_edge(make_edge("e1", "fact-a", "fact-c", 0.8))
            .expect("e1");
        store
            .add_edge(make_edge("e2", "fact-b", "fact-c", 0.6))
            .expect("e2");
        store
            .add_edge(make_edge("e3", "fact-c", "fact-d", 0.7))
            .expect("e3");

        let causes = store.direct_causes(&fact_id("fact-c"));
        assert_eq!(causes.len(), 2, "fact-c has two direct causes");
        let sources: std::collections::HashSet<_> =
            causes.iter().map(|e| e.source_id.as_str()).collect();
        assert!(sources.contains("fact-a"));
        assert!(sources.contains("fact-b"));
    }

    #[test]
    fn direct_effects_returns_only_outbound_edges() {
        let mut store = CausalStore::new();
        store
            .add_edge(make_edge("e1", "fact-a", "fact-b", 0.7))
            .expect("e1");
        store
            .add_edge(make_edge("e2", "fact-a", "fact-c", 0.6))
            .expect("e2");
        // Unrelated: fact-x → fact-y, should NOT appear in fact-a's effects.
        store
            .add_edge(make_edge("e3", "fact-x", "fact-y", 0.5))
            .expect("e3");

        let effects = store.direct_effects(&fact_id("fact-a"));
        assert_eq!(effects.len(), 2);
        let targets: std::collections::HashSet<_> =
            effects.iter().map(|e| e.target_id.as_str()).collect();
        assert!(targets.contains("fact-b"));
        assert!(targets.contains("fact-c"));
        assert!(!targets.contains("fact-y"));
    }

    #[test]
    fn trace_causes_includes_root_and_confidence_compounds() {
        // WHY: trace_causes must walk backwards, product-compound the
        // edge confidences, and include the starting fact as a root
        // with confidence 1.0 and no via_edge. That contract is
        // load-bearing for the recall pipeline's chain-confidence math.
        let mut store = CausalStore::new();
        // chain: fact-a --0.8--> fact-b --0.5--> fact-c
        store
            .add_edge(make_edge("e1", "fact-a", "fact-b", 0.8))
            .expect("e1");
        store
            .add_edge(make_edge("e2", "fact-b", "fact-c", 0.5))
            .expect("e2");

        let chain = store.trace_causes(&fact_id("fact-c"));
        // Root + 2 ancestors = 3 nodes.
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].fact_id, fact_id("fact-c"));
        assert!(chain[0].via_edge.is_none(), "root has no incoming edge");
        assert!((chain[0].chain_confidence - 1.0).abs() < f64::EPSILON);

        // The deepest ancestor should have chain_confidence = 0.8 * 0.5 = 0.4.
        let deepest = chain
            .iter()
            .find(|n| n.fact_id == fact_id("fact-a"))
            .expect("fact-a in chain");
        assert!(
            (deepest.chain_confidence - 0.4).abs() < 1e-9,
            "expected 0.4, got {}",
            deepest.chain_confidence
        );
        assert_eq!(deepest.depth, 2);
    }

    #[test]
    fn trace_effects_terminates_on_cycles() {
        // WHY: heuristically extracted edges can form loops. The
        // traversal must stop at visited nodes instead of blowing the
        // stack. Verify termination and a reasonable node count.
        let mut store = CausalStore::new();
        store
            .add_edge(make_edge("e1", "fact-a", "fact-b", 0.7))
            .expect("e1");
        store
            .add_edge(make_edge("e2", "fact-b", "fact-a", 0.7))
            .expect("e2");

        let chain = store.trace_effects(&fact_id("fact-a"));
        assert!(
            chain.len() <= 3,
            "cycle traversal must not revisit; got {} nodes",
            chain.len()
        );
    }

    #[test]
    fn trace_causes_on_isolated_fact_returns_root_only() {
        // WHY: if the fact has no inbound edges the traversal still
        // returns a single-node chain (the root itself), not an empty
        // Vec — callers rely on "at least the root" as an invariant.
        let store = CausalStore::new();
        let chain = store.trace_causes(&fact_id("lonely-fact"));
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].fact_id, fact_id("lonely-fact"));
        assert_eq!(chain[0].depth, 0);
    }

    #[test]
    fn direct_causes_on_unknown_fact_is_empty() {
        let store = CausalStore::new();
        assert!(store.direct_causes(&fact_id("unknown")).is_empty());
        assert!(store.direct_effects(&fact_id("unknown")).is_empty());
    }
}

// ---------------------------------------------------------------------------
// skill::parse_skill_md
// ---------------------------------------------------------------------------

mod parse_skill_md {
    use episteme::skill::{SkillContent, parse_skill_md};

    #[test]
    fn parses_minimal_skill_md() {
        let src = "# Rust Error Handling\n\
                   Concise description of what this skill does.\n";
        let skill = parse_skill_md(src, "rust-error-handling").expect("minimal parse ok");
        assert_eq!(skill.name, "rust-error-handling");
        assert!(skill.description.contains("Concise description"));
        assert!(skill.steps.is_empty(), "no Steps section => empty steps");
        assert!(skill.tools_used.is_empty());
        // WHY: when no `domains:` frontmatter is present, tags are derived
        // from the slug by splitting on hyphens.
        assert_eq!(skill.domain_tags, vec!["rust", "error", "handling"]);
        assert_eq!(skill.origin, "seeded");
    }

    #[test]
    fn parses_yaml_frontmatter_tools_and_domains() {
        // WHY: frontmatter is the canonical place to declare tools and
        // domains — it must override slug-derived defaults.
        let src = "---\n\
                   tools: [Read, Bash, Edit]\n\
                   domains: [rust, testing]\n\
                   ---\n\
                   \n\
                   # Sample Skill\n\
                   A sample skill with frontmatter.\n";
        let skill = parse_skill_md(src, "sample-skill").expect("parse ok");
        assert_eq!(skill.tools_used, vec!["Read", "Bash", "Edit"]);
        assert_eq!(skill.domain_tags, vec!["rust", "testing"]);
    }

    #[test]
    fn extracts_numbered_steps_section() {
        let src = "# Skill\n\
                   A description.\n\
                   \n\
                   ## Steps\n\
                   1. First step\n\
                   2. Second step\n\
                   3. Third step\n";
        let skill = parse_skill_md(src, "skill").expect("parse ok");
        assert_eq!(skill.steps, vec!["First step", "Second step", "Third step"]);
    }

    #[test]
    fn extracts_tools_used_section_without_frontmatter() {
        // WHY: when no frontmatter tools are present, parse_skill_md falls
        // back to reading the "## Tools Used" section for `- Name: desc`
        // lines. The colon is optional.
        let src = "# Skill\n\
                   A description.\n\
                   \n\
                   ## Tools Used\n\
                   - Read: read files\n\
                   - Bash: run commands\n";
        let skill = parse_skill_md(src, "skill").expect("parse ok");
        assert_eq!(skill.tools_used, vec!["Read", "Bash"]);
    }

    #[test]
    fn description_falls_back_to_when_to_use_section() {
        // WHY: documented fallback — if the body after the title has no
        // free-text description, the "## When to Use" section provides it.
        let src = "# Skill\n\
                   \n\
                   ## When to Use\n\
                   When you need to do X.\n";
        let skill = parse_skill_md(src, "skill").expect("parse ok");
        assert!(
            skill.description.contains("When you need to do X"),
            "description should fall back to 'When to Use' content"
        );
    }

    #[test]
    fn rejects_empty_document() {
        let err = parse_skill_md("", "my-skill").expect_err("empty must error");
        assert_eq!(err.path, "my-skill");
        assert!(
            err.reason.contains("empty"),
            "expected 'empty document' reason, got: {}",
            err.reason
        );
    }

    #[test]
    fn rejects_document_without_top_level_heading() {
        // WHY: SKILL.md *must* begin with a level-1 heading; we're strict
        // about this so the exporter's format round-trips.
        let src = "No heading here.\nJust free text.\n";
        let err = parse_skill_md(src, "slug").expect_err("missing heading errors");
        assert!(
            err.reason.contains("heading"),
            "expected heading reason, got: {}",
            err.reason
        );
    }

    #[test]
    fn rejects_document_with_no_description() {
        // WHY: a title alone isn't enough — parse_skill_md requires either
        // free-text body or a "When to Use" section.
        let src = "# Title Only\n\n## Steps\n1. Do a thing\n";
        let err = parse_skill_md(src, "slug").expect_err("missing description errors");
        assert!(
            err.reason.contains("description"),
            "expected description reason, got: {}",
            err.reason
        );
    }

    #[test]
    fn skill_content_is_fully_public() {
        // WHY: downstream crates construct SkillContent directly (e.g. to
        // feed the exporter). This test pins the struct's field list as a
        // compile-time contract so field removals show up as test
        // breakages, not surprise breakage in callers.
        let sc = SkillContent {
            name: "n".to_owned(),
            description: "d".to_owned(),
            steps: vec!["s".to_owned()],
            tools_used: vec!["t".to_owned()],
            domain_tags: vec!["tag".to_owned()],
            origin: "manual".to_owned(),
        };
        assert_eq!(sc.name, "n");
        assert_eq!(sc.steps.len(), 1);
    }
}
