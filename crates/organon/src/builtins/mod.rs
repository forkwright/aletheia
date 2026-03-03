//! Built-in tool stubs — prove the registry pattern with real schemas.

pub mod communication;
pub mod memory;
pub mod workspace;

use crate::error::Result;
use crate::registry::ToolRegistry;

/// Register all built-in tool stubs into the given [`ToolRegistry`].
///
/// Registers workspace, memory, and communication tools in a single call.
/// Equivalent to calling `workspace::register`, `memory::register`, and
/// `communication::register` in sequence.
///
/// # Errors
/// Returns `Error::DuplicateTool` if any built-in tool name
/// conflicts with an already-registered tool.
pub fn register_all(registry: &mut ToolRegistry) -> Result<()> {
    workspace::register(registry)?;
    memory::register(registry)?;
    communication::register(registry)?;
    Ok(())
}
