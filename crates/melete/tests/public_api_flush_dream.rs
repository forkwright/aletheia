#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "tests index into fixed-size vectors where panic would itself be a failure signal"
)]

// ── MemoryFlush ──

mod memory_flush {
    use melete::flush::{FlushItem, FlushSource, MemoryFlush};

    fn sample_item() -> FlushItem {
        FlushItem {
            content: "Use snafu for errors".to_owned(),
            timestamp: "2026-04-09T00:00:00Z".to_owned(),
            source: FlushSource::Extracted,
        }
    }

    #[test]
    fn round_trips_through_serde_json() {
        let flush = MemoryFlush {
            decisions: vec![sample_item()],
            corrections: vec![FlushItem {
                content: "Incorrect scheme".to_owned(),
                timestamp: "2026-04-09T00:01:00Z".to_owned(),
                source: FlushSource::AgentNote,
            }],
            facts: vec![FlushItem {
                content: "Config lives in taxis".to_owned(),
                timestamp: "2026-04-09T00:02:00Z".to_owned(),
                source: FlushSource::ToolPattern,
            }],
            task_state: Some("writing tests".to_owned()),
        };
        let json = serde_json::to_string(&flush).expect("serialize");
        let restored: MemoryFlush = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.decisions.len(), 1);
        assert_eq!(restored.corrections.len(), 1);
        assert_eq!(restored.facts.len(), 1);
        assert_eq!(restored.task_state.as_deref(), Some("writing tests"));
        assert_eq!(restored.decisions[0].content, "Use snafu for errors");
    }

    #[test]
    fn flush_source_variants_serialize_distinctly() {
        // WHY: FlushSource is #[non_exhaustive] and the wire format
        // distinguishes source provenance for downstream attribution. Two
        // variants must not collapse to the same string.
        let extracted = serde_json::to_string(&FlushSource::Extracted).expect("serialize");
        let agent = serde_json::to_string(&FlushSource::AgentNote).expect("serialize");
        let tool = serde_json::to_string(&FlushSource::ToolPattern).expect("serialize");
        assert_ne!(extracted, agent);
        assert_ne!(extracted, tool);
        assert_ne!(agent, tool);
    }
}

// ── Contradiction / ContradictionLog ──

mod contradiction_log {
    use melete::contradiction::{Contradiction, ContradictionLog, ResolutionStrategy};

    #[test]
    fn empty_constructor_is_empty_and_defaults_to_prefer_newer() {
        let log = ContradictionLog::empty();
        assert!(log.is_empty());
        assert!(log.contradictions.is_empty());
        assert!(matches!(
            log.resolution_strategy,
            ResolutionStrategy::PreferNewer,
        ));
    }

    #[test]
    fn round_trips_through_serde_json_and_reports_not_empty() {
        let original = ContradictionLog {
            contradictions: vec![Contradiction {
                chunk_a: 1,
                chunk_b: 4,
                description: "conflict".to_owned(),
            }],
            timestamp: "2026-04-09T12:00:00Z".to_owned(),
            resolution_strategy: ResolutionStrategy::NeedsUserReview,
        };
        assert!(!original.is_empty());
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: ContradictionLog = serde_json::from_str(&json).expect("deserialize");
        assert!(!restored.is_empty());
        assert_eq!(restored.contradictions.len(), 1);
        assert_eq!(restored.contradictions[0].chunk_a, 1);
        assert_eq!(restored.contradictions[0].chunk_b, 4);
        assert_eq!(restored.contradictions[0].description, "conflict");
        assert_eq!(restored.timestamp, "2026-04-09T12:00:00Z");
        assert!(matches!(
            restored.resolution_strategy,
            ResolutionStrategy::NeedsUserReview,
        ));
    }
}

// ── DreamConfig / DreamEngine / traits ──

mod dream {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use hermeneus::provider::LlmProvider;
    use hermeneus::test_utils::MockProvider;
    use hermeneus::types::{Content, Message, Role};
    use melete::contradiction::ContradictionLog;
    use melete::distill::DistillConfig;
    use melete::dream::{
        ConsolidationTarget, DreamConfig, DreamEngine, MergeReport, SessionTranscript,
        TranscriptSource,
    };
    use melete::flush::MemoryFlush;

    struct NoopSource;
    impl TranscriptSource for NoopSource {
        fn count_sessions_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<usize, std::io::Error> {
            Ok(0)
        }
        fn load_transcripts_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<Vec<SessionTranscript>, std::io::Error> {
            Ok(vec![])
        }
    }

    struct SourceWithFixedCount(usize);
    impl TranscriptSource for SourceWithFixedCount {
        fn count_sessions_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<usize, std::io::Error> {
            Ok(self.0)
        }
        fn load_transcripts_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<Vec<SessionTranscript>, std::io::Error> {
            Ok(vec![])
        }
    }

    struct FixedSource(Vec<SessionTranscript>);
    impl TranscriptSource for FixedSource {
        fn count_sessions_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<usize, std::io::Error> {
            Ok(self.0.len())
        }
        fn load_transcripts_since(
            &self,
            _since: jiff::Timestamp,
        ) -> std::result::Result<Vec<SessionTranscript>, std::io::Error> {
            Ok(self.0.clone())
        }
    }

    struct CountingTarget {
        merges: AtomicUsize,
        stales: AtomicUsize,
    }
    impl CountingTarget {
        fn new() -> Self {
            Self {
                merges: AtomicUsize::new(0),
                stales: AtomicUsize::new(0),
            }
        }
    }
    impl ConsolidationTarget for CountingTarget {
        fn merge_flush(
            &self,
            _flush: &MemoryFlush,
            _nous_id: &str,
        ) -> std::result::Result<MergeReport, std::io::Error> {
            self.merges.fetch_add(1, Ordering::Relaxed);
            Ok(MergeReport {
                facts_added: 3,
                facts_deduped: 1,
                facts_stale: 0,
            })
        }
        fn mark_contradictions_stale(
            &self,
            log: &ContradictionLog,
            _nous_id: &str,
        ) -> std::result::Result<usize, std::io::Error> {
            let n = log.contradictions.len();
            self.stales.fetch_add(n, Ordering::Relaxed);
            Ok(n)
        }
    }

    #[test]
    fn dream_config_new_applies_standard_defaults() {
        // WHY: DreamConfig::new is the documented constructor. Its defaults
        // (24 hours, 5 sessions) appear in operator docs. Changing them
        // silently breaks the runbook.
        let config = DreamConfig::new(PathBuf::from("/tmp/test-lock"));
        assert_eq!(config.min_hours, 24);
        assert_eq!(config.min_sessions, 5);
        assert_eq!(config.scan_interval_secs, 600);
        assert_eq!(config.stale_threshold_secs, 3_600);
        assert_eq!(config.lock_path, PathBuf::from("/tmp/test-lock"));
    }

    #[test]
    fn dream_config_distill_config_defaults_to_distill_config_default() {
        // WHY: the embedded distill config must match DistillConfig::default
        // so the dream pipeline stays in sync with the primary distillation
        // path.
        let config = DreamConfig::new(PathBuf::from("/tmp/test-lock"));
        let default_distill = DistillConfig::default();
        assert_eq!(config.distill_config.model, default_distill.model);
        assert_eq!(
            config.distill_config.verbatim_tail,
            default_distill.verbatim_tail,
        );
    }

    #[test]
    fn merge_report_default_is_zero() {
        let report = MergeReport::default();
        assert_eq!(report.facts_added, 0);
        assert_eq!(report.facts_deduped, 0);
        assert_eq!(report.facts_stale, 0);
    }

    #[test]
    fn transcript_source_is_dyn_compatible() {
        // WHY: TranscriptSource must be object-safe because the dream engine
        // takes `&dyn TranscriptSource`. Losing object safety breaks the
        // entire consolidation pipeline.
        let _s: Box<dyn TranscriptSource> = Box::new(NoopSource);
        let _arc: Arc<dyn TranscriptSource> = Arc::new(SourceWithFixedCount(3));
    }

    #[test]
    fn consolidation_target_is_dyn_compatible() {
        // WHY: ConsolidationTarget must be object-safe for the same reason.
        let _t: Box<dyn ConsolidationTarget> = Box::new(CountingTarget::new());
    }

    #[test]
    fn dream_engine_debug_impl_prints_without_panicking() {
        // WHY: DreamEngine::Debug is hand-written (not derived) and must
        // survive the AtomicI64 field load; a panic here silently poisons
        // diagnostics when operators inspect the engine.
        let engine = DreamEngine::new(DreamConfig::new(PathBuf::from("/tmp/dream-test")));
        let rendered = format!("{engine:?}");
        assert!(rendered.contains("DreamEngine"));
    }

    #[tokio::test]
    async fn on_turn_complete_is_non_blocking_fire_and_forget() {
        // WHY: the documented contract says on_turn_complete returns
        // immediately and spawns the consolidation task in the background.
        // A blocking/slow implementation would stall nous's turn loop.
        let dir = tempfile::tempdir().expect("tempdir");
        let lock_path = dir.path().join(".consolidate-lock");
        let config = DreamConfig {
            min_hours: 0,
            min_sessions: 1,
            lock_path,
            scan_interval_secs: 0,
            stale_threshold_secs: 3_600,
            distill_config: DistillConfig::default(),
        };
        let engine = Arc::new(DreamEngine::new(config));
        let transcript = SessionTranscript {
            session_id: "s1".to_owned(),
            nous_id: "alice".to_owned(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: Content::Text("What's the plan?".to_owned()),
                    cache_breakpoint: false,
                },
                Message {
                    role: Role::Assistant,
                    content: Content::Text(
                        "## Summary\nPlanning next sprint\n## Key Decisions\n- ship tests"
                            .to_owned(),
                    ),
                    cache_breakpoint: false,
                },
            ],
        };
        let source: Arc<dyn TranscriptSource> = Arc::new(FixedSource(vec![transcript]));
        let target: Arc<dyn ConsolidationTarget> = Arc::new(CountingTarget::new());
        let provider: Arc<dyn LlmProvider> = Arc::new(
            MockProvider::new("## Summary\ns\n## Key Decisions\n- done")
                .models(&["claude-sonnet-4-20250514"]),
        );

        // WHY: must return immediately (well under a second). If it blocks
        // on the consolidation pipeline this timing assertion fails.
        let start = std::time::Instant::now();
        engine.on_turn_complete(&source, &target, &provider).await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "on_turn_complete must return quickly, took {elapsed:?}",
        );
    }
}

// ── types re-export ──

mod types_reexport {
    use melete::types::{Content, ContentBlock, Message, Role};

    #[test]
    fn reexports_match_hermeneus_originals() {
        // WHY: melete::types is a convenience re-export. Consumers depend on
        // it being identical to hermeneus, not a wrapper — a type alias
        // that drifted would break generic bounds downstream. The re-export
        // must also include ContentBlock so callers don't need hermeneus
        // in their Cargo.toml.
        let msg: Message = Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
            cache_breakpoint: false,
        };
        let _block = ContentBlock::Text {
            text: "body".to_owned(),
            citations: None,
        };
        assert_eq!(msg.role, Role::User);
    }
}

// ── Send + Sync promises on the engine types ──

mod send_sync_bounds {
    use melete::contradiction::{Contradiction, ContradictionLog};
    use melete::distill::{DistillConfig, DistillEngine, DistillResult};
    use melete::dream::{DreamConfig, DreamEngine, MergeReport, SessionTranscript};
    use melete::flush::{FlushItem, MemoryFlush};
    use melete::similarity::PruningStats;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn core_types_are_send_sync() {
        // WHY: these bounds are load-bearing for the tokio multi-threaded
        // runtime. Downgrading to !Send would break every async caller.
        assert_send_sync::<DistillEngine>();
        assert_send_sync::<DistillConfig>();
        assert_send_sync::<DistillResult>();
        assert_send_sync::<DreamEngine>();
        assert_send_sync::<DreamConfig>();
        assert_send_sync::<MergeReport>();
        assert_send_sync::<SessionTranscript>();
        assert_send_sync::<MemoryFlush>();
        assert_send_sync::<FlushItem>();
        assert_send_sync::<ContradictionLog>();
        assert_send_sync::<Contradiction>();
        assert_send_sync::<PruningStats>();
    }
}
