//! Built-in tool executors and stubs.

/// Agent coordination tools (spawn, dispatch).
pub mod agent;
/// Bookkeeper tools (prompt archival and worktree cleanup).
#[cfg(feature = "energeia")]
pub mod bookkeeper;
/// Inter-agent communication tools (send_message, broadcast).
pub mod communication;
/// Computer use: screen capture, action dispatch, sandboxed execution.
#[cfg(feature = "computer-use")]
pub mod computer_use;
/// Dynamic tool activation meta-tool.
pub mod enable_tool;
/// Energeia capability tools (dromeus, dokimasia, diorthosis, epitropos, parateresis,
/// mathesis, prographe, schedion, metron). Wired to real energeia subsystems.
#[cfg(feature = "energeia")]
pub mod energeia;
/// Filesystem navigation tools (grep, find, ls).
pub mod filesystem;
/// Knowledge graph and session memory tools (remember, recall).
pub mod memory;
/// Planning project management tools (create, status, execute, verify).
pub mod planning;
/// Web research tools (web_fetch). Web search uses Anthropic server-side tools.
pub mod research;
/// Issue triage tools (scan, score, stage, approve).
pub mod triage;
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
#[must_use]
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
    triage::register(registry)?;
    #[cfg(feature = "energeia")]
    // WHY: No EnergeiaServices provided at this registration level — callers that
    // have services configured should use energeia::register(registry, Some(services)).
    // Tools requiring services return structured errors rather than panicking.
    energeia::register(registry, None)?;
    #[cfg(feature = "energeia")]
    bookkeeper::register(registry)?;
    Ok(())
}
