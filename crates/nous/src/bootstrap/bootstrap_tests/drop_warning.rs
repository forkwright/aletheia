//! Regression coverage (#7300) for the bootstrap drop-warning: dropping a
//! section under budget pressure must name both the section and the
//! configured budget cap, not just the near-zero `remaining` figure.

use super::super::*;
use super::{default_budget, setup_oikos};

/// A persona/workspace section larger than the configured bootstrap budget
/// must not be dropped silently. The WARN must name the dropped section
/// *and* the configured budget cap — not only `remaining`, which is
/// near-zero at drop time regardless of how far over budget the content
/// actually was, and gives an operator nothing to correlate against the
/// `bootstrapMaxTokens` config knob.
#[tokio::test]
#[tracing_test::traced_test]
async fn assemble_dropped_section_warns_with_section_and_configured_budget() {
    // SOUL.md (Required) alone consumes the entire budget, so MEMORY.md
    // (Important, truncatable) finds zero tokens remaining and is dropped
    // rather than truncated — see `assemble_optional_dropped` for the same
    // shape without log assertions.
    let large_soul = "x".repeat(2000); // ~500 tokens at 4 chars/token
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", &large_soul), ("MEMORY.md", "memory notes")],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = TokenBudget::new(100_000, 0.0, 0, 500);

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");

    assert!(
        result.sections_dropped.contains(&"MEMORY.md".to_owned()),
        "an over-budget non-required section must be dropped"
    );
    assert!(
        logs_contain("MEMORY.md"),
        "the drop warning must name the dropped section"
    );
    assert!(
        logs_contain("system_budget"),
        "the drop warning must name the configured bootstrap budget"
    );
    assert!(
        logs_contain("system_budget=500"),
        "the drop warning must bind the configured budget's value (500) to the system_budget field"
    );
}

/// Companion to the regression above: when every section fits the
/// configured budget, nothing is dropped and the drop warning never fires.
#[tokio::test]
#[tracing_test::traced_test]
async fn assemble_within_budget_drops_nothing_and_never_warns() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", "identity"), ("MEMORY.md", "short memory note")],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");

    assert!(
        result.sections_dropped.is_empty(),
        "nothing should be dropped when all sections fit comfortably within budget"
    );
    assert!(
        !logs_contain("section dropped"),
        "the drop warning must never fire when every section fits the budget"
    );
}
