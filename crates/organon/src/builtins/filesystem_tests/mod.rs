#![expect(clippy::expect_used, reason = "test assertions")]

//! Split from `filesystem_tests.rs` (839 lines) to satisfy `RUST/file-too-long`.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use koina::id::{NousId, SessionId, ToolName};

use super::*;

mod behavior;
mod helpers;

fn test_ctx(dir: &Path) -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: dir.to_path_buf(),
        allowed_roots: vec![dir.to_path_buf()],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
}

fn tool_input(name: &str, args: serde_json::Value) -> ToolInput {
    ToolInput {
        name: ToolName::new(name).expect("valid"),
        tool_use_id: "toolu_test".to_owned(),
        arguments: args,
    }
}
