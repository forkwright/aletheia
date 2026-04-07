//! Query result container for krites v2.

use serde::{Deserialize, Serialize};

use super::value::Value;

/// Query result with named columns and typed rows.
///
/// Returned by all query execution methods. Column headers correspond
/// to the `?[col1, col2, ...]` output specification in Datalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rows {
    /// Column names from the query output specification.
    pub headers: Vec<String>,
    /// Result rows, each with one value per header.
    pub rows: Vec<Vec<Value>>,
}

impl Rows {
    /// Create empty rows with the given headers.
    #[must_use]
    pub fn empty(headers: Vec<String>) -> Self {
        Self {
            headers,
            rows: Vec::new(),
        }
    }

    /// Number of result rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns true if there are no result rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Get the column index for a header name.
    #[must_use]
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.headers.iter().position(|h| h == name)
    }

    /// Extract a single column as an iterator of values.
    pub fn column(&self, name: &str) -> Option<impl Iterator<Item = &Value>> {
        let idx = self.column_index(name)?;
        Some(self.rows.iter().filter_map(move |row| row.get(idx)))
    }

    /// Get a single scalar value (first row, first column).
    ///
    /// Useful for `count()` or single-value aggregation queries.
    #[must_use]
    pub fn scalar(&self) -> Option<&Value> {
        self.rows.first().and_then(|row| row.first())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn sample_rows() -> Rows {
        Rows {
            headers: vec!["id".to_owned(), "name".to_owned(), "score".to_owned()],
            rows: vec![
                vec![Value::from(1_i64), Value::from("alice"), Value::from(0.95)],
                vec![Value::from(2_i64), Value::from("bob"), Value::from(0.87)],
                vec![Value::from(3_i64), Value::from("carol"), Value::from(0.92)],
            ],
        }
    }

    #[test]
    fn len_and_empty() {
        let rows = sample_rows();
        assert_eq!(rows.len(), 3);
        assert!(!rows.is_empty());

        let empty = Rows::empty(vec!["x".to_owned()]);
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
    }

    #[test]
    fn column_index() {
        let rows = sample_rows();
        assert_eq!(rows.column_index("id"), Some(0));
        assert_eq!(rows.column_index("name"), Some(1));
        assert_eq!(rows.column_index("score"), Some(2));
        assert_eq!(rows.column_index("missing"), None);
    }

    #[test]
    fn column_iterator() {
        let rows = sample_rows();
        let names: Vec<&str> = rows
            .column("name")
            .unwrap()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(names, vec!["alice", "bob", "carol"]);
    }

    #[test]
    fn scalar() {
        let rows = Rows {
            headers: vec!["count".to_owned()],
            rows: vec![vec![Value::from(42_i64)]],
        };
        assert_eq!(rows.scalar().and_then(|v| v.as_int()), Some(42));
    }

    #[test]
    fn scalar_empty() {
        let rows = Rows::empty(vec!["x".to_owned()]);
        assert!(rows.scalar().is_none());
    }
}
