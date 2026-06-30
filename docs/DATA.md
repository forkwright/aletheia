# Data sovereignty

What Aletheia stores, where it lives, and how to control it.

## Data inventory

| Data | Location | Format | Description |
|------|----------|--------|-------------|
| Sessions & messages | `instance/data/sessions.db` | fjall LSM-tree | Conversation history, usage stats, agent notes (path name is historical) |
| Knowledge graph | `instance/data/engine/` | Embedded Datalog engine | Entities, relationships, facts, embeddings |
| Workspace files | `instance/nous/{id}/` | Mixed | Per-agent identity, memory, tools, hooks |
| Shared resources | `instance/shared/` | Mixed | Cross-agent tools, skills, coordination |
| Collaborative space | `instance/theke/` | Mixed | Human + agent shared documents |
| Configuration | `instance/config/aletheia.toml` | TOML | Instance settings, agent definitions |
| Credentials | `instance/config/credentials/` | Various | API keys, OAuth tokens |
| Signal data | `instance/signal/` | signal-cli | Phone account, contacts, message state |
| Logs | `instance/logs/` | Text | Runtime logs |
| Backups | `instance/data/backups/instance/` | Local files | Whole-instance backup sets with manifest, knowledge, sessions, config, and workspace data |
| Archives | `instance/data/archive/` | JSON | Retained session exports before deletion |

## Storage locations

All data lives under the instance root directory. Default: `./instance`. Override with `ALETHEIA_ROOT` or the `-r` flag.

```text
instance/
├── config/aletheia.toml     # Main config
├── config/credentials/      # API keys
├── data/
│   ├── sessions.db          # Session store (fjall LSM-tree; .db suffix is historical)
│   ├── engine/              # Knowledge graph (embedded Datalog engine)
│   ├── backups/instance/    # Whole-instance backup sets
│   └── archive/sessions/    # Archived session JSON files
├── nous/{id}/               # Per-agent workspaces
├── shared/                  # Cross-agent shared resources
├── theke/                   # Human + agent collaborative space
├── logs/                    # Runtime logs
└── signal/                  # Signal messenger data
```

## Data flow

```text
Inbound message
  → Channel (Signal, HTTP)
  → Session store (sessions.db (fjall): session + messages)
  → Nous pipeline (LLM call)
  → Response stored (messages partition)
  → Knowledge extraction (entities, relationships, facts)
  → Knowledge graph (embedded Datalog engine)
  → Recall pipeline (vector search + graph traversal)
```

All processing happens locally. The only external call is to the configured LLM provider (Anthropic by default) for inference.

## Retention defaults

Configured in `instance/config/aletheia.toml` under `data.retention`:

```toml
[data.retention]
session_max_age_days = 90           # Delete closed sessions older than 90 days
orphan_message_max_age_days = 30    # Delete orphan messages older than 30 days
max_sessions_per_nous = 0           # 0 = unlimited; nonzero caps sessions per agent
archiveBeforeDelete = true        # Export to JSON before deleting
```

The retention policy only deletes **closed** sessions (archived or distilled). Active sessions are never deleted regardless of age.

When `archiveBeforeDelete` is true, sessions deleted by age or by the cap are
exported to `instance/data/archive/sessions/{session_id}.json` before removal.

### Session cap

`maxSessionsPerNous` (also accepted as the alias `max_sessions_per_nous`) controls the
maximum number of retained sessions per agent. It is enforced only when
`maintenance.retention.enabled` is `true`:

- `0` is unlimited; no sessions are deleted to satisfy the cap.
- A nonzero cap is enforced per `nous_id`. Sessions are ordered newest first by
  `updated_at`, then by `id` ascending for ties. The newest
  `maxSessionsPerNous` records are retained.
- Active sessions are protected and are never deleted by the cap.
- Archived and distilled sessions outside the retained slots are eligible for
  deletion.
- If `archiveBeforeDelete` is `true`, cap deletions are archived to
  `instance/data/archive/sessions/{session_id}.json` before removal, using the
  same archive path as TTL cleanup.
- The retention summary reports cap-based session deletions separately from
  TTL/orphan cleanup.

## Backup and export

### Create a backup

```bash
aletheia backup
```

Creates a local whole-instance backup set at `instance/data/backups/instance/{timestamp}/`.
Each set includes `manifest.json`, all knowledge cohorts under
`stores/knowledge.fjall`, `stores/sessions.db`, and present runtime stores such
as `stores/auth.fjall`, `stores/daemon-task-state`, and
`stores/cron-locks.fjall`. It also includes `config/` and present workspace
directories (`workspace/nous`, `workspace/shared`, `workspace/theke`). Optional
local data such as archives and prompt/prosoche audit logs is copied when
present. The command does not upload data to cloud storage.

Verify a set before relying on it for recovery:

```bash
aletheia backup verify instance/data/backups/instance/<timestamp>
```

Restore a verified set with manifest-driven staging and rollback:

```bash
aletheia backup restore instance/data/backups/instance/<timestamp>
```

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

No built-in JSON export exists for the session store since the SQLite-to-fjall migration (#3446). Archived sessions are already JSON in `instance/data/archive/sessions/`.

## Deletion

### Delete a specific session

Ad-hoc SQL is not available; the session store is a binary LSM-tree. Archive the session via the API:

```bash
curl -sf -X POST http://localhost:18789/api/v1/sessions/SESSION_ID/archive \
  -H "Authorization: Bearer <token>"
```

To delete permanently, stop the service and remove the store directory.

### Delete all data for a specific agent

No single command exists for this. Remove the agent's workspace:

```bash
rm -rf instance/nous/AGENT_ID/
```

### Delete everything

```bash
rm -rf instance/data/sessions.db
rm -rf instance/data/engine/
rm -rf instance/nous/*/memory/
```

The store will be recreated on next startup.

## Third-party data flows

| Destination | Data Sent | When |
|-------------|-----------|------|
| Anthropic API | Conversation messages, system prompts, tool calls | Every LLM inference call |
| Signal servers | Message content (encrypted) | When Signal channel is enabled |

**Everything else stays local.** The knowledge graph, embeddings, session history, agent workspaces, configuration, and credentials never leave your machine.

## Privacy by default

- **No telemetry.** No usage data, analytics, or crash reports.
- **No phone-home.** No update checks, license validation, or beacon requests.
- **All data local.** Unless you configure an external channel (Signal) or LLM provider, nothing leaves the instance directory.
- **You own the data.** Export, back up, or delete it - standard tools (filesystem operations) or the built-in backup command.
