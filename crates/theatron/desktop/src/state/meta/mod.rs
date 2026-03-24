//! Meta-insights state: agent performance, conversation quality, knowledge growth,
//! memory health, and system self-reflection.

use std::collections::HashMap;

// -- Shared types -------------------------------------------------------------

/// A single data point in a time series.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DataPoint {
    pub label: String,
    pub value: f64,
}

/// Direction of a trend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TrendDirection {
    Up,
    Down,
    #[default]
    Flat,
}

impl TrendDirection {
    #[must_use]
    pub(crate) fn arrow(&self) -> &'static str {
        match self {
            Self::Up => "\u{25b2}",     // ▲
            Self::Down => "\u{25bc}",   // ▼
            Self::Flat => "\u{2014}",   // —
        }
    }

    #[must_use]
    pub(crate) fn color(&self) -> &'static str {
        match self {
            Self::Up => "#22c55e",
            Self::Down => "#ef4444",
            Self::Flat => "#888",
        }
    }
}

// -- Agent performance --------------------------------------------------------

/// Performance scorecard for a single agent.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AgentScorecard {
    pub agent_id: String,
    pub agent_name: String,
    pub avg_tokens_per_response: f64,
    pub tool_calls_per_session: f64,
    pub tool_success_rate: f64,
    pub distillation_frequency: f64,
    pub avg_context_before_distill: f64,
    pub messages_per_session: f64,
    pub sessions_per_day: f64,
    pub errors_per_session: f64,
}

impl AgentScorecard {
    /// Normalized radar axes: response quality, tool efficiency, context management,
    /// productivity, reliability. Each value is 0.0--1.0.
    #[must_use]
    pub(crate) fn radar_axes(&self) -> [f64; 5] {
        [
            // WHY: Lower tokens-per-response = more concise = higher quality score.
            // Normalize against 2000 tokens as a "verbose" baseline.
            (1.0 - (self.avg_tokens_per_response / 2000.0).min(1.0)).max(0.0),
            self.tool_success_rate.clamp(0.0, 1.0),
            // WHY: Lower distillation frequency = better context management.
            (1.0 - (self.distillation_frequency / 10.0).min(1.0)).max(0.0),
            // WHY: More messages per session = more productive (capped at 50).
            (self.messages_per_session / 50.0).clamp(0.0, 1.0),
            // WHY: Fewer errors = more reliable (invert).
            (1.0 - (self.errors_per_session / 5.0).min(1.0)).max(0.0),
        ]
    }
}

/// An anomaly detected in agent performance metrics.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Anomaly {
    pub agent_name: String,
    pub metric_name: String,
    pub current_value: f64,
    pub baseline_mean: f64,
    pub deviation_pct: f64,
    pub direction: TrendDirection,
}

impl Anomaly {
    /// Formatted alert message.
    #[must_use]
    pub(crate) fn message(&self) -> String {
        let dir = if self.deviation_pct > 0.0 {
            "increased"
        } else {
            "decreased"
        };
        format!(
            "{}'s {} {} {:.0}% this week ({:.1} vs {:.1} avg)",
            self.agent_name,
            self.metric_name,
            dir,
            self.deviation_pct.abs(),
            self.current_value,
            self.baseline_mean,
        )
    }
}

/// Detect anomalies using z-score approach.
///
/// Returns entries where the latest value exceeds 2 standard deviations from
/// the mean of `values`. Returns `None` if insufficient data.
#[must_use]
#[expect(dead_code, reason = "used in tests; wired when SSE anomaly detection is plumbed")]
pub(crate) fn detect_anomaly(
    agent_name: &str,
    metric_name: &str,
    values: &[f64],
) -> Option<Anomaly> {
    if values.len() < 3 {
        return None;
    }

    let n = values.len();
    let mean = values.iter().sum::<f64>() / n as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
    let std_dev = variance.sqrt();

    if std_dev < f64::EPSILON {
        return None;
    }

    let latest = values[n - 1];
    let z_score = (latest - mean) / std_dev;

    if z_score.abs() < 2.0 {
        return None;
    }

    let deviation_pct = if mean.abs() > f64::EPSILON {
        ((latest - mean) / mean) * 100.0
    } else {
        0.0
    };

    let direction = if latest > mean {
        TrendDirection::Up
    } else {
        TrendDirection::Down
    };

    Some(Anomaly {
        agent_name: agent_name.to_string(),
        metric_name: metric_name.to_string(),
        current_value: latest,
        baseline_mean: mean,
        deviation_pct,
        direction,
    })
}

/// Store for agent performance data.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AgentPerformanceStore {
    pub scorecards: Vec<AgentScorecard>,
    pub anomalies: Vec<Anomaly>,
    pub tokens_per_response_series: HashMap<String, Vec<DataPoint>>,
}

impl AgentPerformanceStore {
    #[must_use]
    #[expect(dead_code, reason = "constructed via struct literal in view; new() for test convenience")]
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

// -- Conversation quality -----------------------------------------------------

/// Conversation quality time series data.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct QualityStore {
    pub avg_turn_length: Vec<DataPoint>,
    pub response_to_question_ratio: Vec<DataPoint>,
    pub tool_call_density: Vec<DataPoint>,
    pub thinking_time_ratio: Vec<DataPoint>,
    pub depth_distribution: DepthDistribution,
    pub top_topics: Vec<TopicEntry>,
}

impl QualityStore {
    #[must_use]
    #[expect(dead_code, reason = "constructed via struct literal in view")]
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// Session depth distribution buckets.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DepthDistribution {
    pub short: u32,
    pub medium: u32,
    pub long: u32,
}

impl DepthDistribution {
    /// Classify a session by message count into a depth bucket.
    pub(crate) fn classify(message_count: u32) -> &'static str {
        match message_count {
            0..10 => "short",
            10..50 => "medium",
            _ => "long",
        }
    }

    /// Total sessions across all buckets.
    #[must_use]
    pub(crate) fn total(&self) -> u32 {
        self.short + self.medium + self.long
    }

    /// Percentage for a given bucket.
    #[must_use]
    #[expect(dead_code, reason = "used in tests; available for view consumption")]
    pub(crate) fn pct(&self, bucket: u32) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        (f64::from(bucket) / f64::from(total)) * 100.0
    }
}

/// A topic with its frequency.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TopicEntry {
    pub name: String,
    pub count: u32,
}

/// Compute average turn length from a slice of message lengths.
#[must_use]
#[expect(dead_code, reason = "used in tests; wired when per-message metrics are available")]
pub(crate) fn compute_average(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Compute ratio of agent turns to user turns.
#[must_use]
#[expect(dead_code, reason = "used in tests; wired when per-turn metrics are available")]
pub(crate) fn compute_ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    numerator as f64 / denominator as f64
}

// -- Knowledge growth ---------------------------------------------------------

/// Knowledge graph growth metrics over time.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct KnowledgeGrowthStore {
    pub total_entities: Vec<DataPoint>,
    pub new_entities_per_period: Vec<DataPoint>,
    pub total_relationships: Vec<DataPoint>,
    pub new_relationships_per_period: Vec<DataPoint>,
    pub density_over_time: Vec<DataPoint>,
    pub entity_type_distribution: Vec<EntityTypeSlice>,
    pub current_entity_rate: f64,
    pub current_relationship_rate: f64,
    pub acceleration: TrendDirection,
}

impl KnowledgeGrowthStore {
    #[must_use]
    #[expect(dead_code, reason = "constructed via struct literal in view")]
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// A single slice of the entity type distribution.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EntityTypeSlice {
    pub entity_type: String,
    pub count: u32,
    pub color: &'static str,
}

/// Compute growth acceleration from a series of cumulative counts.
///
/// Positive = speeding up, negative = slowing down, ~0 = steady.
#[must_use]
pub(crate) fn compute_acceleration(values: &[f64]) -> TrendDirection {
    if values.len() < 3 {
        return TrendDirection::Flat;
    }

    let n = values.len();
    // WHY: Second derivative approximation -- compare recent growth rate to prior.
    let recent_growth = values[n - 1] - values[n - 2];
    let prior_growth = values[n - 2] - values[n - 3];

    let diff = recent_growth - prior_growth;
    if diff > 1.0 {
        TrendDirection::Up
    } else if diff < -1.0 {
        TrendDirection::Down
    } else {
        TrendDirection::Flat
    }
}

/// Palette for entity type stacked area charts.
pub(crate) const ENTITY_TYPE_COLORS: &[&str] = &[
    "#4a9aff", "#22c55e", "#f59e0b", "#ef4444", "#8b5cf6",
    "#ec4899", "#06b6d4", "#84cc16", "#f97316", "#6366f1",
];

// -- Memory health ------------------------------------------------------------

/// Composite memory health data.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct MemoryHealthStore {
    pub health_score: f64,
    pub confidence_distribution: Vec<ConfidenceBucket>,
    pub stale_entities: Vec<StaleEntity>,
    pub orphan_ratio: f64,
    pub decay_pressure_count: u32,
    pub health_over_time: Vec<DataPoint>,
    pub recommendations: Vec<String>,
}

impl MemoryHealthStore {
    #[must_use]
    #[expect(dead_code, reason = "constructed via struct literal in view")]
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// A bucket in the confidence distribution histogram.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConfidenceBucket {
    pub range_label: String,
    pub count: u32,
}

/// An entity that hasn't been updated recently.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StaleEntity {
    pub name: String,
    pub last_updated: String,
    pub days_stale: u32,
}

/// Compute composite memory health score.
///
/// Weights: confidence_score 0.4, (1 - orphan_ratio) 0.3, (1 - staleness_ratio) 0.3.
#[must_use]
pub(crate) fn compute_health_score(
    avg_confidence: f64,
    orphan_ratio: f64,
    staleness_ratio: f64,
) -> f64 {
    // INVARIANT: All inputs should be 0.0--1.0; clamp defensively.
    let c = avg_confidence.clamp(0.0, 1.0);
    let o = orphan_ratio.clamp(0.0, 1.0);
    let s = staleness_ratio.clamp(0.0, 1.0);

    c * 0.4 + (1.0 - o) * 0.3 + (1.0 - s) * 0.3
}

/// Color for a health score (0.0--1.0).
#[must_use]
pub(crate) fn health_score_color(score: f64) -> &'static str {
    if score >= 0.7 {
        "#22c55e"
    } else if score >= 0.4 {
        "#f59e0b"
    } else {
        "#ef4444"
    }
}

/// Generate recommendations based on health metrics.
#[must_use]
pub(crate) fn generate_recommendations(
    staleness_ratio: f64,
    orphan_ratio: f64,
    avg_confidence: f64,
    growth_rate: f64,
) -> Vec<String> {
    let mut recs = Vec::new();

    if staleness_ratio > 0.15 {
        recs.push(format!(
            "{:.0}% of entities are stale \u{2014} review or archive",
            staleness_ratio * 100.0
        ));
    }
    if orphan_ratio > 0.2 {
        recs.push(format!(
            "{:.0}% of entities are orphaned \u{2014} consider linking or removing",
            orphan_ratio * 100.0
        ));
    }
    if avg_confidence < 0.5 {
        recs.push(
            "Average confidence is low \u{2014} verify or reinforce key facts".to_string(),
        );
    }
    if growth_rate < 1.0 {
        recs.push(
            "Knowledge growth has slowed \u{2014} consider seeding new topics".to_string(),
        );
    }

    recs
}

// -- System reflection --------------------------------------------------------

/// System overview statistics.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SystemOverview {
    pub uptime_seconds: u64,
    pub total_sessions: u64,
    pub total_tokens: u64,
    pub total_entities: u64,
    pub total_cost_usd: f64,
}

/// A cell in the activity heatmap (hour-of-day vs day-of-week).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HeatmapCell {
    pub day: u8,
    pub hour: u8,
    pub count: u32,
}

/// Resource efficiency metrics.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct EfficiencyMetrics {
    pub cost_per_entity: f64,
    pub cost_per_session: f64,
    pub tokens_per_entity: f64,
    pub cost_per_entity_trend: TrendDirection,
}

/// A system journal event.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JournalEvent {
    pub timestamp: String,
    pub event_type: JournalEventType,
    pub message: String,
}

/// Types of system journal events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JournalEventType {
    Error,
    Distillation,
    ConfigChange,
    MemoryMerge,
}

impl JournalEventType {
    #[must_use]
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Distillation => "distillation",
            Self::ConfigChange => "config",
            Self::MemoryMerge => "memory",
        }
    }

    #[must_use]
    pub(crate) fn color(&self) -> &'static str {
        match self {
            Self::Error => "#ef4444",
            Self::Distillation => "#4a9aff",
            Self::ConfigChange => "#f59e0b",
            Self::MemoryMerge => "#22c55e",
        }
    }
}

/// Store for system self-reflection data.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SystemReflectionStore {
    pub overview: SystemOverview,
    pub heatmap: Vec<HeatmapCell>,
    pub efficiency: EfficiencyMetrics,
    pub journal: Vec<JournalEvent>,
}

impl SystemReflectionStore {
    #[must_use]
    #[expect(dead_code, reason = "constructed via struct literal in view")]
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// Build activity heatmap from session timestamps.
///
/// Expects timestamps as `(day_of_week 0=Mon..6=Sun, hour 0..23)` tuples.
#[must_use]
pub(crate) fn build_heatmap(timestamps: &[(u8, u8)]) -> Vec<HeatmapCell> {
    let mut grid = [[0u32; 24]; 7];

    for &(day, hour) in timestamps {
        if day < 7 && hour < 24 {
            grid[day as usize][hour as usize] += 1;
        }
    }

    let mut cells = Vec::with_capacity(7 * 24);
    for day in 0..7u8 {
        for hour in 0..24u8 {
            cells.push(HeatmapCell {
                day,
                hour,
                count: grid[day as usize][hour as usize],
            });
        }
    }
    cells
}

/// Color for a heatmap cell intensity (0 = empty, higher = more active).
#[must_use]
pub(crate) fn heatmap_color(count: u32, max_count: u32) -> &'static str {
    if max_count == 0 || count == 0 {
        return "#1a1a2e";
    }
    let ratio = f64::from(count) / f64::from(max_count);
    if ratio > 0.75 {
        "#22c55e"
    } else if ratio > 0.5 {
        "#4a9aff"
    } else if ratio > 0.25 {
        "#2a4a6a"
    } else {
        "#1a2a3e"
    }
}

/// Compute cost-per-entity.
#[must_use]
pub(crate) fn cost_per_entity(total_cost: f64, total_entities: u64) -> f64 {
    if total_entities == 0 {
        return 0.0;
    }
    total_cost / total_entities as f64
}

/// Compute tokens-per-entity.
#[must_use]
pub(crate) fn tokens_per_entity(total_tokens: u64, total_entities: u64) -> f64 {
    if total_entities == 0 {
        return 0.0;
    }
    total_tokens as f64 / total_entities as f64
}

#[cfg(test)]
mod tests;
