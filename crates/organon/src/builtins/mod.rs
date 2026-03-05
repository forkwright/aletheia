//! Built-in tool executors and stubs.

/// Inter-agent communication tools (send_message, broadcast).
pub mod communication;
/// Filesystem navigation tools (grep, find, ls).
pub mod filesystem;
/// Knowledge graph and session memory tools (remember, recall).
pub mod memory;
/// File viewing with multimodal support (images, PDFs, text).
pub mod view_file;
/// File and shell workspace tools (read, write, edit, exec).
pub mod workspace;

use crate::error::Result;
use crate::registry::ToolRegistry;

/// Register all built-in tool executors.
pub fn register_all(registry: &mut ToolRegistry) -> Result<()> {
    workspace::register(registry)?;
    memory::register(registry)?;
    communication::register(registry)?;
    filesystem::register(registry)?;
    view_file::register(registry)?;
    Ok(())
}
