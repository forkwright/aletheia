//! Text prefixes and phrase patterns for detection and classification.

/// Phrases that indicate the user is issuing a behavioral correction.
///
/// Simple keyword matching is intentionally conservative. False negatives
/// (missed corrections) are preferable to false positives (storing random
/// sentences as corrections).
///
/// Sourced from `nous/src/hooks/builtins/correction.rs`.
pub const CORRECTION_PREFIXES: &[&str] = &[
    "don't ",
    "do not ",
    "stop ",
    "never ",
    "always ",
    "from now on",
    "remember to ",
    "make sure to ",
    "please don't ",
    "please do not ",
    "please always ",
    "please never ",
    "you should always ",
    "you should never ",
    "you must always ",
    "you must never ",
    "i need you to always ",
    "i need you to never ",
];
