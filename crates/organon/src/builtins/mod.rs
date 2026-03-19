//! Built-in tool executors and stubs.

/// Agent coordination tools (spawn, dispatch).
pub mod agent;
/// Inter-agent communication tools (send_message, broadcast).
pub mod communication;
/// Computer use: screen capture, action dispatch, sandboxed execution.
#[cfg(feature = "computer-use")]
pub mod computer_use;
/// Dynamic tool activation meta-tool.
pub mod enable_tool;
/// Filesystem navigation tools (grep, find, ls).
pub mod filesystem;
/// Knowledge graph and session memory tools (remember, recall).
pub mod memory;
/// Planning project management tools (create, status, execute, verify).
pub mod planning;
/// Web research tools (web_fetch). Web search uses Anthropic server-side tools.
pub mod research;
/// File viewing with multimodal support (images, PDFs, text).
pub mod view_file;
/// File and shell workspace tools (read, write, edit, exec).
pub mod workspace;

use crate::error::Result;
use crate::registry::ToolRegistry;
use crate::sandbox::SandboxConfig;

/// Register all built-in tool executors with default sandbox config.
///
/// # Errors
///
/// Returns an error if any built-in tool name collides with an
/// already-registered tool.
pub fn register_all(registry: &mut ToolRegistry) -> Result<()> {
    register_all_with_sandbox(registry, SandboxConfig::default())
}

/// Register all built-in tool executors with custom sandbox config.
pub fn register_all_with_sandbox(
    registry: &mut ToolRegistry,
    sandbox: SandboxConfig,
) -> Result<()> {
    #[cfg(feature = "computer-use")]
    computer_use::register(registry, &sandbox)?;

    workspace::register(registry, sandbox)?;
    memory::register(registry)?;
    communication::register(registry)?;
    filesystem::register(registry)?;
    view_file::register(registry)?;
    agent::register(registry)?;
    enable_tool::register(registry)?;
    planning::register(registry)?;
    research::register(registry)?;
    Ok(())
}
