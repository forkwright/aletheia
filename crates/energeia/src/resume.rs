// WHY: Resume policy defines escalating intervention messages injected into
// stuck sessions. Each stage has a turn budget; when the session exhausts all
// stages it is marked Stuck.

use serde::{Deserialize, Serialize};

/// Multi-stage resume policy for stuck or stalled sessions.
///
/// Each stage has a turn budget and an escalating urgency message injected
/// into the session to redirect the agent's behavior. When all stages are
/// exhausted the session is marked `Stuck`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResumePolicy {
    /// Ordered stages of escalating intervention.
    pub stages: Vec<ResumeStage>,
}

/// A single stage in a resume escalation sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResumeStage {
    /// Maximum turns allowed in this stage before escalating to the next.
    pub max_turns: u32,
    /// Message injected into the session at this escalation level.
    pub message: String,
}

impl Default for ResumePolicy {
    /// Three-stage default escalation covering roughly 200 turns total.
    ///
    /// Stage 1 (80 turns): gentle nudge — plenty of turns remain.
    /// Stage 2 (100 turns): focused redirect — file issues for blockers.
    /// Stage 3 (50 turns): final push — validate, commit, push, create PR.
    fn default() -> Self {
        Self {
            stages: vec![
                ResumeStage {
                    max_turns: 80,
                    message: "Continue the task. Plenty of turns remaining.".to_owned(),
                },
                ResumeStage {
                    max_turns: 100,
                    message: "Focus on criteria. File issues for what you can't fix.".to_owned(),
                },
                ResumeStage {
                    max_turns: 50,
                    message: "Final attempt. Run validation, commit, push, create PR.".to_owned(),
                },
            ],
        }
    }
}

impl ResumePolicy {
    /// Return the resume stage appropriate for the current total turn count.
    ///
    /// Stages are activated by cumulative turn thresholds. Stage 0 covers turns
    /// `0..max_turns[0]`, stage 1 covers `max_turns[0]..sum(max_turns[0..=1])`, and
    /// so on. Returns `None` when all stage budgets have been consumed — the
    /// caller should mark the session as `Stuck`.
    ///
    /// # Example
    /// ```ignore
    /// let policy = ResumePolicy::default();
    /// assert!(policy.next_stage(0).is_some());    // stage 0
    /// assert!(policy.next_stage(79).is_some());   // still stage 0
    /// assert!(policy.next_stage(80).is_some());   // stage 1
    /// assert!(policy.next_stage(229).is_some());  // stage 2 (80+100+49)
    /// assert!(policy.next_stage(230).is_none());  // exhausted
    /// ```
    #[must_use]
    pub fn next_stage(&self, current_turns: u32) -> Option<&ResumeStage> {
        let mut cumulative: u32 = 0;
        for stage in &self.stages {
            cumulative = cumulative.saturating_add(stage.max_turns);
            if current_turns < cumulative {
                return Some(stage);
            }
        }
        None
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_has_three_stages() {
        let policy = ResumePolicy::default();
        assert_eq!(policy.stages.len(), 3, "default should have 3 stages");
    }

    #[test]
    fn default_stages_have_escalating_urgency() {
        let policy = ResumePolicy::default();
        let s0 = &policy.stages[0];
        let s2 = &policy.stages[2];
        // Stage 0 has a larger budget (plenty of turns) than stage 2 (final push).
        assert!(
            s0.max_turns > s2.max_turns,
            "stage 0 ({}) should have more turns than stage 2 ({})",
            s0.max_turns,
            s2.max_turns
        );
    }

    #[test]
    fn default_stage_messages_match_spec() {
        let policy = ResumePolicy::default();
        assert!(
            policy.stages[0].message.contains("Plenty of turns"),
            "stage 0 should be gentle: {}",
            policy.stages[0].message
        );
        assert!(
            policy.stages[1].message.contains("Focus on criteria"),
            "stage 1 should redirect focus: {}",
            policy.stages[1].message
        );
        assert!(
            policy.stages[2].message.contains("Final attempt"),
            "stage 2 should be final push: {}",
            policy.stages[2].message
        );
    }

    #[test]
    fn next_stage_returns_first_stage_at_zero_turns() {
        let policy = ResumePolicy::default();
        let stage = policy.next_stage(0).unwrap();
        assert!(
            stage.message.contains("Plenty of turns"),
            "should return stage 0 at turn 0"
        );
    }

    #[test]
    fn next_stage_stays_in_stage_0_before_threshold() {
        let policy = ResumePolicy::default();
        // Stage 0 covers 0..80 (max_turns = 80).
        let at_79 = policy.next_stage(79).unwrap();
        assert!(
            at_79.message.contains("Plenty of turns"),
            "should still be stage 0 at turn 79"
        );
    }

    #[test]
    fn next_stage_transitions_at_cumulative_boundary() {
        let policy = ResumePolicy::default();
        // Stage 0: 80 turns. At turn 80 we should enter stage 1.
        let at_80 = policy.next_stage(80).unwrap();
        assert!(
            at_80.message.contains("Focus on criteria"),
            "should enter stage 1 at turn 80, got: {}",
            at_80.message
        );
    }

    #[test]
    fn next_stage_transitions_to_stage_2() {
        let policy = ResumePolicy::default();
        // Stage 0: 80 + stage 1: 100 = 180. At turn 180 we enter stage 2.
        let at_180 = policy.next_stage(180).unwrap();
        assert!(
            at_180.message.contains("Final attempt"),
            "should enter stage 2 at turn 180, got: {}",
            at_180.message
        );
    }

    #[test]
    fn next_stage_returns_none_when_exhausted() {
        let policy = ResumePolicy::default();
        // Total budget: 80 + 100 + 50 = 230. At turn 230: all stages exhausted.
        assert!(
            policy.next_stage(230).is_none(),
            "should return None when all stages exhausted"
        );
        assert!(
            policy.next_stage(500).is_none(),
            "should return None far beyond exhaustion"
        );
    }

    #[test]
    fn next_stage_empty_policy_always_returns_none() {
        let policy = ResumePolicy { stages: vec![] };
        assert!(
            policy.next_stage(0).is_none(),
            "empty policy always exhausted"
        );
    }

    #[test]
    fn custom_policy_stages() {
        let policy = ResumePolicy {
            stages: vec![
                ResumeStage {
                    max_turns: 10,
                    message: "first".to_owned(),
                },
                ResumeStage {
                    max_turns: 5,
                    message: "second".to_owned(),
                },
            ],
        };
        assert_eq!(policy.next_stage(0).unwrap().message, "first");
        assert_eq!(policy.next_stage(9).unwrap().message, "first");
        assert_eq!(policy.next_stage(10).unwrap().message, "second");
        assert_eq!(policy.next_stage(14).unwrap().message, "second");
        assert!(policy.next_stage(15).is_none());
    }

    #[test]
    fn roundtrip_serialization() {
        let policy = ResumePolicy::default();
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: ResumePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.stages.len(), 3);
        assert_eq!(deserialized.stages[0].max_turns, policy.stages[0].max_turns);
        assert_eq!(deserialized.stages[0].message, policy.stages[0].message);
    }
}
