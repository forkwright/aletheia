//! One escaping policy for every XML-shaped Poiesis output.
//!
//! ODT, OOXML (`theme1.xml`, PPTX), and the SVG chart emitter all write text
//! into XML character data or attribute values. Before this module they each
//! hand-rolled the same substitution table, and the tables had already
//! drifted (one dropped the apostrophe escape). Delegating to `quick_xml`'s
//! proven implementation makes every sink agree by construction rather than
//! by each author copying the same four lines correctly.

use quick_xml::escape::escape;

/// Escape `&`, `<`, `>`, `"`, and `'` for safe use in XML character data or a
/// quoted XML attribute value.
///
/// This is the single escaping policy for every Poiesis XML-shaped output
/// (SVG, OOXML, ODT). Do not hand-roll a substitution table for a new sink —
/// call this instead.
#[must_use]
pub fn escape_xml(s: &str) -> String {
    escape(s).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_all_reserved_characters() {
        assert_eq!(
            escape_xml("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
    }

    #[test]
    fn leaves_plain_text_untouched() {
        assert_eq!(escape_xml("plain text"), "plain text");
    }
}
