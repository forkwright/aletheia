//! Benchmark report aggregation, statistics, and publishability checks.

use std::collections::BTreeMap;

use crate::provenance::EvalProvenance;

use super::{
    BENCHMARK_STAT_RESAMPLES, BenchmarkComparisonMetric, BenchmarkComparisonReport,
    BenchmarkComparisonStatus, BenchmarkIngestionSummary, BenchmarkMetadata,
    BenchmarkPublishability, BenchmarkReliabilitySummary, BenchmarkReport, BenchmarkStatistics,
    JudgeSummary, MAX_PUBLISHABLE_ERROR_RATE, MAX_PUBLISHABLE_INGESTION_ERROR_RATE,
    MAX_PUBLISHABLE_INGESTION_PARTIAL_RATE, MAX_PUBLISHABLE_NO_ANSWER_RATE,
    MAX_PUBLISHABLE_TIMEOUT_RATE, MIN_PUBLISHABLE_SCORED_QUESTIONS, QuestionResult, QuestionStatus,
};

#[derive(Debug, Clone, Copy, Default)]
struct BenchmarkCounts {
    scored: usize,
    ingestion_errors: usize,
    ingestion_partials: usize,
    errors: usize,
    timeouts: usize,
    no_answers: usize,
}

impl BenchmarkCounts {
    fn from_questions(questions: &[QuestionResult]) -> Self {
        let mut counts = Self::default();
        for question in questions {
            match question.status {
                QuestionStatus::Scored => counts.scored += 1,
                QuestionStatus::IngestionError => counts.ingestion_errors += 1,
                QuestionStatus::IngestionPartial => counts.ingestion_partials += 1,
                QuestionStatus::Error => counts.errors += 1,
                QuestionStatus::Timeout => counts.timeouts += 1,
                QuestionStatus::NoAnswer => counts.no_answers += 1,
            }
        }
        counts
    }
}

impl JudgeSummary {
    fn from_questions(questions: &[QuestionResult]) -> Option<Self> {
        let mut summary = Self {
            attempted: 0,
            scored: 0,
            errors: 0,
            correct: 0,
        };
        for score in questions.iter().filter_map(|q| q.judge_score.as_ref()) {
            summary.attempted += 1;
            if score.status.is_scored() {
                summary.scored += 1;
                if score.correct {
                    summary.correct += 1;
                }
            } else {
                summary.errors += 1;
            }
        }
        (summary.attempted > 0).then_some(summary)
    }
}

impl BenchmarkReliabilitySummary {
    #[must_use]
    fn from_counts(total: usize, counts: BenchmarkCounts) -> Self {
        Self {
            attempted: total,
            scored: counts.scored,
            ingestion_errors: counts.ingestion_errors,
            ingestion_partials: counts.ingestion_partials,
            errors: counts.errors,
            timeouts: counts.timeouts,
            no_answers: counts.no_answers,
            error_rate: rate(counts.errors, total),
            timeout_rate: rate(counts.timeouts, total),
            no_answer_rate: rate(counts.no_answers, total),
            ingestion_error_rate: rate(counts.ingestion_errors, total),
            ingestion_partial_rate: rate(counts.ingestion_partials, total),
        }
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "question counts are small (<10000); f64 mantissa handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — question counts are bounded and small"
)]
fn rate(count: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64 // SAFETY: benchmark question counts are bounded and small
    }
}

fn push_rate_reason(
    reasons: &mut Vec<String>,
    label: &str,
    observed: f64,
    maximum: f64,
    count: usize,
    total: usize,
) {
    if observed > maximum {
        reasons.push(format!(
            "{label} rate {:.1}% exceeds maximum {:.1}% ({count}/{total})",
            observed * 100.0,
            maximum * 100.0
        ));
    }
}

impl BenchmarkReport {
    /// Build an aggregate report from individual question results.
    #[must_use]
    pub fn new(benchmark: impl Into<String>, questions: Vec<QuestionResult>) -> Self {
        let counts = BenchmarkCounts::from_questions(&questions);
        let total = questions.len();
        Self {
            benchmark: benchmark.into(),
            total,
            scored: counts.scored,
            ingestion_summary: BenchmarkIngestionSummary::default(),
            ingestion_errors: counts.ingestion_errors,
            ingestion_partials: counts.ingestion_partials,
            errors: counts.errors,
            timeouts: counts.timeouts,
            no_answers: counts.no_answers,
            reliability: BenchmarkReliabilitySummary::from_counts(total, counts),
            judge_summary: JudgeSummary::from_questions(&questions),
            questions,
            provenance: None,
            metadata: None,
            statistics: None,
            comparisons: Vec::new(),
            publishability: None,
        }
    }

    /// Build a report with metadata.
    #[must_use]
    pub fn with_metadata(
        benchmark: impl Into<String>,
        questions: Vec<QuestionResult>,
        metadata: BenchmarkMetadata,
    ) -> Self {
        let counts = BenchmarkCounts::from_questions(&questions);
        let total = questions.len();
        Self {
            benchmark: benchmark.into(),
            total,
            scored: counts.scored,
            ingestion_summary: BenchmarkIngestionSummary::default(),
            ingestion_errors: counts.ingestion_errors,
            ingestion_partials: counts.ingestion_partials,
            errors: counts.errors,
            timeouts: counts.timeouts,
            no_answers: counts.no_answers,
            reliability: BenchmarkReliabilitySummary::from_counts(total, counts),
            judge_summary: JudgeSummary::from_questions(&questions),
            questions,
            provenance: None,
            metadata: Some(metadata),
            statistics: None,
            comparisons: Vec::new(),
            publishability: None,
        }
    }

    /// Attach the shared provenance envelope for this benchmark report.
    #[must_use]
    pub fn with_provenance(mut self, provenance: EvalProvenance) -> Self {
        self.provenance = Some(provenance);
        self
    }

    /// Attach aggregate haystack ingestion counters to the report.
    #[must_use]
    pub(crate) fn with_ingestion_summary(mut self, summary: BenchmarkIngestionSummary) -> Self {
        self.ingestion_summary = summary;
        self
    }

    /// Attach bootstrap CIs for F1 and exact-match rate to the report.
    ///
    /// Computes 95% percentile bootstrap CIs using `n_resamples` draws.
    /// Call this after all questions have been collected, before publishing
    /// results. Returns the same report unchanged if fewer than 2 questions
    /// are present (CI requires n ≥ 2).
    ///
    /// # Provenance
    ///
    /// Absorbed from `shared/stats.py` in the quantified-self pipeline.
    /// Reference: Efron & Hastie (2021) *Computer Age Statistical Inference*.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10K); f64 handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn with_statistics(mut self, n_resamples: usize) -> Self {
        use crate::stats::bootstrap::bootstrap_ci;

        if self.questions.len() < 2 {
            return self;
        }
        let f1_scores: Vec<f64> = self.questions.iter().map(|q| q.score.f1).collect();
        let em_scores: Vec<f64> = self
            .questions
            .iter()
            .map(|q| if q.score.exact_match { 1.0 } else { 0.0 })
            .collect();

        let mean_fn = |data: &[f64]| {
            if data.is_empty() {
                0.0
            } else {
                data.iter().sum::<f64>() / data.len() as f64 // SAFETY: bootstrap resample count; small usize within f64 mantissa
            }
        };

        let f1_ci = bootstrap_ci(&f1_scores, mean_fn, n_resamples, 42, 0.95);
        let em_ci = bootstrap_ci(&em_scores, mean_fn, n_resamples, 42, 0.95);

        if let (Ok(f1), Ok(em)) = (f1_ci, em_ci) {
            self.statistics = Some(BenchmarkStatistics {
                f1_ci_low: f1.ci_low,
                f1_ci_high: f1.ci_high,
                em_ci_low: em.ci_low,
                em_ci_high: em.ci_high,
                n_resamples: f1.n_resamples,
                method: "percentile bootstrap (Efron & Hastie 2021)".to_owned(),
            });
        }
        self
    }

    /// Attach the standard benchmark statistical summary and publishability assessment.
    #[must_use]
    pub fn with_standard_statistics(self) -> Self {
        self.with_statistics(BENCHMARK_STAT_RESAMPLES)
            .with_publishability_assessment()
    }

    /// Attach baseline/candidate comparisons against a prior full benchmark report.
    ///
    /// The current report is treated as the candidate and `baseline` as the
    /// baseline. Comparisons are matched by question id and include FDR
    /// correction across the F1 and exact-match metric comparisons.
    #[must_use]
    pub fn with_comparisons_against(mut self, baseline: &Self, label: impl Into<String>) -> Self {
        self.comparisons = self.comparison_reports_against(baseline, label);
        self.with_publishability_assessment()
    }

    /// Build baseline/candidate statistical comparisons without mutating the report.
    #[must_use]
    pub fn comparison_reports_against(
        &self,
        baseline: &Self,
        label: impl Into<String>,
    ) -> Vec<BenchmarkComparisonReport> {
        let label = label.into();
        if self.benchmark != baseline.benchmark {
            return [
                BenchmarkComparisonMetric::F1,
                BenchmarkComparisonMetric::ExactMatch,
            ]
            .into_iter()
            .map(|metric| {
                BenchmarkComparisonReport::unavailable(
                    metric,
                    format!("{label} {metric}"),
                    BenchmarkComparisonStatus::Incomparable,
                    0,
                    format!(
                        "benchmark mismatch: baseline={} candidate={}",
                        baseline.benchmark, self.benchmark
                    ),
                )
            })
            .collect();
        }

        let mut comparisons = [
            BenchmarkComparisonMetric::F1,
            BenchmarkComparisonMetric::ExactMatch,
        ]
        .into_iter()
        .map(|metric| self.comparison_for_metric(baseline, metric, &label))
        .collect::<Vec<_>>();

        let raw_p_values = comparisons
            .iter()
            .filter_map(|comparison| comparison.statistics.as_ref())
            .map(|statistics| statistics.p_raw)
            .filter(|p| p.is_finite())
            .collect::<Vec<_>>();

        if let Ok(adjusted) =
            crate::stats::fdr_correct(&raw_p_values, crate::stats::FdrMethod::BenjaminiHochberg)
        {
            let mut adjusted_iter = adjusted.into_iter();
            for comparison in &mut comparisons {
                if let Some(statistics) = comparison.statistics.as_mut()
                    && statistics.p_raw.is_finite()
                    && let Some(adjusted_p) = adjusted_iter.next()
                {
                    statistics.set_adjusted_p(adjusted_p);
                }
            }
        }

        comparisons
    }

    /// Attach an explicit publishability assessment.
    #[must_use]
    pub fn with_publishability_assessment(mut self) -> Self {
        self.publishability = Some(self.assess_publishability());
        self
    }

    /// Assess whether this report has enough statistical context for publication.
    #[must_use]
    pub fn assess_publishability(&self) -> BenchmarkPublishability {
        let mut reasons = Vec::new();
        if self.scored < MIN_PUBLISHABLE_SCORED_QUESTIONS {
            reasons.push(format!(
                "requires at least {MIN_PUBLISHABLE_SCORED_QUESTIONS} scored questions for bootstrap CIs; got {}",
                self.scored
            ));
        }
        if self.statistics.is_none() {
            reasons.push("missing bootstrap confidence intervals for EM and F1".to_owned());
        }
        self.add_reliability_publishability_reasons(&mut reasons);
        self.add_provenance_publishability_reasons(&mut reasons);
        self.add_metadata_publishability_reasons(&mut reasons);
        self.add_comparison_publishability_reasons(&mut reasons);

        BenchmarkPublishability {
            publishable: reasons.is_empty(),
            minimum_scored_questions: MIN_PUBLISHABLE_SCORED_QUESTIONS,
            reasons,
        }
    }

    fn add_reliability_publishability_reasons(&self, reasons: &mut Vec<String>) {
        let reliability = self.reliability_summary();
        push_rate_reason(
            reasons,
            "error",
            reliability.error_rate,
            MAX_PUBLISHABLE_ERROR_RATE,
            reliability.errors,
            reliability.attempted,
        );
        push_rate_reason(
            reasons,
            "timeout",
            reliability.timeout_rate,
            MAX_PUBLISHABLE_TIMEOUT_RATE,
            reliability.timeouts,
            reliability.attempted,
        );
        push_rate_reason(
            reasons,
            "no-answer",
            reliability.no_answer_rate,
            MAX_PUBLISHABLE_NO_ANSWER_RATE,
            reliability.no_answers,
            reliability.attempted,
        );
        push_rate_reason(
            reasons,
            "ingestion-error",
            reliability.ingestion_error_rate,
            MAX_PUBLISHABLE_INGESTION_ERROR_RATE,
            reliability.ingestion_errors,
            reliability.attempted,
        );
        push_rate_reason(
            reasons,
            "ingestion-partial",
            reliability.ingestion_partial_rate,
            MAX_PUBLISHABLE_INGESTION_PARTIAL_RATE,
            reliability.ingestion_partials,
            reliability.attempted,
        );
    }

    fn add_provenance_publishability_reasons(&self, reasons: &mut Vec<String>) {
        match &self.provenance {
            Some(provenance) => {
                if provenance.config_hash.is_none() {
                    reasons.push("missing benchmark configuration hash".to_owned());
                }
                if provenance.scenario_suite_hash.is_none() {
                    reasons.push("missing dataset hash in provenance".to_owned());
                }
                if provenance.redacted_args.is_empty() {
                    reasons.push("missing redacted CLI provenance".to_owned());
                }
            }
            None => reasons.push("missing eval provenance".to_owned()),
        }
    }

    fn add_metadata_publishability_reasons(&self, reasons: &mut Vec<String>) {
        match &self.metadata {
            Some(metadata) => {
                if metadata.dataset_hash.is_none() {
                    reasons.push("missing dataset hash in benchmark metadata".to_owned());
                }
                if let Some(validation) = &metadata.dataset_validation
                    && !validation.errors.is_empty()
                {
                    reasons.push(format!(
                        "dataset validation has {} error(s)",
                        validation.errors.len()
                    ));
                }
            }
            None => reasons.push("missing benchmark metadata".to_owned()),
        }
    }

    fn add_comparison_publishability_reasons(&self, reasons: &mut Vec<String>) {
        for comparison in &self.comparisons {
            if comparison.status != BenchmarkComparisonStatus::Complete {
                let reason = comparison
                    .reason
                    .as_deref()
                    .unwrap_or("comparison statistics are incomplete");
                reasons.push(format!(
                    "{} comparison is not publishable: {reason}",
                    comparison.metric
                ));
            } else if comparison
                .statistics
                .as_ref()
                .and_then(|statistics| statistics.p_adjusted)
                .is_none()
            {
                reasons.push(format!(
                    "{} comparison is missing FDR-adjusted p-value",
                    comparison.metric
                ));
            }
        }
    }

    /// Recompute the operational reliability summary from report status counts.
    #[must_use]
    pub fn reliability_summary(&self) -> BenchmarkReliabilitySummary {
        BenchmarkReliabilitySummary::from_counts(
            self.total,
            BenchmarkCounts {
                scored: self.scored,
                ingestion_errors: self.ingestion_errors,
                ingestion_partials: self.ingestion_partials,
                errors: self.errors,
                timeouts: self.timeouts,
                no_answers: self.no_answers,
            },
        )
    }

    fn scored_questions(&self) -> Vec<&QuestionResult> {
        self.questions
            .iter()
            .filter(|q| q.status.is_scored())
            .collect()
    }

    fn attempted_questions_by_id(&self) -> BTreeMap<&str, &QuestionResult> {
        self.questions.iter().map(|q| (q.id.as_str(), q)).collect()
    }

    fn comparison_for_metric(
        &self,
        baseline: &Self,
        metric: BenchmarkComparisonMetric,
        label: &str,
    ) -> BenchmarkComparisonReport {
        let (baseline_scores, candidate_scores) = self.paired_scores(baseline, metric);
        let matched_questions = baseline_scores.len();
        let comparison_label = format!("{label} {metric}");

        if matched_questions < MIN_PUBLISHABLE_SCORED_QUESTIONS {
            return BenchmarkComparisonReport::unavailable(
                metric,
                comparison_label,
                BenchmarkComparisonStatus::InsufficientSamples,
                matched_questions,
                format!(
                    "requires at least {MIN_PUBLISHABLE_SCORED_QUESTIONS} matched attempted questions; got {matched_questions}"
                ),
            );
        }

        match crate::stats::comparison_report(
            &baseline_scores,
            &candidate_scores,
            comparison_label.clone(),
            None,
        ) {
            Ok(statistics) => BenchmarkComparisonReport::complete(
                metric,
                comparison_label,
                matched_questions,
                statistics,
            ),
            Err(error) => BenchmarkComparisonReport::unavailable(
                metric,
                comparison_label,
                BenchmarkComparisonStatus::Error,
                matched_questions,
                error.to_string(),
            ),
        }
    }

    fn paired_scores(
        &self,
        baseline: &Self,
        metric: BenchmarkComparisonMetric,
    ) -> (Vec<f64>, Vec<f64>) {
        let baseline_by_id = baseline.attempted_questions_by_id();
        let candidate_by_id = self.attempted_questions_by_id();
        let mut baseline_scores = Vec::new();
        let mut candidate_scores = Vec::new();

        for (id, candidate) in candidate_by_id {
            if let Some(baseline_question) = baseline_by_id.get(id) {
                baseline_scores.push(metric.score(baseline_question));
                candidate_scores.push(metric.score(candidate));
            }
        }

        (baseline_scores, candidate_scores)
    }

    /// Fraction of attempted questions with exact-match score >= 1.0.
    ///
    /// Non-scorable outcomes remain in the denominator and contribute zero.
    /// Use [`BenchmarkReport::scored_only_exact_match_rate`] for the
    /// diagnostic score over answered questions only.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn exact_match_rate(&self) -> f64 {
        if self.questions.is_empty() {
            return 0.0;
        }
        let hits = self
            .questions
            .iter()
            .filter(|q| q.score.exact_match)
            .count();
        hits as f64 / self.questions.len() as f64 // SAFETY: question counts <10_000 per function-level #[expect]
    }

    /// Exact-match rate over answered questions only.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn scored_only_exact_match_rate(&self) -> f64 {
        let scored = self.scored_questions();
        if scored.is_empty() {
            return 0.0;
        }
        let hits = scored.iter().filter(|q| q.score.exact_match).count();
        hits as f64 / scored.len() as f64 // SAFETY: question counts <10_000 per function-level #[expect]
    }

    /// Mean F1 score across attempted questions.
    ///
    /// Non-scorable outcomes remain in the denominator and contribute zero.
    /// Use [`BenchmarkReport::scored_only_mean_f1`] for the diagnostic score
    /// over answered questions only.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn mean_f1(&self) -> f64 {
        if self.questions.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.questions.iter().map(|q| q.score.f1).sum();
        sum / self.questions.len() as f64 // SAFETY: question counts <10_000 per function-level #[expect]
    }

    /// Mean F1 over answered questions only.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn scored_only_mean_f1(&self) -> f64 {
        let scored = self.scored_questions();
        if scored.is_empty() {
            return 0.0;
        }
        let sum: f64 = scored.iter().map(|q| q.score.f1).sum();
        sum / scored.len() as f64 // SAFETY: question counts <10_000 per function-level #[expect]
    }

    /// Mean LLM-as-judge accuracy across all attempted judge calls.
    ///
    /// Judge errors stay in the denominator and count as incorrect. Use
    /// [`BenchmarkReport::judge_summary`] to distinguish parsed judgments from
    /// failed/refused/malformed judge attempts.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "question counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — question counts are bounded and small"
    )]
    pub fn judge_accuracy(&self) -> Option<f64> {
        let summary = self.judge_summary?;
        Some(summary.correct as f64 / summary.attempted as f64) // SAFETY: question counts <10_000 per function-level #[expect]
    }

    /// Mean Recall@k across all questions with recorded retrieval metrics.
    ///
    /// The benchmark runner records `0.0` when `retrieval_k` is configured but
    /// a question is non-scorable or the knowledge search fails, so those
    /// retrieval failures stay in the denominator. Legacy reports with missing
    /// retrieval fields are still excluded because they lack a recorded metric.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "value counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — value counts are bounded and small"
    )]
    pub fn mean_recall_at_k(&self) -> Option<f64> {
        let values: Vec<f64> = self
            .questions
            .iter()
            .filter_map(|q| q.recall_at_k)
            .collect();
        if values.is_empty() {
            return None;
        }
        Some(values.iter().sum::<f64>() / values.len() as f64) // SAFETY: value counts <10_000 per function-level #[expect]
    }

    /// Mean NDCG@k across all questions with recorded retrieval metrics.
    ///
    /// The benchmark runner records `0.0` when `retrieval_k` is configured but
    /// a question is non-scorable or the knowledge search fails, so those
    /// retrieval failures stay in the denominator. Legacy reports with missing
    /// retrieval fields are still excluded because they lack a recorded metric.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "value counts are small (<10000); f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — value counts are bounded and small"
    )]
    pub fn mean_ndcg_at_k(&self) -> Option<f64> {
        let values: Vec<f64> = self.questions.iter().filter_map(|q| q.ndcg_at_k).collect();
        if values.is_empty() {
            return None;
        }
        Some(values.iter().sum::<f64>() / values.len() as f64) // SAFETY: value counts <10_000 per function-level #[expect]
    }

    /// Group attempted questions by category and return `(category, EM, F1)`.
    ///
    /// Non-scorable outcomes remain in each category denominator and
    /// contribute zero. Use [`BenchmarkReport::scored_only_per_category`] for
    /// the diagnostic answered-question denominator.
    #[must_use]
    pub fn per_category(&self) -> Vec<(String, f64, f64)> {
        Self::per_category_for_questions(self.questions.iter())
    }

    /// Group answered questions by category and return `(category, EM, F1)`.
    #[must_use]
    pub fn scored_only_per_category(&self) -> Vec<(String, f64, f64)> {
        Self::per_category_for_questions(self.questions.iter().filter(|q| q.status.is_scored()))
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "bucket counts are small; f64 mantissa handles them exactly"
    )]
    #[expect(
        clippy::as_conversions,
        reason = "usize to f64 — bucket counts are bounded and small"
    )]
    fn per_category_for_questions<'a>(
        questions: impl Iterator<Item = &'a QuestionResult>,
    ) -> Vec<(String, f64, f64)> {
        use std::collections::BTreeMap;
        let mut buckets: BTreeMap<String, Vec<&QuestionResult>> = BTreeMap::new();
        for q in questions {
            buckets.entry(q.category.clone()).or_default().push(q);
        }
        buckets
            .into_iter()
            .map(|(cat, results)| {
                let total = results.len() as f64; // SAFETY: bucket counts small per function-level #[expect]
                let em = results.iter().filter(|r| r.score.exact_match).count() as f64; // SAFETY: bucket counts small per function-level #[expect]
                let f1_sum: f64 = results.iter().map(|r| r.score.f1).sum();
                (cat, em / total, f1_sum / total)
            })
            .collect()
    }
}
