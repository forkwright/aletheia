//! Lint rules for basanos.

pub mod architecture;
pub mod planning;
pub mod vocabulary;
pub mod writing;

use crate::error::Result;

/// A single lint violation found by a rule.
///
/// The `message` may begin with `[warn]` or `[error]` to indicate severity.
/// For v1, `ARCHITECTURE/fact-required` emits `[warn]` messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Rule identifier, e.g. `PLANNING/missing-falsifier`.
    pub rule: String,
    /// File path where the violation was found.
    pub path: String,
    /// Approximate line number (1-based).
    pub line: usize,
    /// Human-readable message.
    pub message: String,
}

/// A lint rule that can be applied to a project tree.
pub trait Rule {
    /// Short `snake_case` identifier for the rule.
    fn id(&self) -> &'static str;

    /// Run the rule against the given project root.
    fn check(&self, project_root: &str) -> Result<Vec<Violation>>;
}

/// All registered rules.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(planning::MissingFalsifierRule),
        Box::new(architecture::fact_required::FactRequiredRule),
        Box::new(writing::PurposeInTechnicalDocRule),
        Box::new(vocabulary::HubWordDisciplineRule::new()),
        Box::new(writing::ReferenceMustCompressRule),
    ]
}
