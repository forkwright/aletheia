# agora

## Signal `!` Commands

The dispatcher exposes `!help` and the fixed `!ping` response on public
routes. All other commands require `Operator` authority proven by an exact,
account-scoped `sourceKind = "direct"` binding.

| Command | Authority | Description |
|---------|-----------|-------------|
| `!help` | Public | list commands visible to the selected route |
| `!ping` | Public | return fixed `Pong.` without agent identity |
| all other recognized commands | Operator | inspect agent or runtime state |

Unknown and denied commands receive the same fixed reply. Unknown names and
arguments are never echoed or stored in the command-lifecycle partition.
