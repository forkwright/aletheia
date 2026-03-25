#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;
use super::{default_budget, setup_oikos};

#[tokio::test]
async fn assemble_conditional_coding_loads_tools_and_memory() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("TOOLS.md", "tool list"),
            ("MEMORY.md", "memory"),
            ("AGENTS.md", "team topology"),
            ("GOALS.md", "goals"),
            ("CONTEXT.md", "runtime config"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional("test", &mut budget, Vec::new(), TaskHint::Coding)
        .await
        .expect("assemble_conditional should succeed");

    assert!(
        result.sections_included.contains(&"SOUL.md".to_owned()),
        "SOUL.md (identity tier) should always be included"
    );
    assert!(
        result.sections_included.contains(&"TOOLS.md".to_owned()),
        "TOOLS.md should be included for Coding hint"
    );
    assert!(
        result.sections_included.contains(&"MEMORY.md".to_owned()),
        "MEMORY.md should be included for Coding hint"
    );
    assert!(
        !result.sections_included.contains(&"AGENTS.md".to_owned()),
        "AGENTS.md should be filtered out for Coding hint"
    );
    assert!(
        !result.sections_included.contains(&"GOALS.md".to_owned()),
        "GOALS.md should be filtered out for Coding hint"
    );
    assert!(
        result.sections_filtered.contains(&"AGENTS.md".to_owned()),
        "AGENTS.md should appear in sections_filtered"
    );
    assert!(
        result.sections_filtered.contains(&"GOALS.md".to_owned()),
        "GOALS.md should appear in sections_filtered"
    );
    assert_eq!(
        result.task_hint,
        TaskHint::Coding,
        "result should record the task hint used"
    );
}

#[tokio::test]
async fn assemble_conditional_research_loads_goals_and_context() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("TOOLS.md", "tool list"),
            ("MEMORY.md", "memory"),
            ("GOALS.md", "goals"),
            ("CONTEXT.md", "runtime config"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional("test", &mut budget, Vec::new(), TaskHint::Research)
        .await
        .expect("assemble_conditional should succeed");

    assert!(
        result.sections_included.contains(&"GOALS.md".to_owned()),
        "GOALS.md should be included for Research hint"
    );
    assert!(
        result.sections_included.contains(&"CONTEXT.md".to_owned()),
        "CONTEXT.md should be included for Research hint"
    );
    assert!(
        result.sections_included.contains(&"MEMORY.md".to_owned()),
        "MEMORY.md should be included for Research hint"
    );
    assert!(
        !result.sections_included.contains(&"TOOLS.md".to_owned()),
        "TOOLS.md should be filtered out for Research hint"
    );
    assert!(
        result.sections_filtered.contains(&"TOOLS.md".to_owned()),
        "TOOLS.md should appear in sections_filtered"
    );
}

#[tokio::test]
async fn assemble_conditional_planning_loads_goals_agents_context() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("AGENTS.md", "team topology"),
            ("GOALS.md", "goals"),
            ("TOOLS.md", "tool list"),
            ("CONTEXT.md", "runtime config"),
            ("MEMORY.md", "memory"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional("test", &mut budget, Vec::new(), TaskHint::Planning)
        .await
        .expect("assemble_conditional should succeed");

    assert!(
        result.sections_included.contains(&"GOALS.md".to_owned()),
        "GOALS.md should be included for Planning hint"
    );
    assert!(
        result.sections_included.contains(&"AGENTS.md".to_owned()),
        "AGENTS.md should be included for Planning hint"
    );
    assert!(
        result.sections_included.contains(&"CONTEXT.md".to_owned()),
        "CONTEXT.md should be included for Planning hint"
    );
    assert!(
        !result.sections_included.contains(&"TOOLS.md".to_owned()),
        "TOOLS.md should be filtered out for Planning hint"
    );
    assert!(
        !result.sections_included.contains(&"MEMORY.md".to_owned()),
        "MEMORY.md should be filtered out for Planning hint"
    );
}

#[tokio::test]
async fn assemble_conditional_conversation_loads_identity_only() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("USER.md", "user info"),
            ("IDENTITY.md", "name and emoji"),
            ("PROSOCHE.md", "checklist"),
            ("AGENTS.md", "team topology"),
            ("GOALS.md", "goals"),
            ("TOOLS.md", "tool list"),
            ("MEMORY.md", "memory"),
            ("CONTEXT.md", "runtime config"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional("test", &mut budget, Vec::new(), TaskHint::Conversation)
        .await
        .expect("assemble_conditional should succeed");

    // WHY: only identity-tier files should be included
    assert!(
        result.sections_included.contains(&"SOUL.md".to_owned()),
        "SOUL.md should be included for Conversation"
    );
    assert!(
        result.sections_included.contains(&"USER.md".to_owned()),
        "USER.md should be included for Conversation"
    );
    assert!(
        result.sections_included.contains(&"IDENTITY.md".to_owned()),
        "IDENTITY.md should be included for Conversation"
    );
    assert!(
        result.sections_included.contains(&"PROSOCHE.md".to_owned()),
        "PROSOCHE.md should be included for Conversation"
    );
    assert_eq!(
        result.sections_included.len(),
        4,
        "only 4 identity-tier files should be included for Conversation"
    );
    // WHY: all operational files should be filtered
    assert_eq!(
        result.sections_filtered.len(),
        6,
        "all 6 conditional files should be filtered for Conversation"
    );
}

#[tokio::test]
async fn assemble_conditional_general_loads_all() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("USER.md", "user info"),
            ("AGENTS.md", "team topology"),
            ("GOALS.md", "goals"),
            ("TOOLS.md", "tool list"),
            ("MEMORY.md", "memory"),
            ("IDENTITY.md", "name and emoji"),
            ("PROSOCHE.md", "checklist"),
            ("CONTEXT.md", "runtime config"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional("test", &mut budget, Vec::new(), TaskHint::General)
        .await
        .expect("assemble_conditional should succeed");

    assert_eq!(
        result.sections_included.len(),
        9,
        "General hint should load all 9 present workspace files"
    );
    assert!(
        result.sections_filtered.is_empty(),
        "General hint should not filter any files"
    );
    assert_eq!(
        result.task_hint,
        TaskHint::General,
        "result should record General task hint"
    );
}

#[tokio::test]
async fn assemble_conditional_with_checklist() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("TOOLS.md", "tool list"),
            ("CHECKLIST.md", "work procedures"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional("test", &mut budget, Vec::new(), TaskHint::Coding)
        .await
        .expect("assemble_conditional should succeed");

    assert!(
        result
            .sections_included
            .contains(&"CHECKLIST.md".to_owned()),
        "CHECKLIST.md should be included for Coding hint"
    );
    assert!(
        result.sections_included.contains(&"TOOLS.md".to_owned()),
        "TOOLS.md should be included for Coding hint"
    );
}

#[tokio::test]
async fn assemble_backward_compat_loads_all() {
    // WHY: assemble() without hint should behave identically to pre-change behavior
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("AGENTS.md", "team topology"),
            ("GOALS.md", "goals"),
            ("TOOLS.md", "tool list"),
            ("MEMORY.md", "memory"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed with backward-compatible behavior");

    assert_eq!(
        result.sections_included.len(),
        5,
        "assemble() without hint should load all present files"
    );
    assert!(
        result.sections_filtered.is_empty(),
        "assemble() without hint should not filter any files"
    );
    assert_eq!(
        result.task_hint,
        TaskHint::General,
        "assemble() should use General hint by default"
    );
}
