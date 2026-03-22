//! Memory tool executors: `memory_search`, `note`, `blackboard`, `datalog_query`.

mod blackboard;
mod datalog;
mod knowledge_ops;
mod note;
#[cfg(test)]
mod tests;

use crate::error::Result;
use crate::registry::ToolRegistry;
use crate::types::{ToolContext, ToolResult};

pub(super) fn require_services(
    ctx: &ToolContext,
) -> std::result::Result<&crate::types::ToolServices, ToolResult> {
    ctx.services
        .as_deref()
        .ok_or_else(|| ToolResult::error("memory services not configured"))
}

/// Register all memory tools (knowledge ops, notes, blackboard, datalog) into the registry.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    knowledge_ops::register(registry)?;
    note::register(registry)?;
    blackboard::register(registry)?;
    datalog::register(registry)?;
    Ok(())
}
