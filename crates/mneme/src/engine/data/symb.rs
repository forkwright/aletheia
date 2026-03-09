// Originally derived from CozoDB v0.7.6 (MPL-2.0).
// Copyright 2022, The Cozo Project Authors — see NOTICE for details.

use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::Deref;

use crate::bail;
use crate::engine::error::DbResult as Result;
use compact_str::CompactString;
use serde::{Deserialize, Serialize};

use crate::engine::parse::SourceSpan;

/// Names with associated source span
#[derive(Clone, Deserialize, Serialize)]
pub struct Symbol {
    pub(crate) name: CompactString,
    #[serde(skip)]
    pub(crate) span: SourceSpan,
}

impl Deref for Symbol {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.name
    }
}

impl Hash for Symbol {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state)
    }
}

impl PartialEq for Symbol {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Symbol {}

impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Symbol {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl Debug for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl Symbol {
    pub(crate) fn new(name: impl Into<CompactString>, span: SourceSpan) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }
    pub(crate) fn is_temp_store_name(&self) -> bool {
        self.name.starts_with('_')
    }
    pub(crate) fn is_prog_entry(&self) -> bool {
        self.name == "?"
    }
    pub(crate) fn is_ignored_symbol(&self) -> bool {
        self.name == "_"
    }
    pub(crate) fn is_generated_ignored_symbol(&self) -> bool {
        self.name.starts_with('~')
    }
    pub(crate) fn ensure_valid_field(&self) -> Result<()> {
        if self.name.contains('(') || self.name.contains(')') {
            #[derive(Debug)]
            struct SymbolInvalidAsField(String, SourceSpan);

            impl std::fmt::Display for SymbolInvalidAsField {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "The symbol {} is not valid as a field", self.0)
                }
            }

            impl std::error::Error for SymbolInvalidAsField {}

            bail!(SymbolInvalidAsField(self.name.to_string(), self.span))
        }
        Ok(())
    }
}

pub(crate) const PROG_ENTRY: &str = "?";
