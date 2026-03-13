//! Configuration resources.

use rmcp::model::{
    RawResourceTemplate, ReadResourceRequestParams, ResourceContents, ResourceTemplate,
};
use snafu::ResultExt as _;

use crate::error::SerializationSnafu;
use crate::state::DiaporeiaState;

/// Build resource templates for config resources.
pub(crate) fn resource_templates() -> Vec<ResourceTemplate> {
    let raw = RawResourceTemplate::new("aletheia://config", "Aletheia Configuration")
        .with_description("Runtime configuration (sensitive fields redacted)")
        .with_mime_type("application/json");
    vec![ResourceTemplate {
        raw,
        annotations: None,
    }]
}

/// Read a configuration resource.
pub(crate) async fn read_resource(
    state: &DiaporeiaState,
    params: &ReadResourceRequestParams,
) -> Result<Vec<ResourceContents>, rmcp::ErrorData> {
    let uri = params.uri.as_str();

    if uri != "aletheia://config" {
        return Err(rmcp::ErrorData::invalid_params(
            format!("unknown config resource: {uri}"),
            None,
        ));
    }

    let config = state.config.read().await;

    let redacted = serde_json::json!({
        "gateway": {
            "port": config.gateway.port,
            "bind": config.gateway.bind,
            "auth": {
                "mode": config.gateway.auth.mode,
            },
        },
        "agents": {
            "count": config.agents.list.len(),
            "ids": config.agents.list.iter().map(|a| &a.id).collect::<Vec<_>>(),
        },
        "embedding": {
            "provider": config.embedding.provider,
            "model": config.embedding.model,
            "dimension": config.embedding.dimension,
        },
    });

    let json = serde_json::to_string_pretty(&redacted)
        .context(SerializationSnafu {})
        .map_err(rmcp::ErrorData::from)?;

    Ok(vec![ResourceContents::text(json, uri)])
}
