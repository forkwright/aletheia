//! Built-in tool stubs — prove the registry pattern with real schemas.

pub mod communication;
pub mod memory;
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
