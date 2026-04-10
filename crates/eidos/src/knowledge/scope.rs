//! Memory scope and access policy types for team memory.

use serde::{Deserialize, Serialize};

/// Memory sharing scope for multi-agent team memory.
///
/// Each scope maps to a subdirectory under the memory root and defines
/// distinct access control semantics. Scopes form the authorization
/// boundary that [`ScopeAccessPolicy`] enforces; path validation then
/// confirms the resolved filesystem path falls within the correct scope
/// directory.
///
/// Taxonomy mirrors the CC memory type model (`user`, `feedback`,
/// `project`, `reference`) from `memoryTypes.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemoryScope {
    /// Private to the user, never shared with other agents.
    ///
    /// WHY: User memories contain personal context (role, preferences,
    /// knowledge level) that should not leak across agent boundaries.
    User,
    /// Selectively shared corrections and preferences, write-gated to the user.
    ///
    /// WHY: Feedback memories encode behavioral guidance. Agents read them
    /// to avoid repeating mistakes, but only the user can write because
    /// agent-written feedback creates self-reinforcing loops.
    Feedback,
    /// Shared across all agents in a workspace, read-write.
    ///
    /// WHY: Project memories track ongoing work, deadlines, and decisions
    /// that every agent in the workspace needs visibility into.
    Project,
    /// Hybrid: agents read, user curates write access.
    ///
    /// WHY: Reference memories point to external systems (Linear, Grafana,
    /// Slack). Agents need to read them for context but the user controls
    /// what gets indexed because stale pointers are worse than no pointers.
    Reference,
}

impl MemoryScope {
    /// All scope variants in definition order.
    pub const ALL: [Self; 4] = [Self::User, Self::Feedback, Self::Project, Self::Reference];

    /// Return the lowercase string representation of this scope.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Feedback => "feedback",
            Self::Project => "project",
            Self::Reference => "reference",
        }
    }

    /// Directory name for this scope under the memory root.
    ///
    /// Each scope maps to a single subdirectory: `<memory_root>/<dir_name>/`.
    /// The name is identical to `as_str()` by convention.
    #[must_use]
    pub fn as_dir_name(self) -> &'static str {
        // WHY: Directory names match the enum's string representation to keep
        // the mapping predictable and greppable.
        self.as_str()
    }

    /// Access control policy for this scope.
    ///
    /// Returns the static [`ScopeAccessPolicy`] that describes who can
    /// read and write within this scope boundary.
    #[must_use]
    #[expect(
        clippy::match_same_arms,
        reason = "Feedback and Reference share the same access policy VALUES but are semantically distinct scopes with different sharing intent"
    )]
    pub fn access_policy(self) -> ScopeAccessPolicy {
        match self {
            Self::User => ScopeAccessPolicy {
                agent_read: false,
                agent_write: false,
                user_write_only: true,
            },
            Self::Feedback => ScopeAccessPolicy {
                agent_read: true,
                agent_write: false,
                user_write_only: true,
            },
            Self::Project => ScopeAccessPolicy {
                agent_read: true,
                agent_write: true,
                user_write_only: false,
            },
            Self::Reference => ScopeAccessPolicy {
                agent_read: true,
                agent_write: false,
                user_write_only: true,
            },
        }
    }

    /// Parse from a string, returning `None` for unknown values.
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "feedback" => Some(Self::Feedback),
            "project" => Some(Self::Project),
            "reference" => Some(Self::Reference),
            _ => None,
        }
    }
}

impl std::str::FromStr for MemoryScope {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::from_str_opt(s).ok_or_else(|| format!("unknown memory scope: {s}"))
    }
}

/// Access control policy for a [`MemoryScope`].
///
/// Defines who can read and write within a scope boundary. The policy is
/// static per scope variant -- it does not change at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeAccessPolicy {
    /// Whether agents can read memories in this scope.
    pub agent_read: bool,
    /// Whether agents can write memories in this scope.
    pub agent_write: bool,
    /// Whether only the user can write (agent writes are rejected).
    pub user_write_only: bool,
}

impl ScopeAccessPolicy {
    /// Whether an agent is allowed to perform a write operation in this scope.
    #[must_use]
    pub fn permits_agent_write(&self) -> bool {
        self.agent_write && !self.user_write_only
    }

    /// Whether an agent is allowed to perform a read operation in this scope.
    #[must_use]
    pub fn permits_agent_read(&self) -> bool {
        self.agent_read
    }
}
