//! Configuration resources.

use rmcp::model::{
    RawResourceTemplate, ReadResourceRequestParams, ResourceContents, ResourceTemplate,
};
use snafu::ResultExt as _;

use koina::http::CONTENT_TYPE_JSON;

use crate::error::SerializationSnafu;
use crate::state::DiaporeiaState;

/// Build resource templates for config resources.
pub(crate) fn resource_templates() -> Vec<ResourceTemplate> {
    let raw = RawResourceTemplate::new("aletheia://config", "Aletheia Configuration")
        .with_description("Runtime configuration (sensitive fields redacted)")
        .with_mime_type(CONTENT_TYPE_JSON);
    vec![ResourceTemplate {
        raw,
        annotations: None,
    }]
}

/// Read a configuration resource.
#[tracing::instrument(skip_all)]
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
    let redacted = taxis::redact::redact(&config);

    let json = serde_json::to_string_pretty(&redacted)
        .context(SerializationSnafu {})
        .map_err(rmcp::ErrorData::from)?;

    Ok(vec![ResourceContents::text(json, uri)])
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test assertions over fixture data")]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use hermeneus::provider::ProviderRegistry;
    use organon::registry::ToolRegistry;
    use tokio_util::sync::CancellationToken;

    use super::*;

    /// WHY: `read_resource` only touches `state.config`, but `DiaporeiaState`
    /// has no partial-construction path -- every field must be initialized.
    /// This mirrors `tools::mod::tests::make_server_state` (the only other
    /// place in the crate builds one); duplicated rather than shared because
    /// that helper is private to a sibling test module.
    fn make_test_state(config: taxis::config::AletheiaConfig) -> Arc<DiaporeiaState> {
        let dir = tempfile::tempdir().expect("tempdir");
        let oikos = Arc::new(taxis::oikos::Oikos::from_root(dir.path()));
        let store = Arc::new(tokio::sync::Mutex::new(
            mneme::store::SessionStore::open(&oikos.sessions_db()).expect("open sessions store"),
        ));
        Arc::new(DiaporeiaState {
            session_store: store.clone(),
            nous_manager: Arc::new(nous::manager::NousManager::new(
                Arc::new(ProviderRegistry::new()),
                Arc::new(ToolRegistry::new()),
                oikos.clone(),
                None,
                None,
                Some(store),
                #[cfg(feature = "knowledge-store")]
                None,
                Arc::new(Vec::new()),
                None,
                None,
                taxis::config::NousBehaviorConfig::default(),
                taxis::config::ToolLimitsConfig::default(),
            )),
            tool_registry: Arc::new(ToolRegistry::new()),
            oikos,
            auth_facade: None,
            start_time: Instant::now(),
            config: Arc::new(tokio::sync::RwLock::new(config)),
            auth_mode: "none".to_owned(),
            none_role: "admin".to_owned(),
            shutdown: CancellationToken::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            note_store: None,
            blackboard_store: None,
        })
    }

    /// WHY(#4571): the resource used to hand-build a 3-section whitelist
    /// (gateway/agents/embedding) instead of calling the same
    /// `taxis::redact::redact` pylon's config surfaces already use for the
    /// identical `AletheiaConfig` type. This is the parity test the issue's
    /// acceptance criteria calls for: the resource's actual wire output must
    /// equal `redact`'s output exactly, not a hand-maintained subset of it.
    #[tokio::test]
    async fn config_resource_output_matches_taxis_redact_exactly() {
        let mut config = taxis::config::AletheiaConfig::default();
        config.gateway.port = 4127;
        let state = make_test_state(config.clone());

        let params = ReadResourceRequestParams::new("aletheia://config");
        let contents = read_resource(&state, &params)
            .await
            .expect("read_resource ok");

        assert_eq!(contents.len(), 1);
        let ResourceContents::TextResourceContents { text, .. } = &contents[0] else {
            panic!("expected text resource contents");
        };
        let actual: serde_json::Value = serde_json::from_str(text).expect("valid json");
        let expected = taxis::redact::redact(&config);
        assert_eq!(
            actual, expected,
            "resource output must equal taxis::redact::redact(&config) exactly"
        );
    }

    /// WHY(#4571): the old hand-built whitelist covered only gateway/agents/
    /// embedding. The fix widens exposure to the full config tree (minus
    /// redact's own sensitive-leaf redaction) -- confirmed here via fields
    /// that were never in that whitelist and so prove this is not still a
    /// narrower hand-picked subset wearing the new call as a facade.
    #[tokio::test]
    async fn config_resource_output_is_widened_beyond_old_three_section_whitelist() {
        let config = taxis::config::AletheiaConfig::default();
        let state = make_test_state(config);

        let params = ReadResourceRequestParams::new("aletheia://config");
        let contents = read_resource(&state, &params)
            .await
            .expect("read_resource ok");
        let ResourceContents::TextResourceContents { text, .. } = &contents[0] else {
            panic!("expected text resource contents");
        };
        let actual: serde_json::Value = serde_json::from_str(text).expect("valid json");

        for field in ["workspace", "channels", "maintenance", "pricing"] {
            assert!(
                actual.get(field).is_some(),
                "widened redaction must include {field:?}, absent from the old whitelist; got: {actual}"
            );
        }
    }

    #[tokio::test]
    async fn unknown_config_uri_is_invalid_params() {
        let state = make_test_state(taxis::config::AletheiaConfig::default());
        let params = ReadResourceRequestParams::new("aletheia://config/nonexistent");
        let err = read_resource(&state, &params)
            .await
            .expect_err("unknown uri must error");
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }
}
