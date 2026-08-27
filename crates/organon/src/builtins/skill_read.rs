//! Read a lazy-loaded skill by name from the knowledge store.
//!
//! Returns the full SKILL.md body (formatted from the stored `SkillContent`)
//! so the agent can load a skill on demand.  If no skill with the given name
//! exists, returns an error result.
//!
//! WHY: deserializes stored skill JSON directly into `eidos::skill_md::SkillMd`
//! and renders through `eidos::skill_md::format_skill_md` -- the single owner
//! of the SKILL.md projection, also used by `episteme::skill::format_skill_md`
//! -- rather than a private duplicate. `eidos` is a dependency-light shared
//! types crate (no `mneme`/`episteme` pull-in); deserializing into its
//! narrower `SkillMd` silently ignores extra stored fields (e.g. `origin`)
//! the renderer does not need.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use eidos::skill_md::{SkillMd, format_skill_md};
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, RollbackSupport, ToolCapabilityMetadata,
    ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput, ToolResult, ToolStability, ToolTag,
};

// ── Executor ─────────────────────────────────────────────────────────────────

struct SkillReadExecutor;

impl ToolExecutor for SkillReadExecutor {
    #[tracing::instrument(skip(self, input, ctx), fields(skill_name = ?input.arguments.get("name").and_then(|v| v.as_str())))]
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(skill_name) = input.arguments.get("name").and_then(|v| v.as_str()) else {
                return Ok(ToolResult::error("missing required field: name"));
            };

            let Some(ref services) = ctx.services else {
                return Ok(ToolResult::error("knowledge services unavailable"));
            };
            let Some(ref knowledge) = services.knowledge else {
                return Ok(ToolResult::error("knowledge store unavailable"));
            };

            match knowledge
                .find_skill_by_name(ctx.nous_id.as_str(), skill_name)
                .await
            {
                Ok(Some(content_json)) => {
                    let skill = match serde_json::from_str::<SkillMd>(&content_json) {
                        Ok(s) => s,
                        Err(e) => {
                            return Ok(ToolResult::error(format!(
                                "skill '{skill_name}' found but content is malformed: {e}"
                            )));
                        }
                    };
                    let md = format_skill_md(&skill);
                    Ok(ToolResult::text(md))
                }
                Ok(None) => Ok(ToolResult::error(format!(
                    "skill not found: '{skill_name}'"
                ))),
                Err(e) => Ok(ToolResult::error(format!(
                    "knowledge store error looking up skill '{skill_name}': {e}"
                ))),
            }
        })
    }
}

// ── ToolDef ──────────────────────────────────────────────────────────────────

fn skill_read_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("skill_read"),
        description: "Load the full body of a lazy-loaded skill by name. \
             Call this when the system prompt hints that a skill is relevant \
             to the current task. Returns the complete SKILL.md content."
            .to_owned(),
        extended_description: Some(
            "Skills marked as lazy-loaded (always: false) appear only as one-line \
             summaries in the system prompt. When you need the full skill body, \
             call skill_read with the skill's name to retrieve it."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "name".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "Name of the skill to load (e.g. \"refactor-pattern\")".to_owned(),
                    enum_values: None,
                    default: None,
                    ..Default::default()
                },
            )]),
            required: vec!["name".to_owned()],
        },
        category: ToolCategory::Research,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

// ── Registration ─────────────────────────────────────────────────────────────

/// Register the `skill_read` tool into `registry`.
///
/// # Errors
///
/// Returns an error if `skill_read` is already registered.
pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(skill_read_def(), Box::new(SkillReadExecutor))?;
    registry.declare_capability(
        ToolName::from_static("skill_read"),
        ToolCapabilityMetadata {
            owner: "organon::builtins::skill_read".to_owned(),
            stability: ToolStability::Stable,
            rollback: RollbackSupport::Supported,
            ..ToolCapabilityMetadata::default()
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY(#7026): stored skill JSON is shaped like
    /// `episteme::skill::SkillContent`'s serialization (including the
    /// `origin` field, which `SkillMd` does not declare), proving the lazy
    /// `skill_read` path tolerates the real wire shape and renders through
    /// the same canonical formatter as the eager export path -- so a future
    /// edit to `eidos::skill_md::format_skill_md` cannot leave this path
    /// behind without this test catching the drift.
    #[test]
    fn renders_canonical_frontmatter_from_stored_skill_content_shape() {
        let stored_json = serde_json::json!({
            "name": "refactor-pattern",
            "description": "Refactor a pattern",
            "steps": ["do it"],
            "tools_used": ["Read", "Edit"],
            "domain_tags": ["rust"],
            "origin": "manual",
            "triggers": [],
            "always": false,
        })
        .to_string();

        let skill: SkillMd = serde_json::from_str(&stored_json).expect("known-good shape parses");
        let md = format_skill_md(&skill);

        assert!(
            md.contains("allowed-tools: Read, Edit"),
            "lazy-loaded skill_read output must carry allowed-tools for Claude Code, same as the eager export path: {md}"
        );
        assert!(
            md.contains("tools: [Read, Edit]"),
            "lazy-loaded skill_read output must still carry tools for Aletheia's own parser: {md}"
        );
    }
}
