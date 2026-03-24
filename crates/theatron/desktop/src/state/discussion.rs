//! Discussion state for planning gray-area questions.

use serde::{Deserialize, Serialize};

/// Priority level for a discussion item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum DiscussionPriority {
    /// Blocking: cannot proceed without an answer.
    Blocking,
    /// Important but not blocking.
    Important,
    /// Nice-to-have clarification.
    NiceToHave,
}

/// Resolution status of a discussion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub(crate) enum DiscussionStatus {
    /// Awaiting a human answer.
    Open,
    /// Answered -- option selected or free-text provided.
    Answered,
    /// Deferred for later.
    Deferred,
}

/// A single option the agent proposes for a discussion question.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct DiscussionOption {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) description: String,
    /// Agent's analysis of pros and cons.
    pub(crate) rationale: String,
    pub(crate) pros: Vec<String>,
    pub(crate) cons: Vec<String>,
    /// Whether this is the agent's recommended choice.
    #[serde(default)]
    pub(crate) recommended: bool,
}

/// An entry in the discussion history (previous answers, reopens, follow-ups).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct DiscussionHistoryEntry {
    pub(crate) action: String,
    pub(crate) actor: String,
    pub(crate) timestamp: String,
    #[serde(default)]
    pub(crate) detail: String,
}

/// A single discussion item -- a gray-area question needing human input.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct Discussion {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) question: String,
    pub(crate) context: String,
    pub(crate) status: DiscussionStatus,
    pub(crate) priority: DiscussionPriority,
    pub(crate) options: Vec<DiscussionOption>,
    /// The selected option id, if answered via option selection.
    #[serde(default)]
    pub(crate) selected_option_id: Option<String>,
    /// Free-text answer override, if provided.
    #[serde(default)]
    pub(crate) free_text_answer: Option<String>,
    #[serde(default)]
    pub(crate) history: Vec<DiscussionHistoryEntry>,
}

/// Request body for answering a discussion.
#[derive(Debug, Serialize)]
pub(crate) struct DiscussionAnswerRequest {
    /// Selected option id, or None for free-text override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) option_id: Option<String>,
    /// Free-text answer override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) free_text: Option<String>,
}

/// Store for discussions associated with the active project.
#[derive(Debug, Clone, Default)]
pub(crate) struct DiscussionStore {
    pub(crate) discussions: Vec<Discussion>,
}

impl DiscussionStore {
    /// Count of discussions awaiting a human answer.
    #[must_use]
    pub(crate) fn open_count(&self) -> usize {
        self.discussions
            .iter()
            .filter(|d| d.status == DiscussionStatus::Open)
            .count()
    }

    /// Count of blocking discussions still open.
    #[must_use]
    pub(crate) fn blocking_count(&self) -> usize {
        self.discussions
            .iter()
            .filter(|d| {
                d.status == DiscussionStatus::Open && d.priority == DiscussionPriority::Blocking
            })
            .count()
    }

    /// Discussions sorted: open first, then blocking before important before nice-to-have.
    #[must_use]
    pub(crate) fn sorted(&self) -> Vec<&Discussion> {
        let mut refs: Vec<&Discussion> = self.discussions.iter().collect();
        refs.sort_by(|a, b| {
            let a_status = status_order(a.status);
            let b_status = status_order(b.status);
            a_status
                .cmp(&b_status)
                .then_with(|| priority_order(a.priority).cmp(&priority_order(b.priority)))
                .then_with(|| a.id.cmp(&b.id))
        });
        refs
    }

    /// Get the selected answer summary for a discussion.
    #[must_use]
    pub(crate) fn answer_summary(discussion: &Discussion) -> Option<String> {
        if let Some(ref text) = discussion.free_text_answer {
            return Some(text.clone());
        }
        if let Some(ref opt_id) = discussion.selected_option_id {
            return discussion
                .options
                .iter()
                .find(|o| &o.id == opt_id)
                .map(|o| o.title.clone());
        }
        None
    }
}

fn status_order(status: DiscussionStatus) -> u8 {
    match status {
        DiscussionStatus::Open => 0,
        DiscussionStatus::Deferred => 1,
        DiscussionStatus::Answered => 2,
    }
}

fn priority_order(priority: DiscussionPriority) -> u8 {
    match priority {
        DiscussionPriority::Blocking => 0,
        DiscussionPriority::Important => 1,
        DiscussionPriority::NiceToHave => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_discussion(
        id: &str,
        status: DiscussionStatus,
        priority: DiscussionPriority,
    ) -> Discussion {
        Discussion {
            id: id.to_string(),
            project_id: "proj1".to_string(),
            question: format!("Question {id}?"),
            context: String::new(),
            status,
            priority,
            options: vec![],
            selected_option_id: None,
            free_text_answer: None,
            history: vec![],
        }
    }

    #[test]
    fn open_count_returns_only_open() {
        let store = DiscussionStore {
            discussions: vec![
                make_discussion("a", DiscussionStatus::Open, DiscussionPriority::Blocking),
                make_discussion(
                    "b",
                    DiscussionStatus::Answered,
                    DiscussionPriority::Important,
                ),
                make_discussion("c", DiscussionStatus::Open, DiscussionPriority::NiceToHave),
                make_discussion(
                    "d",
                    DiscussionStatus::Deferred,
                    DiscussionPriority::Blocking,
                ),
            ],
        };
        assert_eq!(store.open_count(), 2, "only open discussions counted");
    }

    #[test]
    fn blocking_count_returns_open_blocking_only() {
        let store = DiscussionStore {
            discussions: vec![
                make_discussion("a", DiscussionStatus::Open, DiscussionPriority::Blocking),
                make_discussion("b", DiscussionStatus::Open, DiscussionPriority::Important),
                make_discussion(
                    "c",
                    DiscussionStatus::Answered,
                    DiscussionPriority::Blocking,
                ),
                make_discussion("d", DiscussionStatus::Open, DiscussionPriority::Blocking),
            ],
        };
        assert_eq!(
            store.blocking_count(),
            2,
            "only open + blocking discussions counted"
        );
    }

    #[test]
    fn sorted_places_open_before_answered() {
        let store = DiscussionStore {
            discussions: vec![
                make_discussion(
                    "a",
                    DiscussionStatus::Answered,
                    DiscussionPriority::Blocking,
                ),
                make_discussion("b", DiscussionStatus::Open, DiscussionPriority::NiceToHave),
            ],
        };
        let sorted = store.sorted();
        assert_eq!(sorted[0].status, DiscussionStatus::Open);
        assert_eq!(sorted[1].status, DiscussionStatus::Answered);
    }

    #[test]
    fn sorted_places_blocking_before_nice_to_have() {
        let store = DiscussionStore {
            discussions: vec![
                make_discussion("a", DiscussionStatus::Open, DiscussionPriority::NiceToHave),
                make_discussion("b", DiscussionStatus::Open, DiscussionPriority::Blocking),
                make_discussion("c", DiscussionStatus::Open, DiscussionPriority::Important),
            ],
        };
        let sorted = store.sorted();
        assert_eq!(sorted[0].priority, DiscussionPriority::Blocking);
        assert_eq!(sorted[1].priority, DiscussionPriority::Important);
        assert_eq!(sorted[2].priority, DiscussionPriority::NiceToHave);
    }

    #[test]
    fn answer_summary_prefers_free_text() {
        let mut disc = make_discussion(
            "x",
            DiscussionStatus::Answered,
            DiscussionPriority::Important,
        );
        disc.free_text_answer = Some("custom answer".to_string());
        disc.selected_option_id = Some("opt1".to_string());
        disc.options.push(DiscussionOption {
            id: "opt1".to_string(),
            title: "Option One".to_string(),
            description: String::new(),
            rationale: String::new(),
            pros: vec![],
            cons: vec![],
            recommended: false,
        });
        assert_eq!(
            DiscussionStore::answer_summary(&disc),
            Some("custom answer".to_string()),
            "free text takes precedence over selected option"
        );
    }

    #[test]
    fn answer_summary_falls_back_to_option_title() {
        let mut disc = make_discussion(
            "x",
            DiscussionStatus::Answered,
            DiscussionPriority::Important,
        );
        disc.selected_option_id = Some("opt1".to_string());
        disc.options.push(DiscussionOption {
            id: "opt1".to_string(),
            title: "Option One".to_string(),
            description: String::new(),
            rationale: String::new(),
            pros: vec![],
            cons: vec![],
            recommended: false,
        });
        assert_eq!(
            DiscussionStore::answer_summary(&disc),
            Some("Option One".to_string()),
        );
    }

    #[test]
    fn answer_summary_none_when_unanswered() {
        let disc = make_discussion("x", DiscussionStatus::Open, DiscussionPriority::Important);
        assert_eq!(DiscussionStore::answer_summary(&disc), None);
    }
}
