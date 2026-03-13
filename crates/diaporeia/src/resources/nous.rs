//! Nous workspace file resources.
//!
//! Exposes agent workspace files (SOUL.md, IDENTITY.md, etc.) as MCP resources.

use rmcp::model::{
    RawResourceTemplate, ReadResourceRequestParams, ResourceContents, ResourceTemplate,
};
use snafu::ResultExt as _;

use crate::error::WorkspaceFileSnafu;
use crate::state::DiaporeiaState;

/// Workspace files exposed as resources.
const WORKSPACE_FILES: &[(&str, &str, &str)] = &[
    (
        "soul",
        "Nous SOUL",
        "Character and principles for a nous agent",
    ),
    (
        "identity",
        "Nous Identity",
        "Name and emoji identity for a nous agent",
    ),
    (
        "memory",
        "Nous Memory",
        "Persistent knowledge for a nous agent",
    ),
    ("goals", "Nous Goals", "Active goals for a nous agent"),
    ("tools", "Nous Tools", "Tool inventory for a nous agent"),
];

/// Build resource templates for nous workspace files.
pub(crate) fn resource_templates() -> Vec<ResourceTemplate> {
    WORKSPACE_FILES
        .iter()
        .map(|(slug, name, desc)| {
            let raw =
                RawResourceTemplate::new(format!("aletheia://nous/{{nous_id}}/{slug}"), *name)
                    .with_description(*desc)
                    .with_mime_type("text/markdown");
            ResourceTemplate {
                raw,
                annotations: None,
            }
        })
        .collect()
}

/// Read a nous workspace file resource.
///
/// URI format: `aletheia://nous/{nous_id}/{file}`
pub(crate) fn read_resource(
    state: &DiaporeiaState,
    params: &ReadResourceRequestParams,
) -> Result<Vec<ResourceContents>, rmcp::ErrorData> {
    let uri = params.uri.as_str();

    // Parse: aletheia://nous/{nous_id}/{file}
    let path = uri
        .strip_prefix("aletheia://nous/")
        .ok_or_else(|| rmcp::ErrorData::invalid_params("invalid nous resource URI", None))?;

    let (nous_id, file_slug) = path.split_once('/').ok_or_else(|| {
        rmcp::ErrorData::invalid_params("expected aletheia://nous/{id}/{file}", None)
    })?;

    let filename = match file_slug {
        "soul" => "SOUL.md",
        "identity" => "IDENTITY.md",
        "memory" => "MEMORY.md",
        "goals" => "GOALS.md",
        "tools" => "TOOLS.md",
        other => {
            return Err(rmcp::ErrorData::invalid_params(
                format!("unknown workspace file: {other}"),
                None,
            ));
        }
    };

    let file_path = state.oikos.nous_file(nous_id, filename);
    let content = std::fs::read_to_string(&file_path)
        .context(WorkspaceFileSnafu {})
        .map_err(rmcp::ErrorData::from)?;

    Ok(vec![ResourceContents::text(content, uri)])
}
