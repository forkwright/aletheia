//! Stdio MCP server command.

use std::path::PathBuf;
use std::sync::Arc;

use snafu::prelude::*;
use taxis::loader::load_config;

use crate::commands::resolve_oikos;
use crate::error::Result;
use crate::runtime::RuntimeBuilder;

/// Run diaporeia over stdio for local MCP clients.
pub(crate) async fn run(instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = Arc::new(resolve_oikos(instance_root)?);
    let config = load_config(&oikos).whatever_context("failed to load config")?;
    let runtime =
        Box::pin(RuntimeBuilder::production(Arc::clone(&oikos), config.clone()).build()).await?;

    let mcp_auth_facade = if runtime.state.auth_mode == "none" {
        None
    } else {
        Some(Arc::clone(&runtime.state.auth_facade))
    };

    let diaporeia_state = Arc::new(diaporeia::state::DiaporeiaState {
        session_store: Arc::clone(&runtime.state.session_store),
        nous_manager: Arc::clone(&runtime.state.nous_manager),
        tool_registry: Arc::clone(&runtime.state.tool_registry),
        oikos: Arc::clone(&runtime.state.oikos),
        auth_facade: mcp_auth_facade,
        start_time: runtime.state.start_time,
        config: Arc::clone(&runtime.state.config),
        auth_mode: runtime.state.auth_mode.clone(),
        none_role: runtime.state.none_role.clone(),
        shutdown: runtime.shutdown_token.clone(),
        #[cfg(feature = "recall")]
        knowledge_store: runtime.state.knowledge_store.clone(),
        note_store: Some(Arc::new(nous::adapters::SessionNoteAdapter(Arc::clone(
            &runtime.state.session_store,
        )))),
        blackboard_store: Some(Arc::new(nous::adapters::SessionBlackboardAdapter(
            Arc::clone(&runtime.state.session_store),
        ))),
    });

    diaporeia::transport::serve_stdio(diaporeia_state)
        .await
        .whatever_context("diaporeia stdio MCP transport failed")
}
