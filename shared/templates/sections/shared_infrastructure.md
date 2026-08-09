
## Shared infrastructure

All nous share common resources at `$ALETHEIA_SHARED`:

### Environment
Runtime environment file: `$ALETHEIA_ROOT/config/env`

Convention-based paths (no mapping files needed):
- Agent workspace: `$ALETHEIA_NOUS/$AGENT_ID`
- Vault domain: `$ALETHEIA_THEKE/$DOMAIN`
- Shared config: `$ALETHEIA_SHARED/config/$NAME`
- Shared tools: `$ALETHEIA_SHARED/bin/$NAME`

### Shared memory
- `$ALETHEIA_SHARED/memory/facts.jsonl` - Single fact store (symlinked to all nous)
- `$ALETHEIA_SHARED/USER.md` - Human context (symlinked to all nous)

### Coordination
- **Blackboard:** the `blackboard` tool - actions `write`/`read`/`list`/`delete` (JSON args: `key`, `value`, `ttl_seconds`) - Quick coordination
- **Formal hand-offs:** no standalone task-contract tool exists - use `sessions_send`/`sessions_ask` directly, or a `HANDOFF:`-tagged entry in your daily memory file (see Team topology)
- **Agent health:** `aletheia health` - Ecosystem monitoring
