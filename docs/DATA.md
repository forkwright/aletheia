# Data Sovereignty

What Aletheia stores, where it lives, and how to control it.

## Data Inventory

| Data | Location | Format | Description |
|------|----------|--------|-------------|
| Sessions & messages | `instance/data/sessions.db` | SQLite (WAL) | Conversation history, usage stats, agent notes |
| Knowledge graph | `instance/data/engine/` | Embedded Datalog engine | Entities, relationships, facts, embeddings |
| Workspace files | `instance/nous/{id}/` | Mixed | Per-agent identity, memory, tools, hooks |
| Shared resources | `instance/shared/` | Mixed | Cross-agent tools, skills, coordination |
| Collaborative space | `instance/theke/` | Mixed | Human + agent shared documents |
| Configuration | `instance/config/aletheia.toml` | TOML | Instance settings, agent definitions |
| Credentials | `instance/config/credentials/` | Various | API keys, OAuth tokens |
| Signal data | `instance/signal/` | signal-cli | Phone account, contacts, message state |
| Logs | `instance/logs/` | Text | Runtime logs |
| Backups | `instance/data/backups/` | SQLite | Point-in-time session database copies |
| Archives | `instance/data/archive/` | JSON | Retained session exports before deletion |

## Storage Locations

All data lives under the instance root directory. Default: `./instance`. Override with `ALETHEIA_ROOT` or the `-r` flag.

```text
instance/
├── config/aletheia.toml     # Main config
├── config/credentials/      # API keys
├── data/
│   ├── sessions.db          # Session store (SQLite, WAL mode)
│   ├── engine/              # Knowledge graph (embedded Datalog engine)
│   ├── backups/             # Database backups
│   └── archive/sessions/    # Archived session JSON files
├── nous/{id}/               # Per-agent workspaces
├── shared/                  # Cross-agent shared resources
├── theke/                   # Human + agent collaborative space
├── logs/                    # Runtime logs
└── signal/                  # Signal messenger data
```

## Data Flow

```text
Inbound message
  → Channel (Signal, HTTP)
  → Session store (sessions.db: session + messages)
  → Nous pipeline (LLM call)
  → Response stored (messages table)
  → Knowledge extraction (entities, relationships, facts)
  → Knowledge graph (embedded Datalog engine)
  → Recall pipeline (vector search + graph traversal)
```

All processing happens locally. The only external call is to the configured LLM provider (Anthropic by default) for inference.

## Retention Defaults

Configured in `instance/config/aletheia.toml` under `data.retention`:

```yaml
data:
  retention:
    sessionMaxAgeDays: 90           # Delete closed sessions older than 90 days
    orphanMessageMaxAgeDays: 30     # Delete orphan messages older than 30 days
    maxSessionsPerNous: 0           # 0 = unlimited sessions per agent
    archiveBeforeDelete: true       # Export to JSON before deleting
```

The retention policy only deletes **closed** sessions (archived or distilled). Active sessions are never deleted regardless of age.

When `archiveBeforeDelete` is true, each session is exported to `instance/data/archive/sessions/{session_id}.json` before removal.

## Backup and Export

### Create a backup

```bash
aletheia backup
```

Creates a point-in-time SQLite backup at `instance/data/backups/sessions_{timestamp}.db` using `VACUUM INTO` (no locking, safe while running).

### List backups

```bash
aletheia backup --list
```

### Prune old backups

```bash
aletheia backup --prune --keep 5
```

Keeps the 5 most recent backups, deletes the rest.

### Export as JSON

```bash
aletheia backup --export-json
```

Exports every session as an individual JSON file to `instance/data/archive/sessions/`.
Each file contains session metadata, all messages, and export timestamp.

## Deletion

### Delete a specific session

```sql
-- Connect to instance/data/sessions.db
DELETE FROM agent_notes WHERE session_id = 'SESSION_ID';
DELETE FROM distillations WHERE session_id = 'SESSION_ID';
DELETE FROM usage WHERE session_id = 'SESSION_ID';
DELETE FROM messages WHERE session_id = 'SESSION_ID';
DELETE FROM sessions WHERE id = 'SESSION_ID';
```

### Delete all data for a specific agent

```sql
-- Get all session IDs for the agent
DELETE FROM agent_notes WHERE session_id IN (SELECT id FROM sessions WHERE nous_id = 'AGENT_ID');
DELETE FROM distillations WHERE session_id IN (SELECT id FROM sessions WHERE nous_id = 'AGENT_ID');
DELETE FROM usage WHERE session_id IN (SELECT id FROM sessions WHERE nous_id = 'AGENT_ID');
DELETE FROM messages WHERE session_id IN (SELECT id FROM sessions WHERE nous_id = 'AGENT_ID');
DELETE FROM sessions WHERE nous_id = 'AGENT_ID';
```

Also remove the agent's workspace: `rm -rf instance/nous/AGENT_ID/`

### Delete everything

```bash
rm instance/data/sessions.db
rm -rf instance/data/engine/
rm -rf instance/nous/*/memory/
```

The database will be recreated on next startup with the migration framework.

## Third-Party Data Flows

| Destination | Data Sent | When |
|-------------|-----------|------|
| Anthropic API | Conversation messages, system prompts, tool calls | Every LLM inference call |
| Signal servers | Message content (encrypted) | When Signal channel is enabled |

**Everything else stays local.** The knowledge graph, embeddings, session history, agent workspaces, configuration, and credentials never leave your machine.

## Privacy by Default

- **No telemetry.** No usage data, analytics, or crash reports.
- **No phone-home.** No update checks, license validation, or beacon requests.
- **All data local.** Unless you configure an external channel (Signal) or LLM provider, nothing leaves the instance directory.
- **You own the data.** Export, back up, or delete it - standard tools (SQLite CLI, filesystem operations) or the built-in backup command.
