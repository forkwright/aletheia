//! Nous workspace file resources.
//!
//! Exposes agent workspace files (SOUL.md, IDENTITY.md, etc.) as MCP resources.

use rmcp::model::{
    RawResourceTemplate, ReadResourceRequestParams, ResourceContents, ResourceTemplate,
};
use snafu::ResultExt as _;

use koina::id::NousId;
use taxis::oikos::Oikos;

use crate::error::WorkspaceFileSnafu;
use crate::state::DiaporeiaState;

/// Workspace files exposed as resources.
pub(crate) const WORKSPACE_FILES: &[(&str, &str, &str)] = &[
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
    let content = read_resource_content(state.oikos.as_ref(), uri)?;

    Ok(vec![ResourceContents::text(content, uri)])
}

/// Return whether a concrete nous workspace resource exists and is readable.
///
/// WHY(#4635): `resources/list` should only advertise files that can actually
/// be read; unreadable or missing files are silently omitted.
pub(crate) fn resource_exists(oikos: &Oikos, uri: &str) -> bool {
    let Ok((nous_id, filename)) = parse_resource_uri(uri) else {
        return false;
    };
    let Ok(path) = oikos.contained_nous_file(&nous_id, filename) else {
        return false;
    };
    std::fs::metadata(&path).is_ok_and(|m| m.is_file())
}

fn read_resource_content(oikos: &Oikos, uri: &str) -> Result<String, rmcp::ErrorData> {
    let (nous_id, filename) = parse_resource_uri(uri)?;

    let file_path = oikos.contained_nous_file(&nous_id, filename).map_err(|e| {
        rmcp::ErrorData::internal_error(crate::sanitize::strip_paths(&e.to_string()), None)
    })?;
    std::fs::read_to_string(&file_path)
        .context(WorkspaceFileSnafu {})
        .map_err(rmcp::ErrorData::from)
}

pub(crate) fn parse_resource_uri(uri: &str) -> Result<(NousId, &'static str), rmcp::ErrorData> {
    let path = uri
        .strip_prefix("aletheia://nous/")
        .ok_or_else(|| rmcp::ErrorData::invalid_params("invalid nous resource URI", None))?;

    let (raw_nous_id, file_slug) = path.split_once('/').ok_or_else(|| {
        rmcp::ErrorData::invalid_params("expected aletheia://nous/{id}/{file}", None)
    })?;
    if file_slug.contains('/') {
        return Err(rmcp::ErrorData::invalid_params(
            "expected aletheia://nous/{id}/{file}",
            None,
        ));
    }

    let nous_id = NousId::new(raw_nous_id.to_owned())
        .map_err(|e| rmcp::ErrorData::invalid_params(format!("invalid nous id: {e}"), None))?;

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

    Ok((nous_id, filename))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn write_test_file(path: &std::path::Path, contents: &[u8]) {
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(contents).unwrap();
    }

    #[test]
    fn resource_templates_returns_five_entries() {
        let templates = resource_templates();
        assert_eq!(
            templates.len(),
            5,
            "expected one template per workspace file"
        );
    }

    #[test]
    fn resource_templates_uris_use_nous_scheme() {
        let templates = resource_templates();
        for t in &templates {
            let uri = t.raw.uri_template.as_str();
            assert!(
                uri.starts_with("aletheia://nous/"),
                "URI must use aletheia://nous/ scheme: {uri}"
            );
        }
    }

    #[test]
    fn resource_templates_include_nous_id_placeholder() {
        let templates = resource_templates();
        for t in &templates {
            let uri = t.raw.uri_template.as_str();
            assert!(
                uri.contains("{nous_id}"),
                "URI template must include {{nous_id}}: {uri}"
            );
        }
    }

    #[test]
    fn resource_templates_mime_type_is_markdown() {
        let templates = resource_templates();
        for t in &templates {
            assert_eq!(
                t.raw.mime_type.as_deref(),
                Some("text/markdown"),
                "workspace files must be served as markdown"
            );
        }
    }

    #[test]
    fn resource_templates_cover_core_workspace_files() {
        let templates = resource_templates();
        let uris: Vec<&str> = templates
            .iter()
            .map(|t| t.raw.uri_template.as_str())
            .collect();
        for slug in &["soul", "identity", "memory", "goals", "tools"] {
            assert!(
                uris.iter().any(|u| u.ends_with(slug)),
                "expected template for '{slug}' workspace file"
            );
        }
    }

    #[test]
    fn parse_resource_uri_accepts_valid_nous_id() {
        let (nous_id, filename) =
            parse_resource_uri("aletheia://nous/alice/soul").expect("valid URI");

        assert_eq!(nous_id.as_str(), "alice");
        assert_eq!(filename, "SOUL.md");
    }

    #[test]
    fn parse_resource_uri_rejects_traversal_nous_id() {
        let err = parse_resource_uri("aletheia://nous/../soul").unwrap_err();

        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn parse_resource_uri_rejects_separator_in_nous_id() {
        let err = parse_resource_uri("aletheia://nous/alice/../soul").unwrap_err();

        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn parse_resource_uri_rejects_absolute_path_nous_id() {
        let err = parse_resource_uri("aletheia://nous//tmp/soul").unwrap_err();

        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[test]
    #[cfg(unix)]
    fn read_resource_content_rejects_symlink_outside_instance_root() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let outside = tempfile::tempdir().expect("create outside temp dir");
        let outside_file = outside.path().join("SOUL.md");
        write_test_file(&outside_file, b"# Escape\n");
        let link = dir.path().join("nous/alice/SOUL.md");
        std::fs::create_dir_all(link.parent().expect("link parent")).unwrap();
        std::os::unix::fs::symlink(&outside_file, &link).unwrap();

        let oikos = Oikos::from_root(dir.path());
        let err = read_resource_content(&oikos, "aletheia://nous/alice/soul").unwrap_err();

        assert_eq!(err.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert!(
            !err.message
                .contains(outside.path().to_string_lossy().as_ref()),
            "resource errors must not expose escaped server paths"
        );
    }
}
