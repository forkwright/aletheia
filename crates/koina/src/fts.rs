//! Full-text-search query sanitization shared by every Krites-backed FTS query path.
//!
//! Krites' embedded Datalog engine parses the `query:` argument of
//! `~facts:content_fts{... query: $query_text ...}` (and the equivalent skill
//! search) using CozoDB's own full-text query grammar, in which `?`, `*`,
//! `"`, parentheses, hyphens, underscores, and boolean keywords are operators
//! rather than literal text. Binding raw user text (e.g. any question, which
//! ends in `?`) can trigger an FTS parse error that must not surface as a
//! false "no results" for the caller (#4156).

/// Reduce free text to a bag-of-terms full-text-search query safe for
/// Krites' Cozo-derived FTS grammar.
///
/// Keeps only alphanumeric word tokens, joined by a single space; every
/// other character — including `-` and `_`, which the grammar itself treats
/// as operators/separators, not word content — is a token boundary. Returns
/// an empty string when the input has no word characters; callers should
/// treat that as "no text query" rather than binding an empty (and
/// therefore FTS-parse-invalid) query string.
///
/// WHY(#7020): the single owner for FTS query sanitization, previously
/// reimplemented independently in `episteme::knowledge_store::marshal` and
/// `nous::skills`. The Nous copy preserved hyphens and underscores as word
/// content, so the #4156 correction (treating those as FTS-operator
/// characters, not text) applied to only one of the two query paths that
/// reach the same engine.
#[must_use]
pub fn sanitize_fts_query(raw: &str) -> String {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_question_mark() {
        assert_eq!(
            sanitize_fts_query("what do you remember about datalog?"),
            "what do you remember about datalog"
        );
    }

    #[test]
    fn drops_fts_operators_and_punctuation() {
        assert_eq!(
            sanitize_fts_query("\"foo*\" AND (bar) -baz?!"),
            "foo AND bar baz"
        );
    }

    #[test]
    fn splits_on_hyphen_and_underscore() {
        // WHY(#7020): hyphens/underscores are FTS-grammar separators, not
        // word content — this is the #4156 behavior Nous's copy lacked.
        assert_eq!(
            sanitize_fts_query("rust-error_handling"),
            "rust error handling"
        );
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(sanitize_fts_query("  hello\t\nworld  "), "hello world");
    }

    #[test]
    fn empty_when_no_word_chars() {
        assert_eq!(sanitize_fts_query("???"), "");
        assert_eq!(sanitize_fts_query(""), "");
    }
}
