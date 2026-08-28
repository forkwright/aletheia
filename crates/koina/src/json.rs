//! Shared pretty-or-compact JSON display projection.
//!
//! WHY(#7031): organon's tool-schema responses and proskenion's tool panel
//! both rendered a [`serde_json::Value`] with the identical policy —
//! pretty-print, falling back to the value's compact `Display` form if
//! serialization somehow fails — as independent copies. One canonical
//! projection here means both surfaces render tool-inspection JSON
//! identically by construction, not by coincidence.

/// Pretty-print a JSON value for human display, falling back to its compact
/// form if pretty-printing fails.
#[must_use]
pub fn pretty_or_compact(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_is_pretty_printed_with_indentation() {
        let value = serde_json::json!({"a": 1});
        assert_eq!(pretty_or_compact(&value), "{\n  \"a\": 1\n}");
    }

    #[test]
    fn scalar_renders_compact() {
        let value = serde_json::json!(42);
        assert_eq!(pretty_or_compact(&value), "42");
    }

    #[test]
    fn null_renders_compact() {
        let value = serde_json::Value::Null;
        assert_eq!(pretty_or_compact(&value), "null");
    }
}
