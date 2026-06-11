//! Spawn-class isolation guard (#186).

use hermeneus::types::{ContentBlock, ToolResultContent};
use koina::id::ToolName;
use organon::registry::ToolRegistry;
use tracing::warn;

/// Spawn-class isolation guard: if a tool whose groups contain `SpawnSubtask`
/// appears and is not the last `tool_use`, truncate all subsequent `tool_uses` and
/// inject synthetic error `tool_result` blocks for each truncated call.
///
/// WHY: Spawn-class tools delegate to a sub-agent. Co-occurring tools race
/// with the sub-agent's result, producing undefined hook firing order and
/// edit conflicts. The guard makes the failure mode loud and recoverable.
pub(super) fn enforce_spawn_isolation(
    tool_uses: &mut Vec<(String, String, serde_json::Value)>,
    denied_blocks: &mut Vec<ContentBlock>,
    tools: &ToolRegistry,
) {
    let spawn_index = tool_uses.iter().position(|(_, name, _)| {
        ToolName::new(name)
            .ok()
            .and_then(|n| tools.get_def(&n))
            .is_some_and(|def| {
                def.groups
                    .contains(&organon::types::ToolGroupId::SpawnSubtask)
            })
    });

    if let Some(idx) = spawn_index
        && idx < tool_uses.len().saturating_sub(1)
    {
        let truncated = tool_uses.split_off(idx + 1);
        for (id, name, _) in truncated {
            warn!(
                tool = %name,
                tool_use_id = %id,
                "tool call truncated: spawn-class tool calls cannot co-occur with other tool calls in the same turn"
            );
            denied_blocks.push(ContentBlock::ToolResult {
                tool_use_id: id,
                content: ToolResultContent::Text(
                    "Tool call dropped: spawn-class tool calls cannot co-occur with other tool calls in the same turn.".to_owned(),
                ),
                is_error: Some(true),
            });
        }
    }
}
