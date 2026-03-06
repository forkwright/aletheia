//! Built-in tool executors and stubs.

/// Inter-agent communication tools (send_message, broadcast).
pub mod communication;
/// Dynamic tool activation meta-tool.
pub mod enable_tool;
/// Filesystem navigation tools (grep, find, ls).
pub mod filesystem;
/// Knowledge graph and session memory tools (remember, recall).
pub mod memory;
/// Web research tools (web_search, web_fetch).
pub mod research;
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
    enable_tool::register(registry)?;
    research::register(registry)?;
    Ok(())
}
