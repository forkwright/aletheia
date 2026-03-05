//! Built-in tool stubs — prove the registry pattern with real schemas.

/// Inter-agent communication tools (send_message, broadcast).
pub mod communication;
/// Knowledge graph and session memory tools (remember, recall).
pub mod memory;
/// File and shell workspace tools (read_file, write_file, run_command).
pub mod workspace;

use crate::error::Result;
use crate::registry::ToolRegistry;

/// Register all built-in tool stubs.
pub fn register_all(registry: &mut ToolRegistry) -> Result<()> {
    workspace::register(registry)?;
    memory::register(registry)?;
    communication::register(registry)?;
    Ok(())
}
