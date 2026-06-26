//! Intake report tool: parse free-form Slack-style text into a structured
//! report scaffold.
//!
//! Uses keyword-based classification (no LLM call) via [`poiesis_intake`].

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

struct IntakeReportExecutor;

impl ToolExecutor for IntakeReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;
            let Some(text) = args.get("text").and_then(serde_json::Value::as_str) else {
                return Ok(ToolResult::error(
                    "intake_report requires 'text' argument".to_owned(),
                ));
            };

            let req = match poiesis_intake::parse_intake(text) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ToolResult::error(format!("parse failed: {e}")));
                }
            };

            let files = match poiesis_scaffold::scaffold_report(
                &req.slug,
                &req.description,
                poiesis_scaffold::Format::Typst,
                false,
            ) {
                Ok(f) => f,
                Err(e) => {
                    return Ok(ToolResult::error(format!("scaffold failed: {e}")));
                }
            };

            let mut output = format!(
                "Kind: {:?}\nSlug: {}\nDescription: {}\n\nGenerated {} file(s):\n",
                req.kind,
                req.slug,
                req.description,
                files.len()
            );

            for f in &files {
                let _ = std::fmt::Write::write_fmt(
                    &mut output,
                    format_args!(
                        "\n--- {} ---\n{}",
                        f.path.display(),
                        String::from_utf8_lossy(&f.contents)
                    ),
                );
            }

            Ok(ToolResult::text(output))
        })
    }
}

fn intake_report_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("intake_report"), // kanon:ignore RUST/expect
        description: "Parse free-form Slack-style text into a structured report scaffold. \
                      Classifies the request (Analysis, Report, Dashboard, or Unclassified) \
                      via keyword matching and returns skeleton report files."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "text".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "Free-form intake text to classify and scaffold".to_owned(),
                    enum_values: None,
                    default: None,
                    ..Default::default()
                },
            )]),
            required: vec!["text".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Edit, ToolTag::Recon],
    }
}

/// Register the `intake_report` tool into the registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(intake_report_def(), Box::new(IntakeReportExecutor))?;
    Ok(())
}
