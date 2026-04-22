//! Placeholder text and filler patterns commonly found in source code,
//! documents, and generated content.

/// Common placeholder text fragments and filler strings.
///
/// Used to detect unreviewed content, template leakage, or dummy data.
pub const PLACEHOLDER_TEXT_PATTERNS: &[&str] = &[
    "TODO",
    "FIXME",
    "TBD",
    "XXX",
    "HACK",
    "lorem ipsum",
    "dolor sit amet",
    "consectetur adipiscing",
    "qwerty",
    "asdf",
    "123456",
    "000000",
    "placeholder",
    "insert text here",
    "sample text",
    "example text",
];
