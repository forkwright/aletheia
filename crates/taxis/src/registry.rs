//! Static parameter registry: metadata for every tunable constant.
//!
//! Each [`ParameterSpec`] describes one knob in the configuration surface:
//! what it controls, its valid range, whether agents can self-tune it, and
//! what outcome signal a tuning loop should optimise for.

use std::sync::LazyLock;

/// Classification of who should tune a parameter and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ParameterTier {
    /// Operator sets at deployment time; not self-tunable.
    Deployment,
    /// Operator or agent may override per-agent via config.
    PerAgent,
    /// Eligible for automated tuning by the self-tuning loop.
    SelfTuning,
}

impl std::fmt::Display for ParameterTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deployment => f.write_str("deployment"),
            Self::PerAgent => f.write_str("per-agent"),
            Self::SelfTuning => f.write_str("self-tuning"),
        }
    }
}

/// Hint for which direction improves the outcome signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TuningDirection {
    /// Increasing the value generally improves the outcome signal.
    Higher,
    /// Decreasing the value generally improves the outcome signal.
    Lower,
    /// Optimal direction depends on deployment context.
    Contextual,
}

impl std::fmt::Display for TuningDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Higher => f.write_str("higher"),
            Self::Lower => f.write_str("lower"),
            Self::Contextual => f.write_str("contextual"),
        }
    }
}

/// A typed default value for a parameter.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum ParameterValue {
    /// Integer parameter.
    Int(i64),
    /// Floating-point parameter.
    Float(f64),
    /// Boolean toggle.
    Bool(bool),
    /// Duration in the unit described by the parameter key (seconds, milliseconds, etc.).
    Duration(u64),
}

impl std::fmt::Display for ParameterValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Bool(v) => write!(f, "{v}"),
            Self::Duration(v) => write!(f, "{v}"),
        }
    }
}

/// Metadata for a single tunable parameter.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParameterSpec {
    /// Dotted config key (e.g. `"agents.defaults.behavior.distillationContextTokenTrigger"`).
    pub key: &'static str,
    /// Config section this parameter lives in.
    pub section: &'static str,
    /// Who should tune this parameter.
    pub tier: ParameterTier,
    /// Default value compiled into the binary.
    pub default: ParameterValue,
    /// Optional `(min, max)` numeric bounds.
    pub bounds: Option<(f64, f64)>,
    /// Whether the parameter can be changed without restarting.
    pub hot_reloadable: bool,
    /// Human-readable description.
    pub description: &'static str,
    /// Which subsystem behavior this parameter affects.
    pub affects: &'static str,
    /// Outcome signal that a tuning loop should optimise for.
    pub outcome_signal: &'static str,
    /// What evidence is needed before changing this parameter.
    pub evidence_required: &'static str,
    /// Hint for which direction improves the outcome signal.
    pub direction_hint: TuningDirection,
}

/// Return every registered parameter spec.
#[must_use]
pub fn all_specs() -> &'static [ParameterSpec] {
    &REGISTRY
}

/// Return specs whose `section` matches `section`.
#[must_use]
pub fn specs_by_section(section: &str) -> Vec<&'static ParameterSpec> {
    REGISTRY.iter().filter(|s| s.section == section).collect()
}

/// Return specs whose `affects` field contains `outcome`.
#[must_use]
pub fn specs_affecting(outcome: &str) -> Vec<&'static ParameterSpec> {
    REGISTRY.iter().filter(|s| s.affects.contains(outcome)).collect()
}

/// Look up a single spec by its dotted key.
#[must_use]
pub fn spec_by_key(key: &str) -> Option<&'static ParameterSpec> {
    REGISTRY.iter().find(|s| s.key == key)
}

// ---------------------------------------------------------------------------
// Static registry — populated at first access via LazyLock
// ---------------------------------------------------------------------------

static REGISTRY: LazyLock<Vec<ParameterSpec>> = LazyLock::new(build_registry);

// NOTE(#2306): 769 lines: the registry is a single data declaration — one Vec literal
// of ParameterSpec entries. Splitting it into separate functions would scatter the
// registry across files without improving readability. The function is pure data.
#[expect(
    clippy::too_many_lines,
    reason = "static data declaration: one Vec literal of 50+ ParameterSpec entries"
)]
fn build_registry() -> Vec<ParameterSpec> {
    vec![
        // ===================================================================
        // Distillation
        // ===================================================================
        ParameterSpec {
            key: "agents.defaults.behavior.distillationContextTokenTrigger",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Int(120_000),
            bounds: Some((10_000.0, 500_000.0)),
            hot_reloadable: true,
            description: "Context token count that triggers automatic distillation",
            affects: "distillation_frequency",
            outcome_signal: "turn_quality_post_distillation",
            evidence_required: "A/B comparison of turn quality and cost before/after threshold change",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.distillationMessageCountTrigger",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Int(150),
            bounds: Some((10.0, 1000.0)),
            hot_reloadable: true,
            description: "Message count that triggers distillation",
            affects: "distillation_frequency",
            outcome_signal: "turn_quality_post_distillation",
            evidence_required: "Session length distribution analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.distillationStaleSessionDays",
            section: "agents.defaults.behavior",
            tier: ParameterTier::PerAgent,
            default: ParameterValue::Duration(7),
            bounds: Some((1.0, 90.0)),
            hot_reloadable: true,
            description: "Days idle before a session is considered stale for distillation",
            affects: "distillation_frequency",
            outcome_signal: "stale_session_cleanup_rate",
            evidence_required: "Session idle time distribution",
            direction_hint: TuningDirection::Lower,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.distillationStaleMinMessages",
            section: "agents.defaults.behavior",
            tier: ParameterTier::PerAgent,
            default: ParameterValue::Int(20),
            bounds: Some((1.0, 200.0)),
            hot_reloadable: true,
            description: "Minimum messages required for stale-session distillation",
            affects: "distillation_frequency",
            outcome_signal: "stale_session_cleanup_rate",
            evidence_required: "Session length distribution",
            direction_hint: TuningDirection::Lower,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.distillationNeverDistilledTrigger",
            section: "agents.defaults.behavior",
            tier: ParameterTier::PerAgent,
            default: ParameterValue::Int(30),
            bounds: Some((5.0, 500.0)),
            hot_reloadable: true,
            description: "Message count trigger for sessions never distilled",
            affects: "distillation_frequency",
            outcome_signal: "first_distillation_quality",
            evidence_required: "First-distillation quality metrics",
            direction_hint: TuningDirection::Lower,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.distillationMaxBackoffTurns",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Int(8),
            bounds: Some((1.0, 50.0)),
            hot_reloadable: true,
            description: "Maximum backoff turns before distillation is forced",
            affects: "distillation_frequency",
            outcome_signal: "distillation_backoff_effectiveness",
            evidence_required: "Backoff-vs-force distillation quality comparison",
            direction_hint: TuningDirection::Higher,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.distillationMaxToolResultLen",
            section: "agents.defaults.behavior",
            tier: ParameterTier::PerAgent,
            default: ParameterValue::Int(500),
            bounds: Some((50.0, 5000.0)),
            hot_reloadable: true,
            description: "Maximum character length for truncated tool results in distillation prompts",
            affects: "distillation_quality",
            outcome_signal: "distillation_summary_fidelity",
            evidence_required: "Comparison of distillation accuracy at different truncation points",
            direction_hint: TuningDirection::Higher,
        },

        // ===================================================================
        // Competence scoring
        // ===================================================================
        ParameterSpec {
            key: "agents.defaults.behavior.competenceCorrectionPenalty",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.05),
            bounds: Some((0.001, 0.5)),
            hot_reloadable: true,
            description: "Competence score penalty per correction",
            affects: "competence_scoring",
            outcome_signal: "correction_rate_trend",
            evidence_required: "Correction frequency and severity over 100+ turns",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.competenceSuccessBonus",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.02),
            bounds: Some((0.001, 0.2)),
            hot_reloadable: true,
            description: "Competence score bonus per successful turn",
            affects: "competence_scoring",
            outcome_signal: "competence_score_stability",
            evidence_required: "Score trajectory analysis over 200+ turns",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.competenceDisagreementPenalty",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.01),
            bounds: Some((0.001, 0.2)),
            hot_reloadable: true,
            description: "Competence score penalty per user disagreement",
            affects: "competence_scoring",
            outcome_signal: "user_satisfaction_correlation",
            evidence_required: "User feedback correlation analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.competenceMinScore",
            section: "agents.defaults.behavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Float(0.1),
            bounds: Some((0.0, 0.5)),
            hot_reloadable: true,
            description: "Competence score floor",
            affects: "competence_scoring",
            outcome_signal: "recovery_from_low_competence",
            evidence_required: "Agent recovery behavior at minimum score",
            direction_hint: TuningDirection::Lower,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.competenceMaxScore",
            section: "agents.defaults.behavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Float(0.95),
            bounds: Some((0.5, 1.0)),
            hot_reloadable: true,
            description: "Competence score ceiling",
            affects: "competence_scoring",
            outcome_signal: "score_distribution_shape",
            evidence_required: "Long-term score distribution analysis",
            direction_hint: TuningDirection::Higher,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.competenceDefaultScore",
            section: "agents.defaults.behavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Float(0.5),
            bounds: Some((0.1, 0.95)),
            hot_reloadable: true,
            description: "Initial competence score for a new agent",
            affects: "competence_scoring",
            outcome_signal: "new_agent_calibration_speed",
            evidence_required: "Time-to-accurate-score for new agents",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.competenceEscalationFailureThreshold",
            section: "agents.defaults.behavior",
            tier: ParameterTier::PerAgent,
            default: ParameterValue::Float(0.30),
            bounds: Some((0.05, 0.8)),
            hot_reloadable: true,
            description: "Competence score below which escalation fires",
            affects: "competence_scoring",
            outcome_signal: "escalation_precision",
            evidence_required: "False-positive and false-negative escalation rates",
            direction_hint: TuningDirection::Contextual,
        },

        // ===================================================================
        // Knowledge — conflict resolution
        // ===================================================================
        ParameterSpec {
            key: "knowledge.conflictIntraBatchDedupThreshold",
            section: "knowledge",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.95),
            bounds: Some((0.5, 1.0)),
            hot_reloadable: true,
            description: "Similarity threshold above which intra-batch candidates are merged",
            affects: "knowledge_dedup",
            outcome_signal: "duplicate_fact_rate",
            evidence_required: "Duplicate fact rate at different thresholds",
            direction_hint: TuningDirection::Higher,
        },
        ParameterSpec {
            key: "knowledge.conflictCandidateDistanceThreshold",
            section: "knowledge",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.28),
            bounds: Some((0.05, 0.8)),
            hot_reloadable: true,
            description: "Maximum vector distance for a fact to be a conflict candidate",
            affects: "knowledge_conflict_resolution",
            outcome_signal: "conflict_detection_recall",
            evidence_required: "Conflict detection recall/precision at different thresholds",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "knowledge.conflictMaxCandidates",
            section: "knowledge",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(5),
            bounds: Some((1.0, 50.0)),
            hot_reloadable: true,
            description: "Maximum conflict candidates evaluated per fact",
            affects: "knowledge_conflict_resolution",
            outcome_signal: "conflict_resolution_latency",
            evidence_required: "Latency and accuracy tradeoff at different candidate counts",
            direction_hint: TuningDirection::Higher,
        },
        ParameterSpec {
            key: "knowledge.decayReinforcementBoost",
            section: "knowledge",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.02),
            bounds: Some((0.001, 0.2)),
            hot_reloadable: true,
            description: "Confidence boost per reinforcement event",
            affects: "knowledge_confidence",
            outcome_signal: "fact_confidence_calibration",
            evidence_required: "Confidence vs. accuracy correlation analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "knowledge.decayCrossAgentBonusPerAgent",
            section: "knowledge",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.15),
            bounds: Some((0.0, 0.5)),
            hot_reloadable: true,
            description: "Confidence bonus per additional corroborating agent",
            affects: "knowledge_confidence",
            outcome_signal: "cross_agent_fact_accuracy",
            evidence_required: "Multi-agent corroboration accuracy metrics",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "knowledge.decayMaxCrossAgentMultiplier",
            section: "knowledge",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Float(1.75),
            bounds: Some((1.0, 5.0)),
            hot_reloadable: true,
            description: "Cap on total cross-agent confidence multiplier",
            affects: "knowledge_confidence",
            outcome_signal: "cross_agent_fact_accuracy",
            evidence_required: "Multi-agent confidence distribution analysis",
            direction_hint: TuningDirection::Contextual,
        },

        // ===================================================================
        // Knowledge — dedup weights
        // ===================================================================
        ParameterSpec {
            key: "agents.defaults.behavior.knowledgeDedupWeightName",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.4),
            bounds: Some((0.0, 1.0)),
            hot_reloadable: true,
            description: "Weight of name similarity in dedup scoring",
            affects: "knowledge_dedup",
            outcome_signal: "dedup_precision_recall",
            evidence_required: "Dedup accuracy at different weight combinations",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.knowledgeDedupWeightEmbed",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.3),
            bounds: Some((0.0, 1.0)),
            hot_reloadable: true,
            description: "Weight of embedding similarity in dedup scoring",
            affects: "knowledge_dedup",
            outcome_signal: "dedup_precision_recall",
            evidence_required: "Dedup accuracy at different weight combinations",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.knowledgeDedupJwThreshold",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.85),
            bounds: Some((0.5, 1.0)),
            hot_reloadable: true,
            description: "Jaro-Winkler score above which strings are considered similar",
            affects: "knowledge_dedup",
            outcome_signal: "dedup_precision_recall",
            evidence_required: "Name-match accuracy at different JW thresholds",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.knowledgeDedupEmbedThreshold",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.80),
            bounds: Some((0.5, 1.0)),
            hot_reloadable: true,
            description: "Cosine similarity above which embeddings are considered similar",
            affects: "knowledge_dedup",
            outcome_signal: "dedup_precision_recall",
            evidence_required: "Embedding-match accuracy at different thresholds",
            direction_hint: TuningDirection::Contextual,
        },

        // ===================================================================
        // Knowledge — fact lifecycle thresholds
        // ===================================================================
        ParameterSpec {
            key: "agents.defaults.behavior.factActiveThreshold",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.7),
            bounds: Some((0.3, 1.0)),
            hot_reloadable: true,
            description: "Confidence above which a fact is considered Active",
            affects: "knowledge_fact_lifecycle",
            outcome_signal: "fact_stage_distribution",
            evidence_required: "Fact lifecycle stage distribution analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.factFadingThreshold",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.3),
            bounds: Some((0.05, 0.7)),
            hot_reloadable: true,
            description: "Confidence below which a fact is considered Fading",
            affects: "knowledge_fact_lifecycle",
            outcome_signal: "fact_stage_distribution",
            evidence_required: "Fact lifecycle stage distribution analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.factDormantThreshold",
            section: "agents.defaults.behavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Float(0.1),
            bounds: Some((0.0, 0.3)),
            hot_reloadable: true,
            description: "Confidence below which a fact is considered Dormant",
            affects: "knowledge_fact_lifecycle",
            outcome_signal: "fact_stage_distribution",
            evidence_required: "Dormant fact recovery rate analysis",
            direction_hint: TuningDirection::Lower,
        },

        // ===================================================================
        // Tool limits
        // ===================================================================
        ParameterSpec {
            key: "toolLimits.maxPatternLength",
            section: "toolLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(1_000),
            bounds: Some((10.0, 100_000.0)),
            hot_reloadable: true,
            description: "Maximum character length for glob patterns",
            affects: "tool_filesystem",
            outcome_signal: "glob_timeout_rate",
            evidence_required: "Pattern length vs. execution time distribution",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "toolLimits.subprocessTimeoutSecs",
            section: "toolLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(60),
            bounds: Some((5.0, 600.0)),
            hot_reloadable: true,
            description: "Timeout in seconds for filesystem subprocess commands",
            affects: "tool_filesystem",
            outcome_signal: "subprocess_timeout_rate",
            evidence_required: "Subprocess execution time distribution",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "toolLimits.maxWriteBytes",
            section: "toolLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(10_485_760),
            bounds: Some((1024.0, 104_857_600.0)),
            hot_reloadable: true,
            description: "Maximum bytes per workspace write operation (default 10 MiB)",
            affects: "tool_workspace",
            outcome_signal: "write_rejection_rate",
            evidence_required: "Write size distribution analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "toolLimits.maxReadBytes",
            section: "toolLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(52_428_800),
            bounds: Some((1024.0, 524_288_000.0)),
            hot_reloadable: true,
            description: "Maximum bytes per workspace read operation (default 50 MiB)",
            affects: "tool_workspace",
            outcome_signal: "read_rejection_rate",
            evidence_required: "Read size distribution analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "toolLimits.maxCommandLength",
            section: "toolLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(10_000),
            bounds: Some((100.0, 1_000_000.0)),
            hot_reloadable: true,
            description: "Maximum character length of a shell command",
            affects: "tool_workspace",
            outcome_signal: "command_rejection_rate",
            evidence_required: "Command length distribution analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "toolLimits.messageMaxLen",
            section: "toolLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(4_000),
            bounds: Some((100.0, 100_000.0)),
            hot_reloadable: true,
            description: "Maximum characters per intra-session message",
            affects: "tool_communication",
            outcome_signal: "message_truncation_rate",
            evidence_required: "Message length distribution",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "toolLimits.interSessionMaxMessageLen",
            section: "toolLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(100_000),
            bounds: Some((1000.0, 1_000_000.0)),
            hot_reloadable: true,
            description: "Maximum characters per inter-session message",
            affects: "tool_communication",
            outcome_signal: "inter_session_message_truncation_rate",
            evidence_required: "Inter-session message length distribution",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "toolLimits.interSessionMaxTimeoutSecs",
            section: "toolLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(300),
            bounds: Some((10.0, 3600.0)),
            hot_reloadable: true,
            description: "Maximum wait timeout in seconds for inter-session messages",
            affects: "tool_communication",
            outcome_signal: "inter_session_timeout_rate",
            evidence_required: "Inter-session response time distribution",
            direction_hint: TuningDirection::Contextual,
        },

        // ===================================================================
        // API limits
        // ===================================================================
        ParameterSpec {
            key: "apiLimits.maxMessageBytes",
            section: "apiLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(262_144),
            bounds: Some((1024.0, 10_485_760.0)),
            hot_reloadable: true,
            description: "Maximum bytes per streaming message body (default 256 KiB)",
            affects: "api_message_handling",
            outcome_signal: "message_rejection_rate",
            evidence_required: "Message size distribution analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "apiLimits.maxHistoryLimit",
            section: "apiLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(1_000),
            bounds: Some((10.0, 100_000.0)),
            hot_reloadable: true,
            description: "Maximum messages returned by the history endpoint",
            affects: "api_history",
            outcome_signal: "history_request_latency",
            evidence_required: "History endpoint latency vs. limit analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "apiLimits.maxImportBatchSize",
            section: "apiLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(1_000),
            bounds: Some((1.0, 100_000.0)),
            hot_reloadable: true,
            description: "Maximum facts in a single bulk-import request",
            affects: "api_knowledge_import",
            outcome_signal: "import_throughput",
            evidence_required: "Import batch size vs. throughput and error rate",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "apiLimits.idempotencyTtlSecs",
            section: "apiLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(300),
            bounds: Some((10.0, 86_400.0)),
            hot_reloadable: true,
            description: "TTL in seconds for idempotency key cache entries",
            affects: "api_idempotency",
            outcome_signal: "idempotency_cache_hit_rate",
            evidence_required: "Retry timing distribution analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "apiLimits.idempotencyCapacity",
            section: "apiLimits",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(10_000),
            bounds: Some((100.0, 1_000_000.0)),
            hot_reloadable: true,
            description: "Maximum idempotency cache entries (LRU cap)",
            affects: "api_idempotency",
            outcome_signal: "idempotency_cache_eviction_rate",
            evidence_required: "Cache utilization and eviction rate analysis",
            direction_hint: TuningDirection::Higher,
        },

        // ===================================================================
        // Timeouts and retry
        // ===================================================================
        ParameterSpec {
            key: "timeouts.llmCallSecs",
            section: "timeouts",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(300),
            bounds: Some((30.0, 3600.0)),
            hot_reloadable: true,
            description: "Maximum wall-clock seconds for a single LLM API call",
            affects: "llm_latency",
            outcome_signal: "llm_timeout_rate",
            evidence_required: "LLM call duration distribution",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "retry.maxAttempts",
            section: "retry",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(3),
            bounds: Some((0.0, 10.0)),
            hot_reloadable: true,
            description: "Maximum number of retry attempts after an initial transient failure",
            affects: "llm_reliability",
            outcome_signal: "retry_success_rate",
            evidence_required: "Retry success rate by attempt number",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "retry.backoffBaseMs",
            section: "retry",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(1_000),
            bounds: Some((100.0, 30_000.0)),
            hot_reloadable: true,
            description: "Initial exponential backoff delay in milliseconds",
            affects: "llm_reliability",
            outcome_signal: "retry_success_rate",
            evidence_required: "Retry timing vs. success rate analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "retry.backoffMaxMs",
            section: "retry",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(30_000),
            bounds: Some((100.0, 300_000.0)),
            hot_reloadable: true,
            description: "Maximum backoff delay cap in milliseconds",
            affects: "llm_reliability",
            outcome_signal: "retry_success_rate",
            evidence_required: "Backoff ceiling vs. retry outcome analysis",
            direction_hint: TuningDirection::Contextual,
        },

        // ===================================================================
        // Safety
        // ===================================================================
        ParameterSpec {
            key: "agents.defaults.behavior.safetyLoopDetectionThreshold",
            section: "agents.defaults.behavior",
            tier: ParameterTier::PerAgent,
            default: ParameterValue::Int(3),
            bounds: Some((1.0, 20.0)),
            hot_reloadable: true,
            description: "Consecutive identical tool-call sequences before loop detection fires",
            affects: "safety_loop_detection",
            outcome_signal: "loop_detection_precision",
            evidence_required: "False-positive loop detection rate",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.safetyConsecutiveErrorThreshold",
            section: "agents.defaults.behavior",
            tier: ParameterTier::PerAgent,
            default: ParameterValue::Int(4),
            bounds: Some((1.0, 20.0)),
            hot_reloadable: true,
            description: "Consecutive errors before the pipeline aborts with a safety interrupt",
            affects: "safety_error_handling",
            outcome_signal: "unnecessary_abort_rate",
            evidence_required: "Abort necessity analysis at different thresholds",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "agents.defaults.behavior.safetySessionTokenCap",
            section: "agents.defaults.behavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(500_000),
            bounds: Some((10_000.0, 5_000_000.0)),
            hot_reloadable: true,
            description: "Hard token cap for a single session",
            affects: "safety_cost_control",
            outcome_signal: "session_cost_distribution",
            evidence_required: "Session token usage distribution",
            direction_hint: TuningDirection::Contextual,
        },

        // ===================================================================
        // Capacity
        // ===================================================================
        ParameterSpec {
            key: "capacity.maxToolOutputBytes",
            section: "capacity",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(51_200),
            bounds: Some((0.0, 10_485_760.0)),
            hot_reloadable: true,
            description: "Maximum bytes returned by a single tool call before truncation (0 to disable)",
            affects: "tool_output_quality",
            outcome_signal: "tool_output_truncation_rate",
            evidence_required: "Tool output size distribution",
            direction_hint: TuningDirection::Higher,
        },
        ParameterSpec {
            key: "capacity.opusContextTokens",
            section: "capacity",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(1_000_000),
            bounds: Some((200_000.0, 2_000_000.0)),
            hot_reloadable: false,
            description: "Context window token limit applied to Opus-class models",
            affects: "model_context_window",
            outcome_signal: "context_utilization_rate",
            evidence_required: "Context utilization and quality at different window sizes",
            direction_hint: TuningDirection::Higher,
        },

        // ===================================================================
        // Nous behavior
        // ===================================================================
        ParameterSpec {
            key: "nousBehavior.loopDetectionWindow",
            section: "nousBehavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(50),
            bounds: Some((5.0, 500.0)),
            hot_reloadable: true,
            description: "Number of recent tool calls scanned for loop detection",
            affects: "safety_loop_detection",
            outcome_signal: "loop_detection_precision",
            evidence_required: "Window size vs. detection accuracy analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "nousBehavior.gcIntervalSecs",
            section: "nousBehavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(300),
            bounds: Some((30.0, 3600.0)),
            hot_reloadable: true,
            description: "Completed-task garbage collection interval in seconds",
            affects: "resource_management",
            outcome_signal: "memory_usage_trend",
            evidence_required: "Memory usage vs. GC interval analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "nousBehavior.managerHealthIntervalSecs",
            section: "nousBehavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(30),
            bounds: Some((5.0, 300.0)),
            hot_reloadable: true,
            description: "Agent health poll interval in seconds",
            affects: "agent_health_monitoring",
            outcome_signal: "failure_detection_latency",
            evidence_required: "Time-to-detect-failure at different poll intervals",
            direction_hint: TuningDirection::Lower,
        },

        // ===================================================================
        // Provider behavior
        // ===================================================================
        ParameterSpec {
            key: "providerBehavior.nonStreamingTimeoutSecs",
            section: "providerBehavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(120),
            bounds: Some((10.0, 600.0)),
            hot_reloadable: true,
            description: "Timeout in seconds for non-streaming LLM requests",
            affects: "llm_latency",
            outcome_signal: "non_streaming_timeout_rate",
            evidence_required: "Non-streaming request duration distribution",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "providerBehavior.complexityLowThreshold",
            section: "providerBehavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Int(30),
            bounds: Some((0.0, 100.0)),
            hot_reloadable: true,
            description: "Complexity score below which Haiku-class model is selected",
            affects: "model_routing",
            outcome_signal: "model_routing_cost_quality_tradeoff",
            evidence_required: "Quality and cost comparison across routing thresholds",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "providerBehavior.complexityHighThreshold",
            section: "providerBehavior",
            tier: ParameterTier::SelfTuning,
            default: ParameterValue::Int(70),
            bounds: Some((0.0, 100.0)),
            hot_reloadable: true,
            description: "Complexity score above which Opus-class model is selected",
            affects: "model_routing",
            outcome_signal: "model_routing_cost_quality_tradeoff",
            evidence_required: "Quality and cost comparison across routing thresholds",
            direction_hint: TuningDirection::Contextual,
        },

        // ===================================================================
        // Messaging
        // ===================================================================
        ParameterSpec {
            key: "messaging.pollIntervalMs",
            section: "messaging",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(2_000),
            bounds: Some((100.0, 60_000.0)),
            hot_reloadable: true,
            description: "How often Semeion polls for new channel messages in milliseconds",
            affects: "message_latency",
            outcome_signal: "message_delivery_latency",
            evidence_required: "Message delivery latency distribution",
            direction_hint: TuningDirection::Lower,
        },
        ParameterSpec {
            key: "messaging.circuitBreakerThreshold",
            section: "messaging",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Int(5),
            bounds: Some((1.0, 50.0)),
            hot_reloadable: true,
            description: "Consecutive channel errors before the channel is halted",
            affects: "message_reliability",
            outcome_signal: "channel_availability",
            evidence_required: "Channel error patterns and recovery times",
            direction_hint: TuningDirection::Contextual,
        },

        // ===================================================================
        // Daemon behavior
        // ===================================================================
        ParameterSpec {
            key: "daemonBehavior.watchdogBackoffBaseSecs",
            section: "daemonBehavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(2),
            bounds: Some((1.0, 60.0)),
            hot_reloadable: true,
            description: "Base duration in seconds for watchdog restart backoff",
            affects: "daemon_reliability",
            outcome_signal: "restart_recovery_time",
            evidence_required: "Restart backoff vs. recovery time analysis",
            direction_hint: TuningDirection::Contextual,
        },
        ParameterSpec {
            key: "daemonBehavior.watchdogBackoffCapSecs",
            section: "daemonBehavior",
            tier: ParameterTier::Deployment,
            default: ParameterValue::Duration(300),
            bounds: Some((10.0, 3600.0)),
            hot_reloadable: true,
            description: "Maximum watchdog restart backoff duration in seconds",
            affects: "daemon_reliability",
            outcome_signal: "restart_recovery_time",
            evidence_required: "Maximum downtime tolerance analysis",
            direction_hint: TuningDirection::Contextual,
        },
    ]
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn registry_is_non_empty() {
        let specs = all_specs();
        assert!(
            specs.len() >= 30,
            "expected at least 30 parameter specs, got {}",
            specs.len()
        );
    }

    #[test]
    fn all_keys_are_unique() {
        let specs = all_specs();
        let mut seen = std::collections::HashSet::new();
        for spec in specs {
            assert!(
                seen.insert(spec.key),
                "duplicate key in registry: {}",
                spec.key
            );
        }
    }

    #[test]
    fn spec_by_key_finds_known_parameter() {
        let spec = spec_by_key("agents.defaults.behavior.distillationContextTokenTrigger");
        assert!(spec.is_some(), "expected to find distillation context token trigger spec");
        let spec = spec.unwrap();
        assert_eq!(spec.tier, ParameterTier::SelfTuning);
    }

    #[test]
    fn spec_by_key_returns_none_for_unknown() {
        assert!(spec_by_key("nonexistent.key").is_none());
    }

    #[test]
    fn specs_by_section_filters_correctly() {
        let specs = specs_by_section("knowledge");
        assert!(
            !specs.is_empty(),
            "expected at least one knowledge section spec"
        );
        for spec in &specs {
            assert_eq!(spec.section, "knowledge");
        }
    }

    #[test]
    fn specs_affecting_filters_correctly() {
        let specs = specs_affecting("distillation");
        assert!(
            !specs.is_empty(),
            "expected at least one spec affecting distillation"
        );
        for spec in &specs {
            assert!(
                spec.affects.contains("distillation"),
                "spec {} does not affect distillation",
                spec.key
            );
        }
    }

    #[test]
    fn bounds_are_valid_where_present() {
        for spec in all_specs() {
            if let Some((min, max)) = spec.bounds {
                assert!(
                    min <= max,
                    "spec {}: bounds min ({}) > max ({})",
                    spec.key,
                    min,
                    max
                );
            }
        }
    }

    #[test]
    fn all_specs_have_non_empty_fields() {
        for spec in all_specs() {
            assert!(!spec.key.is_empty(), "spec has empty key");
            assert!(!spec.section.is_empty(), "spec {} has empty section", spec.key);
            assert!(!spec.description.is_empty(), "spec {} has empty description", spec.key);
            assert!(!spec.affects.is_empty(), "spec {} has empty affects", spec.key);
            assert!(!spec.outcome_signal.is_empty(), "spec {} has empty outcome_signal", spec.key);
            assert!(
                !spec.evidence_required.is_empty(),
                "spec {} has empty evidence_required",
                spec.key
            );
        }
    }
}
