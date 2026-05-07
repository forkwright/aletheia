//! Compaction and restoration prompts.
//!
//! Two distinct prompts for different compaction scenarios:
//! - [`COMPACT_PROMPT`]: mid-session token-budget compaction (terse, decision-focused)
//! - [`RESTORE_PROMPT`]: session-boundary or dream-consolidation restoration
//!   (first-person, tool-trail-preserving)

/// Mid-session token-budget compaction. Discards noise; preserves only
/// decisions made and outstanding questions. Tone: terse, decision-focused,
/// instructional. Output target: dramatically shorter than input (≥60%).
///
/// Fires on: `TokenBudget` hits.
pub const COMPACT_PROMPT: &str = r"You are a context compressor. Given a conversation history, produce a terse summary that is at least 60% shorter than the input while preserving:

- Every decision made and its rationale
- Every file path or code change touched
- Current task state and immediate next steps
- Any errors encountered and how they were resolved

Remove all redundant explanations, conversational filler, and duplicate reasoning. Output only the compressed summary. Use an impersonal, directive tone.";

/// Session-boundary or dream-consolidation restoration. Preserves
/// actionable continuation state: the next action you intended to take,
/// the tool calls you just ran with key results, the working hypothesis.
/// Tone: first-person ("I"), tool-trail-preserving, drops only redundant
/// prose. Output target: a continuation note future-you can act on.
///
/// Fires on: `SessionBoundary`, `DreamConsolidation`.
pub const RESTORE_PROMPT: &str = r#"You are a session restoration assistant. Given a conversation history, produce a first-person continuation note ("I ...") that preserves actionable continuation state:

- The next action I intended to take and why
- The tool calls I just ran, with their key results (file paths, code snippets, command outputs)
- My current working hypothesis or plan
- Any errors I encountered and how I resolved them
- Any open questions I still need to answer

Drop redundant prose and conversational filler, but keep the tool trail intact so I can pick up exactly where I left off. Output only the continuation note."#;
