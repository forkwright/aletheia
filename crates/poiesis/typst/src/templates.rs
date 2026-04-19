//! Built-in Typst templates, embedded at compile time.
//!
//! Each template is identified by a short slug string. Call
//! [`crate::render_template`] with the slug to render. Templates expect
//! their data payload to be available at the virtual path `data.json`
//! (which [`crate::render_typst`] populates automatically).

/// Slug for the default one-page report template.
pub const DEFAULT: &str = "default";

/// Source for the [`DEFAULT`] template.
///
/// Embedded at compile time from `templates/default.typ`.
pub const DEFAULT_SOURCE: &str = include_str!("../templates/default.typ");

/// List of all known template slugs.
pub const SLUGS: &[&str] = &[DEFAULT];

/// Resolve a slug to its Typst source, or `None` if unknown.
#[must_use]
pub fn lookup(slug: &str) -> Option<&'static str> {
    match slug {
        DEFAULT => Some(DEFAULT_SOURCE),
        _ => None,
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_slug_is_registered() {
        assert!(SLUGS.contains(&DEFAULT), "DEFAULT must be in SLUGS");
    }

    #[test]
    fn lookup_default_returns_source() {
        let src = lookup(DEFAULT).expect("default must resolve");
        assert!(src.contains("json(\"data.json\")"), "default must load data");
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup("no-such").is_none());
    }
}
