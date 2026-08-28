#![expect(clippy::expect_used, reason = "test assertions")]

//! Split from `filesystem_tests.rs` (839 lines) to satisfy `RUST/file-too-long`.

use super::*;

mod behavior;
mod helpers;

fn test_ctx(dir: &Path) -> ToolContext {
    crate::testing::make_test_context_at(dir)
}

fn tool_input(name: &str, args: serde_json::Value) -> ToolInput {
    crate::testing::make_tool_input_with_args(name, args)
}

fn test_sandbox() -> crate::sandbox::SandboxConfig {
    crate::sandbox::SandboxConfig {
        enabled: false,
        nproc_limit: 4096,
        ..crate::sandbox::SandboxConfig::default()
    }
}

fn grep_executor() -> GrepExecutor {
    GrepExecutor::new(crate::subprocess::SubprocessRunner::new(test_sandbox()))
}

fn find_executor() -> FindExecutor {
    FindExecutor::new(crate::subprocess::SubprocessRunner::new(test_sandbox()))
}
