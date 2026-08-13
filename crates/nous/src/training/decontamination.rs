//! Authorship decontamination gate for training and DPO capture.
//!
//! # Ownership
//!
//! This module is the single owner of the question "may this turn enter a
//! corpus?". Both capture paths — [`crate::training::TrainingCapture`] for the
//! supervised corpus and the DPO pair extractor in [`crate::pipeline`] — route
//! through [`DecontaminationGate::evaluate`]. They previously carried
//! independent copies of the classify-and-threshold logic, and the copies had
//! already drifted: one returned "allow" from the error arm and the other
//! returned "pass", spelling the same fail-open behaviour two ways with no
//! single place to correct it.
//!
//! # Observability
//!
//! ## Metrics
//! | Metric | Type | Labels | Condition |
//! |--------|------|--------|-----------|
//! | `aletheia_training_capture_rejected_total` | counter | `nous_id`, `class` | Per turn the gate withheld from the corpus |

use std::sync::Arc;

use aletheia_classify::{AuthorClass, Classifier};
use mneme::training::{DecontaminationPolicy, TrainingConfig};
use tracing::{debug, warn};

/// Verdict label for a turn the classifier attributed to the user.
pub(crate) const VERDICT_USER: &str = "user";
/// Verdict label for a turn the classifier attributed to a non-user author.
pub(crate) const VERDICT_NON_USER: &str = "non_user";
/// Verdict label for a turn the classifier could not classify.
pub(crate) const VERDICT_CLASSIFIER_ERROR: &str = "classifier_error";
/// Verdict label for a turn the gate did not screen.
pub(crate) const VERDICT_NOT_SCREENED: &str = "not_screened";

/// What the gate decided to do with a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disposition {
    /// Write the turn to the corpus.
    Admit,
    /// Write the turn to the quarantine shard instead of the corpus.
    Quarantine,
    /// Do not persist the turn at all.
    Drop,
}

impl Disposition {
    /// Whether the turn is kept out of the training corpus.
    pub(crate) fn withholds_from_corpus(self) -> bool {
        !matches!(self, Self::Admit)
    }
}

/// A decontamination decision plus the provenance to record alongside it.
#[derive(Debug, Clone)]
pub(crate) struct Verdict {
    /// What to do with the turn.
    pub(crate) disposition: Disposition,
    /// Stable verdict label, one of the `VERDICT_*` constants.
    pub(crate) label: &'static str,
    /// Classifier artifact version, `None` when the turn was not screened.
    pub(crate) classifier_version: Option<String>,
}

impl Verdict {
    /// The verdict for a turn no policy screened.
    fn not_screened() -> Self {
        Self {
            disposition: Disposition::Admit,
            label: VERDICT_NOT_SCREENED,
            classifier_version: None,
        }
    }
}

/// Screens candidate corpus rows against the configured authorship policy.
#[derive(Debug)]
pub(crate) struct DecontaminationGate {
    /// Classifier instance, `None` when the policy does not screen.
    classifier: Option<Arc<Classifier>>,
    /// Confidence at or above which a non-user class is acted on.
    threshold: f32,
    /// Policy governing disposition of non-user text and classifier failures.
    policy: DecontaminationPolicy,
}

impl DecontaminationGate {
    /// Build a gate from training configuration.
    ///
    /// Constructs a classifier only when the policy screens, so a
    /// `Disabled` policy costs nothing.
    pub(crate) fn from_config(config: &TrainingConfig) -> Self {
        let classifier = config
            .decontamination_policy
            .screens()
            .then(|| Arc::new(Classifier::new()));
        Self {
            classifier,
            threshold: config.author_classifier_threshold,
            policy: config.decontamination_policy,
        }
    }

    /// The policy this gate enforces.
    pub(crate) fn policy(&self) -> DecontaminationPolicy {
        self.policy
    }

    /// Replace the classifier instance.
    ///
    /// WHY retained: tests and callers that load a classifier from a real
    /// artifact directory need to substitute it after construction.
    pub(crate) fn set_classifier(&mut self, classifier: Option<Arc<Classifier>>) {
        self.classifier = classifier;
    }

    /// Decide whether `user_message` may enter a corpus.
    ///
    /// `session_id` and `nous_id` are used for logging and the rejection
    /// metric only; they do not affect the decision.
    pub(crate) fn evaluate(&self, user_message: &str, session_id: &str, nous_id: &str) -> Verdict {
        let Some(classifier) = &self.classifier else {
            return Verdict::not_screened();
        };
        let version = classifier.metadata().artifact_version.clone();

        match classifier.classify(user_message) {
            Ok(probs) => {
                let class = probs.argmax();
                let confidence = probs.confidence();
                if class == AuthorClass::User || confidence < self.threshold {
                    return Verdict {
                        disposition: Disposition::Admit,
                        label: VERDICT_USER,
                        classifier_version: Some(version),
                    };
                }
                let disposition = self.disposition_for_screened_out();
                if disposition.withholds_from_corpus() {
                    crate::metrics::record_training_capture_rejected(nous_id, class.as_str());
                }
                debug!(
                    session_id,
                    class = class.as_str(),
                    confidence,
                    policy = self.policy.as_str(),
                    disposition = ?disposition,
                    "decontamination gate screened out non-user text"
                );
                Verdict {
                    disposition,
                    label: VERDICT_NON_USER,
                    classifier_version: Some(version),
                }
            }
            Err(e) => {
                // WHY: a classifier that cannot answer is not evidence that
                // the turn is user-authored. Admitting it was the fail-open
                // path this gate exists to close, so every policy above
                // `Warn` withholds the turn instead (#5382).
                let disposition = self.disposition_for_screened_out();
                if disposition.withholds_from_corpus() {
                    crate::metrics::record_training_capture_rejected(
                        nous_id,
                        VERDICT_CLASSIFIER_ERROR,
                    );
                }
                warn!(
                    error = %e,
                    session_id,
                    policy = self.policy.as_str(),
                    disposition = ?disposition,
                    "authorship classification failed"
                );
                Verdict {
                    disposition,
                    label: VERDICT_CLASSIFIER_ERROR,
                    classifier_version: Some(version),
                }
            }
        }
    }

    /// Map the policy onto a disposition for a turn the gate screened out.
    ///
    /// `Disabled` cannot reach here: the gate returns early without a
    /// classifier, so it is folded into `Admit`.
    fn disposition_for_screened_out(&self) -> Disposition {
        match self.policy {
            DecontaminationPolicy::Disabled | DecontaminationPolicy::Warn => Disposition::Admit,
            DecontaminationPolicy::Quarantine => Disposition::Quarantine,
            // WHY the wildcard joins `FailClosed`: `DecontaminationPolicy` is
            // `#[non_exhaustive]`, and an unrecognised future policy must not
            // widen the corpus.
            DecontaminationPolicy::FailClosed | _ => Disposition::Drop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Longer than `aletheia_classify::MAX_TEXT_LENGTH`, so `classify`
    /// returns `TextTooLong` — the reachable classifier-error path.
    fn unclassifiable() -> String {
        "x".repeat(200_000)
    }

    fn gate(policy: DecontaminationPolicy) -> DecontaminationGate {
        DecontaminationGate::from_config(&TrainingConfig {
            decontamination_policy: policy,
            author_classifier_threshold: 0.85,
            ..TrainingConfig::default()
        })
    }

    #[test]
    fn disabled_policy_does_not_screen() {
        let v = gate(DecontaminationPolicy::Disabled).evaluate("anything", "s", "n");
        assert_eq!(v.disposition, Disposition::Admit);
        assert_eq!(v.label, VERDICT_NOT_SCREENED);
        assert!(v.classifier_version.is_none());
    }

    #[test]
    fn classifier_error_is_admitted_under_warn() {
        let v = gate(DecontaminationPolicy::Warn).evaluate(&unclassifiable(), "s", "n");
        assert_eq!(v.label, VERDICT_CLASSIFIER_ERROR);
        assert_eq!(v.disposition, Disposition::Admit);
    }

    #[test]
    fn classifier_error_is_quarantined_under_quarantine() {
        let v = gate(DecontaminationPolicy::Quarantine).evaluate(&unclassifiable(), "s", "n");
        assert_eq!(v.label, VERDICT_CLASSIFIER_ERROR);
        assert_eq!(v.disposition, Disposition::Quarantine);
    }

    // WHY: this is the regression test for #5382. Before the policy existed
    // both capture paths admitted an unclassifiable turn outright.
    #[test]
    fn classifier_error_is_dropped_under_fail_closed() {
        let v = gate(DecontaminationPolicy::FailClosed).evaluate(&unclassifiable(), "s", "n");
        assert_eq!(v.label, VERDICT_CLASSIFIER_ERROR);
        assert_eq!(v.disposition, Disposition::Drop);
        assert!(v.disposition.withholds_from_corpus());
    }

    #[test]
    fn screened_turns_record_the_classifier_version() {
        let v = gate(DecontaminationPolicy::Warn).evaluate("hello there", "s", "n");
        assert_eq!(
            v.classifier_version.as_deref(),
            Some(Classifier::new().metadata().artifact_version.as_str())
        );
    }
}
