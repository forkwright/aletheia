#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;
use super::{default_budget, setup_oikos};

#[tokio::test]
async fn assemble_output_style_uses_user_communication() {
    let user_content = "\
# User

## Who
- Name: Test

## Communication
- Bullet points only
- No prose";
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", "identity"), ("USER.md", user_content)],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        result
            .sections_included
            .contains(&"output-style".to_owned()),
        "output-style section should be included"
    );
    assert!(
        result.system_prompt.contains("Bullet points only"),
        "output-style should contain USER.md Communication content"
    );
    assert!(
        result.system_prompt.contains("No prose"),
        "output-style should contain full Communication content"
    );
}

#[tokio::test]
async fn assemble_output_style_defaults_without_user() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        result
            .sections_included
            .contains(&"output-style".to_owned()),
        "output-style section should be present even without USER.md"
    );
    assert!(
        result.system_prompt.contains("Answer-first"),
        "output-style should contain default directives when USER.md is absent"
    );
    assert!(
        result.system_prompt.contains("Structure over prose"),
        "output-style should contain default directives"
    );
}

#[tokio::test]
async fn assemble_output_style_defaults_when_no_communication_section() {
    let user_content = "\
# User

## Who
- Name: Test

## Domains
- code";
    let (_dir, oikos) = setup_oikos(
        "test",
        &[("SOUL.md", "identity"), ("USER.md", user_content)],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");
    assert!(
        result.system_prompt.contains("Answer-first"),
        "output-style should use defaults when USER.md has no Communication section"
    );
}

// --- _llm manifest bootstrap tests ---

#[tokio::test]
async fn assemble_llm_cold_start_loads_l1_required_and_l3_optional() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            (
                "_llm:manifest.toml",
                "version = 1\n\n[levels.L3]\npath = \"L3-api-index\"\n",
            ),
            ("_llm:README.md", "workspace overview"),
            ("_llm:L3-api-index/nous.md", "nous api index"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos).with_llm_recipe(LlmRecipe::ColdStart);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional_with_recipe(
            "test",
            &mut budget,
            Vec::new(),
            TaskHint::General,
            LlmRecipe::ColdStart,
        )
        .await
        .expect("assemble should succeed");

    assert!(
        result
            .sections_included
            .contains(&"_llm/README.md".to_owned()),
        "L1 _llm file should be included for ColdStart"
    );
    assert!(
        result
            .sections_included
            .contains(&"_llm/L3-api-index/nous.md".to_owned()),
        "L3 _llm file should be included for ColdStart"
    );
    // L1 should be Required priority, so it sorts before workspace Flexible files
    let l1_pos = result
        .sections_included
        .iter()
        .position(|s| s == "_llm/README.md")
        .expect("L1 should be in sections");
    let soul_pos = result
        .sections_included
        .iter()
        .position(|s| s == "SOUL.md")
        .expect("SOUL.md should be in sections");
    assert!(
        l1_pos > soul_pos,
        "L1 (Important/Required) should sort after SOUL.md (Required) but before Flexible files"
    );
}

#[tokio::test]
async fn assemble_llm_none_skips_all_llm_content() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            (
                "_llm:manifest.toml",
                "version = 1\n\n[levels.L3]\npath = \"L3-api-index\"\n",
            ),
            ("_llm:README.md", "workspace overview"),
            ("_llm:L3-api-index/nous.md", "nous api index"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos).with_llm_recipe(LlmRecipe::None);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional_with_recipe(
            "test",
            &mut budget,
            Vec::new(),
            TaskHint::General,
            LlmRecipe::None,
        )
        .await
        .expect("assemble should succeed");

    assert!(
        !result.system_prompt.contains("workspace overview"),
        "_llm content should be skipped when recipe is None"
    );
    assert!(
        !result
            .sections_included
            .iter()
            .any(|s| s.starts_with("_llm/")),
        "no _llm sections should appear when recipe is None"
    );
}

#[tokio::test]
async fn assemble_llm_missing_directory_no_regression() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "identity")]);
    let assembler = BootstrapAssembler::new(&oikos).with_llm_recipe(LlmRecipe::ColdStart);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional_with_recipe(
            "test",
            &mut budget,
            Vec::new(),
            TaskHint::General,
            LlmRecipe::ColdStart,
        )
        .await
        .expect("assemble should succeed even without _llm/ directory");

    assert_eq!(
        result.sections_included,
        vec!["SOUL.md", "output-style"],
        "bootstrap should work as before when _llm/ is absent"
    );
}

#[tokio::test]
async fn assemble_llm_respects_token_budget() {
    let large_llm = "x".repeat(10_000); // ~2500 tokens at 4 chars/token
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            (
                "_llm:manifest.toml",
                "version = 1\n\n[levels.L3]\npath = \"L3-api-index\"\n",
            ),
            ("_llm:README.md", &large_llm),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos).with_llm_recipe(LlmRecipe::InSession);
    let mut budget = TokenBudget::new(100_000, 0.0, 0, 500);

    let result = assembler
        .assemble_conditional_with_recipe(
            "test",
            &mut budget,
            Vec::new(),
            TaskHint::General,
            LlmRecipe::InSession,
        )
        .await
        .expect("assemble should succeed");

    assert!(
        result.sections_included.contains(&"SOUL.md".to_owned()),
        "SOUL.md should always be included"
    );
    // L1 is Optional for InSession and is truncatable. With ~430 tokens remaining
    // after SOUL.md + output-style, the large _llm content should be truncated to fit.
    assert!(
        result
            .sections_truncated
            .contains(&"_llm/README.md".to_owned()),
        "large optional _llm content should be truncated under tight budget"
    );
    assert!(
        result.total_tokens <= 500,
        "total tokens should respect the bootstrap budget"
    );
}

#[tokio::test]
async fn assemble_llm_refactor_loads_l1_and_l3_important() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            (
                "_llm:manifest.toml",
                "version = 1\n\n[levels.L3]\npath = \"L3-api-index\"\n",
            ),
            ("_llm:architecture.toml", "architecture decisions"),
            ("_llm:L3-api-index/nous.md", "nous api index"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos).with_llm_recipe(LlmRecipe::Refactor);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional_with_recipe(
            "test",
            &mut budget,
            Vec::new(),
            TaskHint::Planning,
            LlmRecipe::Refactor,
        )
        .await
        .expect("assemble should succeed");

    assert!(
        result
            .sections_included
            .contains(&"_llm/architecture.toml".to_owned()),
        "L1 should be included for Refactor recipe"
    );
    assert!(
        result
            .sections_included
            .contains(&"_llm/L3-api-index/nous.md".to_owned()),
        "L3 should be included for Refactor recipe"
    );
}

#[tokio::test]
async fn assemble_llm_malformed_manifest_is_skipped() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "identity"),
            ("_llm:manifest.toml", "this is not valid toml <<<<<"),
            ("_llm:README.md", "workspace overview"),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos).with_llm_recipe(LlmRecipe::ColdStart);
    let mut budget = default_budget();

    let result = assembler
        .assemble_conditional_with_recipe(
            "test",
            &mut budget,
            Vec::new(),
            TaskHint::General,
            LlmRecipe::ColdStart,
        )
        .await
        .expect("assemble should succeed even with malformed manifest");

    assert!(
        !result
            .sections_included
            .iter()
            .any(|s| s.starts_with("_llm/")),
        "malformed manifest should cause graceful skip of all _llm content"
    );
}
