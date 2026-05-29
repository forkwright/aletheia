//! Regression tests for the three hard-won daimon agent constraints (#4109).
//!
//! Each test verifies one constraint so a future regression can be
//! pinpointed immediately rather than traced through integration tests.

#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;
use super::{default_budget, setup_oikos};

// ── Constraint 1: two-channel identity injection ────────────────────────────

/// SOUL.md is Required — it must survive even under extreme budget pressure.
///
/// This is the "core identity channel": a Required section cannot be dropped
/// by the budget trimmer regardless of how many other sections compete for tokens.
#[tokio::test]
async fn soul_required_survives_extreme_budget_pressure() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "Core identity text."),
            ("USER.md", "Operator profile."),
            ("CONTEXT.md", "Runtime context, large filler."),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    // Tiny budget: system_budget = min(5000 - 100 - 3000, 40) = 40 tokens.
    // Enough for SOUL.md (~8 tokens); not enough for large operational files.
    let mut budget = crate::budget::TokenBudget::new(5_000, 0.6, 100, 40);

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed even under extreme budget pressure");

    assert!(
        result.sections_included.contains(&"SOUL.md".to_owned()),
        "SOUL.md (Required) must be included even under extreme budget pressure"
    );
}

/// IDENTITY.md uses the Identity slot (slot 0) so it appears before SOUL.md.
///
/// Under budget pressure, IDENTITY.md (Flexible) may be dropped while SOUL.md
/// (Required) survives — the two slots provide independent priority tracks.
#[tokio::test]
async fn identity_slot_before_soul_persona_slot() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "Agent persona."),
            ("IDENTITY.md", "Agent name and avatar."),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");

    let identity_pos = result
        .sections_included
        .iter()
        .position(|s| s == "IDENTITY.md")
        .expect("IDENTITY.md should be included when budget allows");
    let soul_pos = result
        .sections_included
        .iter()
        .position(|s| s == "SOUL.md")
        .expect("SOUL.md should always be included");

    assert!(
        identity_pos < soul_pos,
        "IDENTITY.md (Identity slot=0) must appear before SOUL.md (SoulPersona slot=1)"
    );
}

/// SOUL.md (Required) survives when IDENTITY.md (Flexible) is dropped.
///
/// With a tiny budget, Flexible sections drop first. SOUL.md must remain.
#[tokio::test]
async fn soul_survives_when_identity_dropped() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "Persona text that must survive."),
            ("IDENTITY.md", "Optional identity metadata."),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    // Very small budget: system_budget = min(5000 - 100 - 3000, 40) = 40 tokens.
    // SOUL.md (~12 tokens) fits; IDENTITY.md (~12 tokens) also fits in theory.
    // SOUL.md is Required so it must survive regardless.
    let mut budget = crate::budget::TokenBudget::new(5_000, 0.6, 100, 40);

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");

    assert!(
        result.sections_included.contains(&"SOUL.md".to_owned()),
        "SOUL.md (Required) must survive when budget forces Flexible sections to drop"
    );
    // IDENTITY.md may or may not be dropped depending on exact token counts,
    // but SOUL.md must always be present regardless.
}

// ── Constraint 3: voice-exemplar cueing ─────────────────────────────────────

/// VOICE.md (when present) is loaded and positioned in the `SoulPersona` slot.
///
/// This anchors the model's output style near the top of context, immediately
/// after SOUL.md, from the first token (#4109 constraint 3).
#[tokio::test]
async fn voice_md_loaded_when_present() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "Agent persona."),
            ("VOICE.md", "## Examples\n\nSample output style text here."),
        ],
    );
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed");

    assert!(
        result.sections_included.contains(&"VOICE.md".to_owned()),
        "VOICE.md must be loaded when present to satisfy voice-exemplar constraint"
    );
    assert!(
        result
            .system_prompt
            .contains("Sample output style text here."),
        "VOICE.md content must appear in the system prompt"
    );
}

/// VOICE.md is silently absent when the file does not exist.
///
/// The Optional priority ensures a missing VOICE.md does not cause errors.
#[tokio::test]
async fn voice_md_absent_when_missing() {
    let (_dir, oikos) = setup_oikos("test", &[("SOUL.md", "Agent persona.")]);
    let assembler = BootstrapAssembler::new(&oikos);
    let mut budget = default_budget();

    let result = assembler
        .assemble("test", &mut budget)
        .await
        .expect("assemble should succeed without VOICE.md");

    assert!(
        !result.sections_included.contains(&"VOICE.md".to_owned()),
        "missing VOICE.md must not cause an error or appear in sections"
    );
}

/// VOICE.md appears after SOUL.md within the `SoulPersona` slot.
///
/// Both use `BootstrapSlot::SoulPersona`. Within the same slot, sections are
/// sorted by priority: SOUL.md (Required=0) before VOICE.md (Optional=3).
/// This keeps core persona before the voice exemplar.
#[tokio::test]
async fn voice_md_after_soul_in_same_slot() {
    let (_dir, oikos) = setup_oikos(
        "test",
        &[
            ("SOUL.md", "Agent persona."),
            ("VOICE.md", "Voice exemplar sample."),
        ],
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
        .expect("SOUL.md should always be included");
    let voice_pos = result
        .sections_included
        .iter()
        .position(|s| s == "VOICE.md")
        .expect("VOICE.md should be included when budget allows");

    assert!(
        soul_pos < voice_pos,
        "SOUL.md (Required) must appear before VOICE.md (Optional) in the same SoulPersona slot"
    );
}
