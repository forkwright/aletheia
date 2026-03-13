# Prosoche: attention system

**prosoche** (Greek: προσοχή) means "directed attention." In Stoic practice, prosoche is the discipline of sustained awareness; you cannot reveal what is hidden (aletheia) without first attending carefully.

In Aletheia, prosoche is the **heartbeat subsystem**: a periodic background check that prompts each agent to survey its environment and report anything that needs attention.

---

## Architecture

Prosoche lives inside the **oikonomos** daemon crate (`crates/daemon/`). It is an in-process background task that runs alongside the HTTP gateway.

```text
aletheia binary
├── pylon (HTTP gateway)
├── nous (agent actors)
└── oikonomos (daemon)
    ├── TaskRunner — cron/interval scheduler
    ├── ProsocheCheck — heartbeat check definition
    └── DaemonBridge — sends prompts to nous actors
```

### Data flow

1. `TaskRunner` fires the prosoche task on schedule
2. Runner calls `DaemonBridge::send_prompt()` with session key `"daemon:prosoche"`
3. The bridge routes the prompt to the target nous actor
4. The agent reads its `PROSOCHE.md` workspace file and executes the checklist
5. Results surface as a background session (detected by `session_key.contains("prosoche")`)

---

## Heartbeat intervals

The daemon supports multiple schedule types:

| Type | Example | Use Case |
|------|---------|----------|
| `Cron` | `0 */45 8-23 * * *` | Every 45 min during waking hours |
| `Interval` | `Duration::from_secs(3600)` | Fixed hourly interval |
| `Once` | ISO 8601 timestamp | One-shot scheduled task |
| `Startup` | n/a | Run once when daemon starts |

### Default maintenance schedules

| Task | Schedule |
|------|----------|
| Trace rotation | `0 0 3 * * *` (3 AM daily) |
| Drift detection | `0 0 4 * * *` (4 AM daily) |
| DB size monitor | Every 6 hours |
| Retention | `0 30 3 * * *` (3:30 AM daily) |

Tasks support optional **active windows** `(start_hour, end_hour)` to restrict execution to specific hours.

---

## Configuration

### Agent workspace file

Each agent has an `instance/nous/<agent-id>/PROSOCHE.md` file that defines its heartbeat checklist. A template is provided at `instance.example/nous/_template/PROSOCHE.md`.

The checklist specifies what the agent should check on each heartbeat tick:

1. **Calendar**: upcoming events in the next 4 hours
2. **Tasks**: overdue or due-today items
3. **System health**: agent status checks

**Constraints per tick:**
- Maximum 5 tool calls
- No investigation or research; just check and report
- Response: `HEARTBEAT_OK` if nothing needs action, or brief one-line alerts

### Attention types

```rust
pub enum AttentionCategory {
    Calendar,
    Task,
    SystemHealth,
    Custom(String),
}

pub enum Urgency {
    Low,       // Informational
    Medium,    // Address within current session
    High,      // Within hours
    Critical,  // Immediate action
}
```

---

## Checks

`ProsocheCheck` produces a `ProsocheResult` containing zero or more `AttentionItem` entries. Each item has a category, summary, and urgency level.

Currently, prosoche dispatches a prompt to the agent and relies on the agent's tool access (calendar, task manager, system health) to perform the actual checks. If no bridge is configured, prosoche completes successfully with empty results and logs a warning.

### Failure handling

- Tasks are disabled after **3 consecutive failures**
- A successful execution resets the failure counter
- Task status can be queried via `aletheia maintenance status`

---

## Extensibility

### Adding a new check category

1. Add a variant to `AttentionCategory` in `crates/daemon/src/prosoche.rs`
2. Update the agent's `PROSOCHE.md` template with instructions for the new check
3. Ensure the agent has access to the relevant tools (register in `organon`)

### Custom scheduled tasks

The `TaskRunner` accepts arbitrary tasks via `TaskAction`:

| Action | Description |
|--------|-------------|
| `Command(String)` | Shell command |
| `Tool { name, args }` | Tool invocation |
| `Prompt(String)` | Prompt sent to a nous agent |
| `Builtin(BuiltinTask)` | Built-in maintenance task |

### Bridge pattern

The daemon communicates with nous actors through the `DaemonBridge` trait. This keeps the daemon crate decoupled from nous internals; the bridge is wired in the binary crate (`crates/aletheia/src/daemon_bridge.rs`).

```rust
pub trait DaemonBridge: Send + Sync {
    fn send_prompt(
        &self,
        nous_id: &str,
        session_key: &str,
        prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<ExecutionResult>> + Send + '_>>;
}
```

---

## Operational notes

```bash
# Check maintenance task status (includes prosoche)
aletheia maintenance status

# View prosoche activity in logs
journalctl --user -u aletheia --since "1 hour ago" | grep prosoche
```

Prosoche workspace files have **priority 7** in the token budget; they are dropped before semi-static files (MEMORY, TOOLS) under budget pressure, but SOUL.md is never dropped.
