//! Tests for the two-axis bootstrap sort: slot (role) primary, priority (importance) secondary.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: hashmap keys are valid after filtering"
)]

use super::super::*;
use super::{default_budget, setup_oikos};

#[tokio::test]
async fn soul_persona_before_operator_profile() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", "Be terse."), ("USER.md", "Be verbose.")],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");

    let soul_pos = result
        .sections_included
        .iter()
        .position(|s| s == "SOUL.md")
        .expect("SOUL.md should be included");
    let user_pos = result
        .sections_included
        .iter()
        .position(|s| s == "USER.md")
        .expect("USER.md should be included");
    assert!(
        soul_pos < user_pos,
        "SOUL.md (SoulPersona) should appear before USER.md (OperatorProfile)"
    );
}

#[tokio::test]
async fn empty_soul_is_skipped_gracefully() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", ""), ("USER.md", "Be verbose.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");

    assert!(
        !result.sections_included.contains(&"SOUL.md".to_owned()),
        "empty SOUL.md should be skipped"
    );
    assert!(
        result.sections_included.contains(&"USER.md".to_owned()),
        "USER.md should still be included"
    );
}

#[tokio::test]
async fn missing_soul_with_user_errors() {
    let (_dir, oikos) = setup_oikos("test", &[("USER.md", "Be verbose.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let err = assembler
        .assemble("test", &mut budget)
        .await
        .expect_err("assemble should fail without SOUL.md");
    assert!(
        err.to_string().contains("SOUL.md"),
        "error should mention missing SOUL.md"
    );
}

#[tokio::test]
async fn slot_equal_priority_determines_order() {
    // Two files at the same slot (Context) with different priorities.
    // We use extra pack sections to create this situation.
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", "identity"), ("CONTEXT.md", "runtime")],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let extra = vec![
        BootstrapSection {
            name: "context-required".to_owned(),
            priority: SectionPriority::Required,
            content: "required context".to_owned(),
            tokens: 2,
            truncatable: false,
            slot: BootstrapSlot::Context,
        },
        BootstrapSection {
            name: "context-optional".to_owned(),
            priority: SectionPriority::Optional,
            content: "optional context".to_owned(),
            tokens: 2,
            truncatable: true,
            slot: BootstrapSlot::Context,
        },
    ];

    let result = assembler
        .assemble_with_extra("test", &mut budget, extra)
        .await
        .expect("assemble should succeed");

    let req_pos = result
        .sections_included
        .iter()
        .position(|s| s == "context-required")
        .expect("context-required should be included");
    let opt_pos = result
        .sections_included
        .iter()
        .position(|s| s == "context-optional")
        .expect("context-optional should be included");
    assert!(
        req_pos < opt_pos,
        "Required context should appear before Optional context at the same slot"
    );
}

#[tokio::test]
async fn full_slot_precedence_order() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("CONTEXT.md", "runtime config"),
            ("SOUL.md", "identity"),
            ("IDENTITY.md", "name and emoji"),
            ("USER.md", "operator"),
            ("PROSOCHE.md", "checklist"),
            ("AGENTS.md", "team"),
            ("GOALS.md", "goals"),
            ("TOOLS.md", "tools"),
            ("CHECKLIST.md", "procedures"),
            ("MEMORY.md", "memory"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");

    let positions: std::collections::HashMap<&str, usize> = result
        .sections_included
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let name = s.as_str();
            if std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            {
                Some((name, i))
            } else {
                None
            }
        })
        .collect();

    assert!(
        positions["IDENTITY.md"] < positions["SOUL.md"],
        "Identity before SoulPersona"
    );
    assert!(
        positions["SOUL.md"] < positions["USER.md"],
        "SoulPersona before OperatorProfile"
    );
    assert!(
        positions["USER.md"] < positions["PROSOCHE.md"],
        "OperatorProfile before Prosoche"
    );
    assert!(
        positions["PROSOCHE.md"] < positions["AGENTS.md"],
        "Prosoche before Team"
    );
    assert!(
        positions["AGENTS.md"] < positions["GOALS.md"],
        "Team before Goals"
    );
    assert!(
        positions["GOALS.md"] < positions["TOOLS.md"],
        "Goals before Tools"
    );
    assert!(
        positions["TOOLS.md"] < positions["CHECKLIST.md"],
        "Tools before Checklist"
    );
    assert!(
        positions["CHECKLIST.md"] < positions["MEMORY.md"],
        "Checklist before Memory"
    );
    assert!(
        positions["MEMORY.md"] < positions["CONTEXT.md"],
        "Memory before Context"
    );
}

#[tokio::test]
async fn required_section_not_debted_by_lower_slot_flexible() {
    // WHY(#5829): A Flexible section with a lower slot index must not consume
    // budget ahead of a Required section with a higher slot index.
    let identity = "x".repeat(116); // 29 tokens at the default 4 chars/token
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", "identity"), ("IDENTITY.md", &identity)],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = TokenBudget::new(200_000, 0.6, 16_384, 30);

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");

    assert!(
        result.sections_included.contains(&"SOUL.md".to_owned()),
        "SOUL.md must be included"
    );
    assert!(
        result.sections_dropped.contains(&"IDENTITY.md".to_owned()),
        "Flexible IDENTITY.md should be dropped after Required SOUL.md takes priority"
    );
    assert_eq!(
        budget.adjusted_history_budget(),
        budget.history_budget(),
        "no history debt should accrue when Required fits and Flexible is dropped"
    );
}
