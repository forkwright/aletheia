use std::collections::BTreeMap;

use crate::engine::DataValue;

use super::schema::*;

/// Accumulates Datalog script lines and parameter bindings.
#[must_use]
pub struct QueryBuilder {
    lines: Vec<String>,
    params: BTreeMap<String, DataValue>,
}

impl QueryBuilder {
    /// Create an empty query builder.
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            params: BTreeMap::new(),
        }
    }

    /// Start a `:put` operation against a relation.
    pub fn put(self, relation: Relation) -> PutBuilder {
        PutBuilder {
            parent: self,
            relation,
            all_fields: Vec::new(),
            key_count: 0,
            rows: Vec::new(),
        }
    }

    /// Start a `?[...] := *relation{...}` scan query.
    pub fn scan(self, relation: Relation) -> ScanBuilder {
        ScanBuilder {
            parent: self,
            relation,
            select: Vec::new(),
            bindings: Vec::new(),
            filters: Vec::new(),
            order: None,
            limit: None,
        }
    }

    /// Append a raw Datalog line (escape hatch for complex queries).
    pub fn raw(mut self, line: &str) -> Self {
        self.lines.push(line.to_owned());
        self
    }

    /// Bind a named parameter.
    pub fn param(mut self, name: &str, value: DataValue) -> Self {
        self.params.insert(name.to_owned(), value);
        self
    }

    /// Consume the builder, producing `(script, params)`.
    pub fn build(self) -> (String, BTreeMap<String, DataValue>) {
        (self.lines.join("\n"), self.params)
    }

    /// Consume the builder, producing only the script string.
    pub fn build_script(self) -> String {
        self.lines.join("\n")
    }
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds a `:put relation { keys => values }` operation.
#[must_use]
pub struct PutBuilder {
    parent: QueryBuilder,
    relation: Relation,
    all_fields: Vec<&'static str>,
    key_count: usize,
    rows: Vec<Vec<String>>,
}

impl PutBuilder {
    /// Declare key fields (before the `=>` in the `:put` clause).
    pub fn keys(mut self, fields: &[impl Field]) -> Self {
        self.key_count = fields.len();
        for f in fields {
            self.all_fields.push(f.name());
        }
        self
    }

    /// Declare value fields (after the `=>` in the `:put` clause).
    pub fn values(mut self, fields: &[impl Field]) -> Self {
        for f in fields {
            self.all_fields.push(f.name());
        }
        self
    }

    /// Add an explicit row with custom param references.
    ///
    /// Each entry is a Datalog expression: `"$param_name"`, `"null"`, a quoted
    /// literal like `"\"9999-12-31\""`, etc. Required for multi-row puts where
    /// different rows bind different params (e.g. `SUPERSEDE_FACT`).
    pub fn row(mut self, exprs: &[&str]) -> Self {
        self.rows
            .push(exprs.iter().map(|s| (*s).to_owned()).collect());
        self
    }

    /// Finish the `:put`, returning the parent `QueryBuilder`.
    ///
    /// If no explicit `row()` was called, generates a single row from field
    /// names (convention: `$field_name` for each field).
    pub fn done(mut self) -> QueryBuilder {
        if self.rows.is_empty() {
            let auto_row: Vec<String> = self.all_fields.iter().map(|f| format!("${f}")).collect();
            self.rows.push(auto_row);
        }

        let field_list = self.all_fields.join(", ");

        let row_strs: Vec<String> = self
            .rows
            .iter()
            .map(|r| format!("[{}]", r.join(", ")))
            .collect();
        let data = row_strs.join(", ");

        let key_fields: Vec<&str> = self.all_fields[..self.key_count].to_vec();
        let value_fields: Vec<&str> = self.all_fields[self.key_count..].to_vec();

        let put_clause = if value_fields.is_empty() {
            format!(
                ":put {} {{{}}}",
                self.relation.name(),
                key_fields.join(", ")
            )
        } else {
            format!(
                ":put {} {{{} => {}}}",
                self.relation.name(),
                key_fields.join(", "),
                value_fields.join(", ")
            )
        };

        let line = format!("?[{field_list}] <- [{data}]\n{put_clause}");
        self.parent.lines.push(line);
        self.parent
    }
}

/// Builds a `?[select] := *relation{bindings}, filters` query.
#[must_use]
pub struct ScanBuilder {
    parent: QueryBuilder,
    relation: Relation,
    select: Vec<&'static str>,
    bindings: Vec<String>,
    filters: Vec<String>,
    order: Option<String>,
    limit: Option<String>,
}

impl ScanBuilder {
    /// Set the `?[...]` projection fields.
    pub fn select(mut self, fields: &[impl Field]) -> Self {
        self.select = fields.iter().map(|f| f.name()).collect();
        self
    }

    /// Bind a field in the `*relation{...}` clause (just the field name).
    pub fn bind(mut self, field: impl Field) -> Self {
        self.bindings.push(field.name().to_owned());
        self
    }

    /// Bind a field to an expression: `field: expr` in `*relation{...}`.
    pub fn bind_to(mut self, field: impl Field, expr: &str) -> Self {
        self.bindings.push(format!("{}: {expr}", field.name()));
        self
    }

    /// Add a filter condition (raw Datalog expression after the scan clause).
    pub fn filter(mut self, expr: &str) -> Self {
        self.filters.push(expr.to_owned());
        self
    }

    /// Set `:order` directive (e.g. `"-confidence"`).
    pub fn order(mut self, expr: &str) -> Self {
        self.order = Some(expr.to_owned());
        self
    }

    /// Set `:limit` directive (e.g. `"$limit"`).
    pub fn limit(mut self, expr: &str) -> Self {
        self.limit = Some(expr.to_owned());
        self
    }

    /// Finish the scan, returning the parent `QueryBuilder`.
    pub fn done(mut self) -> QueryBuilder {
        let select_list = self.select.join(", ");
        let binding_list = self.bindings.join(", ");

        let mut parts = vec![format!(
            "?[{select_list}] :=\n    *{}{{{binding_list}}}",
            self.relation.name()
        )];

        for f in &self.filters {
            parts.push(format!("    {f}"));
        }

        let mut line = parts.join(",\n");

        if let Some(ref ord) = self.order {
            use std::fmt::Write;
            let _ = write!(line, "\n:order {ord}");
        }
        if let Some(ref lim) = self.limit {
            use std::fmt::Write;
            let _ = write!(line, "\n:limit {lim}");
        }

        self.parent.lines.push(line);
        self.parent
    }
}
