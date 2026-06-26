//! Read a lazy-loaded skill by name from the knowledge store.
//!
//! Returns the full SKILL.md body (formatted from the stored `SkillContent`)
//! so the agent can load a skill on demand.  If no skill with the given name
//! exists, returns an error result.

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

/// Minimal skill body for deserialising knowledge-store JSON without a
/// dependency on `mneme` / `episteme`.
#[derive(Debug, Clone, serde::Deserialize)]
struct SkillBody {
    name: String,
    description: String,
    #[serde(default)]
    steps: Vec<String>,
    #[serde(default)]
    tools_used: Vec<String>,
    #[serde(default)]
    domain_tags: Vec<String>,
    #[serde(default)]
    triggers: Vec<String>,
    #[serde(default)]
    always: bool,
}

/// Format a [`SkillBody`] as a SKILL.md with YAML frontmatter.
fn format_skill_md(skill: &SkillBody) -> String {
    use std::fmt::Write as _;
    let mut md = String::with_capacity(512);

    md.push_str("---\n");
    let _ = writeln!(md, "name: {}", skill.name);
    let desc_needs_quoting = skill.description.contains(':')
        || skill.description.contains('#')
        || skill.description.contains('"');
    if desc_needs_quoting {
        let escaped = skill.description.replace('"', r#"\""#);
        let _ = writeln!(md, "description: \"{escaped}\"");
    } else {
        let _ = writeln!(md, "description: {}", skill.description);
    }
    if !skill.tools_used.is_empty() {
        let _ = writeln!(md, "tools: [{}]", skill.tools_used.join(", "));
    }
    if !skill.domain_tags.is_empty() {
        let _ = writeln!(md, "domains: [{}]", skill.domain_tags.join(", "));
    }
    if !skill.triggers.is_empty() {
        let _ = writeln!(md, "triggers: [{}]", skill.triggers.join(", "));
    }
    if skill.always {
        md.push_str("always: true\n");
    }
    md.push_str("---\n\n");

    let _ = writeln!(md, "# {}\n", skill.name);
    md.push_str("## When to Use\n");
    let _ = writeln!(md, "{}\n", skill.description);

    if !skill.steps.is_empty() {
        md.push_str("## Steps\n");
        for (i, step) in skill.steps.iter().enumerate() {
            let _ = writeln!(md, "{}. {}", i + 1, step);
        }
        md.push('\n');
    }

    if !skill.tools_used.is_empty() {
        md.push_str("## Tools Used\n");
        for tool in &skill.tools_used {
            let _ = writeln!(md, "- {tool}");
        }
        md.push('\n');
    }

    if !skill.domain_tags.is_empty() {
        md.push_str("## Tags\n");
        md.push_str(&skill.domain_tags.join(", "));
    }

    md
}

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
                    let skill = match serde_json::from_str::<SkillBody>(&content_json) {
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
                    ..Default::default(),
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
    registry.register(skill_read_def(), Box::new(SkillReadExecutor))
}
