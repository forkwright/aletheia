//! Keyword lists for task classification and intent detection.

/// Keywords that suggest a coding or implementation task.
///
/// Sourced from `nous/src/bootstrap/mod.rs`.
pub const CODING_KEYWORDS: &[&str] = &[
    "code",
    "implement",
    "fix",
    "bug",
    "compile",
    "test",
    "refactor",
    "debug",
    "build",
    "error",
    "function",
    "struct",
    "deploy",
    "lint",
];

/// Keywords that suggest a research or investigation task.
///
/// Sourced from `nous/src/bootstrap/mod.rs`.
pub const RESEARCH_KEYWORDS: &[&str] = &[
    "research",
    "find",
    "search",
    "investigate",
    "analyze",
    "review",
    "compare",
    "evaluate",
    "explain",
    "understand",
];

/// Keywords that suggest a planning or design task.
///
/// Sourced from `nous/src/bootstrap/mod.rs`.
pub const PLANNING_KEYWORDS: &[&str] = &[
    "plan",
    "design",
    "architect",
    "strategy",
    "roadmap",
    "organize",
    "coordinate",
    "priority",
    "goal",
    "milestone",
];

/// Keywords that suggest a casual conversation rather than a task.
///
/// Sourced from `nous/src/bootstrap/mod.rs`.
pub const CONVERSATION_KEYWORDS: &[&str] = &[
    "hello",
    "hi",
    "hey",
    "thanks",
    "thank you",
    "ok",
    "okay",
    "yes",
    "no",
    "sure",
    "bye",
];
