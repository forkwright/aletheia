//! Stack-based navigation for hierarchical view drill-in/drill-out.

use crate::id::{NousId, SessionId};

/// A distinct view that can be navigated to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum View {
    /// Top-level: agent sidebar + active conversation.
    Home,
    /// Session list for a specific agent.
    Sessions { agent_id: NousId },
    /// Single conversation view.
    Conversation {
        agent_id: NousId,
        session_id: SessionId,
    },
    /// Full message detail (content, tool results, metadata).
    MessageDetail { message_index: usize },
    /// Memory inspector — browsing the knowledge graph.
    MemoryInspector,
    /// Fact detail within the memory inspector.
    FactDetail { fact_id: String },
}

impl View {
    /// Short human-readable label for breadcrumb display.
    pub fn label(&self) -> &str {
        match self {
            Self::Home => "Home",
            Self::Sessions { .. } => "Sessions",
            Self::Conversation { .. } => "Conversation",
            Self::MessageDetail { .. } => "Message",
            Self::MemoryInspector => "Memory",
            Self::FactDetail { .. } => "Fact",
        }
    }
}

/// A stack of views supporting push/pop navigation with breadcrumbs.
///
/// Invariant: the stack always contains at least one element (`View::Home`).
#[derive(Debug, Clone)]
pub struct ViewStack {
    stack: Vec<View>,
}

impl ViewStack {
    pub fn new() -> Self {
        Self {
            stack: vec![View::Home],
        }
    }

    /// Push a new view onto the stack.
    pub fn push(&mut self, view: View) {
        self.stack.push(view);
    }

    /// Pop the current view, returning it. Returns `None` if at Home (cannot pop).
    pub fn pop(&mut self) -> Option<View> {
        if self.stack.len() > 1 {
            self.stack.pop()
        } else {
            None
        }
    }

    /// The currently active view (top of stack).
    #[expect(
        clippy::expect_used,
        reason = "ViewStack invariant: stack is never empty — new() initialises with Home and pop() guards the minimum"
    )]
    pub fn current(&self) -> &View {
        self.stack.last().expect("ViewStack invariant: never empty")
    }

    /// Generate breadcrumb labels for the full navigation path.
    pub fn breadcrumbs(&self) -> Vec<&str> {
        self.stack.iter().map(|v| v.label()).collect()
    }

    /// Current stack depth (1 = Home only).
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Whether we're at the root Home view.
    pub fn is_home(&self) -> bool {
        self.stack.len() == 1
    }
}

impl Default for ViewStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_at_home() {
        let stack = ViewStack::new();
        assert_eq!(stack.current(), &View::Home);
        assert_eq!(stack.depth(), 1);
        assert!(stack.is_home());
    }

    #[test]
    fn push_increases_depth() {
        let mut stack = ViewStack::new();
        stack.push(View::Sessions {
            agent_id: "syn".into(),
        });
        assert_eq!(stack.depth(), 2);
        assert!(!stack.is_home());
    }

    #[test]
    fn push_changes_current() {
        let mut stack = ViewStack::new();
        stack.push(View::Sessions {
            agent_id: "syn".into(),
        });
        assert_eq!(
            stack.current(),
            &View::Sessions {
                agent_id: "syn".into()
            }
        );
    }

    #[test]
    fn pop_returns_to_previous() {
        let mut stack = ViewStack::new();
        stack.push(View::Sessions {
            agent_id: "syn".into(),
        });
        let popped = stack.pop();
        assert_eq!(
            popped,
            Some(View::Sessions {
                agent_id: "syn".into()
            })
        );
        assert_eq!(stack.current(), &View::Home);
        assert!(stack.is_home());
    }

    #[test]
    fn pop_at_home_returns_none() {
        let mut stack = ViewStack::new();
        assert!(stack.pop().is_none());
        assert_eq!(stack.current(), &View::Home);
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn cannot_pop_below_home() {
        let mut stack = ViewStack::new();
        stack.pop();
        stack.pop();
        stack.pop();
        assert_eq!(stack.depth(), 1);
        assert_eq!(stack.current(), &View::Home);
    }

    #[test]
    fn breadcrumbs_single_level() {
        let stack = ViewStack::new();
        assert_eq!(stack.breadcrumbs(), vec!["Home"]);
    }

    #[test]
    fn breadcrumbs_multi_level() {
        let mut stack = ViewStack::new();
        stack.push(View::Sessions {
            agent_id: "syn".into(),
        });
        stack.push(View::Conversation {
            agent_id: "syn".into(),
            session_id: "abc123".into(),
        });
        assert_eq!(
            stack.breadcrumbs(),
            vec!["Home", "Sessions", "Conversation"]
        );
    }

    #[test]
    fn breadcrumbs_after_pop() {
        let mut stack = ViewStack::new();
        stack.push(View::Sessions {
            agent_id: "syn".into(),
        });
        stack.push(View::Conversation {
            agent_id: "syn".into(),
            session_id: "abc123".into(),
        });
        stack.pop();
        assert_eq!(stack.breadcrumbs(), vec!["Home", "Sessions"]);
    }

    #[test]
    fn deep_navigation_chain() {
        let mut stack = ViewStack::new();
        stack.push(View::Sessions {
            agent_id: "syn".into(),
        });
        stack.push(View::Conversation {
            agent_id: "syn".into(),
            session_id: "sess1".into(),
        });
        stack.push(View::MessageDetail { message_index: 5 });
        assert_eq!(stack.depth(), 4);
        assert_eq!(
            stack.breadcrumbs(),
            vec!["Home", "Sessions", "Conversation", "Message"]
        );

        // Pop all the way back
        stack.pop();
        assert_eq!(stack.depth(), 3);
        stack.pop();
        assert_eq!(stack.depth(), 2);
        stack.pop();
        assert!(stack.is_home());
    }

    #[test]
    fn view_labels() {
        assert_eq!(View::Home.label(), "Home");
        assert_eq!(
            View::Sessions {
                agent_id: "x".into()
            }
            .label(),
            "Sessions"
        );
        assert_eq!(
            View::Conversation {
                agent_id: "x".into(),
                session_id: "y".into()
            }
            .label(),
            "Conversation"
        );
        assert_eq!(View::MessageDetail { message_index: 0 }.label(), "Message");
    }

    #[test]
    fn default_is_home() {
        let stack = ViewStack::default();
        assert!(stack.is_home());
        assert_eq!(stack.current(), &View::Home);
    }

    #[test]
    fn view_eq_same_variant_different_data() {
        let a = View::Sessions {
            agent_id: "syn".into(),
        };
        let b = View::Sessions {
            agent_id: "cody".into(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn view_eq_different_variants() {
        let a = View::Home;
        let b = View::Sessions {
            agent_id: "syn".into(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn message_detail_push_pop() {
        let mut stack = ViewStack::new();
        stack.push(View::MessageDetail { message_index: 42 });
        assert_eq!(stack.current(), &View::MessageDetail { message_index: 42 });
        assert_eq!(stack.breadcrumbs(), vec!["Home", "Message"]);
        stack.pop();
        assert!(stack.is_home());
    }
}
