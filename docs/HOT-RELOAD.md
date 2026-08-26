# Hot reload classification

Every `AletheiaConfig` field is classified as either **Hot** (safe to apply via SIGHUP without restart) or **Cold** (requires process restart to take effect).

## Summary

Counts below are field rows in the "Detailed classification" tables further
down this document (the tables are the source; count them again after
editing a table so this summary cannot drift the way it previously did).
"Reserved" fields are declared but not wired into runtime startup at all —
neither hot nor cold.

| Category | Hot Fields | Cold Fields | Reserved |
|----------|-----------|-------------|----------|
| Gateway | 0 | 26 | 0 |
| Agents | 36 | 0 | 0 |
| Channels | 0 | 6 | 0 |
| Bindings | 0 | 7 | 0 |
| Messaging authority | 0 | 5 | 0 |
| Embedding | 3 | 0 | 0 |
| Data | 1 | 0 | 0 |
| Maintenance | 24 | 1 | 4 |
| Pricing | 2 | 0 | 0 |
| Sandbox | 0 | 9 | 0 |
| Tools | 0 | 2 | 0 |
| Credential | 6 | 0 | 0 |
| Logging | 7 | 0 | 0 |
| MCP | 3 | 0 | 0 |
| Local Provider | 4 | 0 | 0 |
| Packs | 0 | 1 | 0 |
| **Total** | **86** | **57** | **4** |

---

## Detailed classification

### Gateway (`gateway`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `gateway.port` | **Cold** | TCP listener already bound; changing requires re-binding the socket |
| `gateway.bind` | **Cold** | Interface binding decision made at startup; affects socket creation |
| `gateway.auth.mode` | **Cold** | Authentication mode changes affect middleware stack initialization |
| `gateway.auth.noneRole` | **Cold** | Registry-declared restart prefix (#5324): stored in `AppState.none_role` at startup, not refreshed on reload |
| `gateway.auth.signingKey` | **Cold** | Registry-declared restart prefix (#5324): used to build `JwtManager` at startup, key rotation requires restart |
| `gateway.tls.enabled` | **Cold** | TLS termination settings require listener reconfiguration |
| `gateway.tls.certPath` | **Cold** | Certificate paths loaded at startup for TLS context |
| `gateway.tls.keyPath` | **Cold** | Private key paths loaded at startup for TLS context |
| `gateway.cors.allowedOrigins` | **Cold** | Whole `gateway.cors` subtree is a registry-declared restart prefix (#5173): CORS is a security-relevant control, not evaluated fresh per-request |
| `gateway.cors.maxAgeSecs` | **Cold** | Same `gateway.cors` restart prefix |
| `gateway.bodyLimit.maxBytes` | **Cold** | Body limit configured in Axum router at startup |
| `gateway.csrf.enabled` | **Cold** | CSRF middleware layer initialized at startup |
| `gateway.csrf.disableAcknowledged` | **Cold** | Acknowledgement for running without CSRF protection |
| `gateway.csrf.headerName` | **Cold** | Header name captured by CSRF middleware at startup |
| `gateway.csrf.headerValue` | **Cold** | Expected value captured by CSRF middleware at startup |
| `gateway.rateLimit.enabled` | **Cold** | Whole `gateway.rateLimit` subtree is a registry-declared restart prefix (#5173): rate limiting is security-relevant and must not silently take effect on SIGHUP |
| `gateway.rateLimit.requestsPerMinute` | **Cold** | Same `gateway.rateLimit` restart prefix |
| `gateway.rateLimit.trustProxy` | **Cold** | Rate limiter is built with this flag at startup |
| `gateway.rateLimit.perUser.enabled` | **Cold** | Same `gateway.rateLimit` restart prefix |
| `gateway.rateLimit.perUser.defaultRpm` | **Cold** | Same `gateway.rateLimit` restart prefix |
| `gateway.rateLimit.perUser.defaultBurst` | **Cold** | Same `gateway.rateLimit` restart prefix |
| `gateway.rateLimit.perUser.llmRpm` | **Cold** | Same `gateway.rateLimit` restart prefix |
| `gateway.rateLimit.perUser.llmBurst` | **Cold** | Same `gateway.rateLimit` restart prefix |
| `gateway.rateLimit.perUser.toolRpm` | **Cold** | Same `gateway.rateLimit` restart prefix |
| `gateway.rateLimit.perUser.toolBurst` | **Cold** | Same `gateway.rateLimit` restart prefix |
| `gateway.rateLimit.perUser.staleAfterSecs` | **Cold** | Same `gateway.rateLimit` restart prefix |

### Agents (`agents`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `agents.defaults.model` | Hot | Model spec read per-request from config snapshot |
| `agents.defaults.model.primary` | Hot | Primary model identifier read at request time |
| `agents.defaults.model.fallbacks` | Hot | Fallback chain read at request time |
| `agents.defaults.model.retriesBeforeFallback` | Hot | Retry count read per-request |
| `agents.defaults.contextTokens` | Hot | Context window size read per-request |
| `agents.defaults.maxOutputTokens` | Hot | Output limit read per-request |
| `agents.defaults.bootstrapMaxTokens` | Hot | Bootstrap budget read per-request |
| `agents.defaults.thinkingEnabled` | Hot | Thinking toggle read per-request |
| `agents.defaults.thinkingBudget` | Hot | Thinking budget read per-request |
| `agents.defaults.charsPerToken` | Hot | Estimation ratio read per-request |
| `agents.defaults.prosocheModel` | Hot | Prosoche model read per-request |
| `agents.defaults.maxToolResultBytes` | Hot | Tool result limit read per-request |
| `agents.defaults.agency` | Hot | Agency level read per-request |
| `agents.defaults.maxToolIterations` | Hot | Iteration limit read per-request |
| `agents.defaults.allowedRoots` | Hot | Filesystem roots read per-request |
| `agents.defaults.caching.enabled` | Hot | Caching toggle read per-request |
| `agents.defaults.caching.strategy` | Hot | Caching strategy read per-request |
| `agents.defaults.recall.enabled` | Hot | Recall toggle read per-request |
| `agents.defaults.recall.maxResults` | Hot | Recall limit read per-request |
| `agents.defaults.recall.minScore` | Hot | Relevance threshold read per-request |
| `agents.defaults.recall.maxRecallTokens` | Hot | Token budget read per-request |
| `agents.defaults.recall.iterative` | Hot | Iterative mode toggle read per-request |
| `agents.defaults.recall.maxCycles` | Hot | Max cycles read per-request |
| `agents.defaults.recall.weights.*` | Hot | Recall weights read per-request |
| `agents.defaults.historyBudgetRatio` | Hot | Budget ratio read per-request |
| `agents.list` | Hot | Agent definitions resolved per-request |
| `agents.list[].id` | Hot | Agent ID used for resolution |
| `agents.list[].name` | Hot | Display name read per-request |
| `agents.list[].model` | Hot | Model override read per-request |
| `agents.list[].workspace` | Hot | Workspace path read per-request |
| `agents.list[].thinkingEnabled` | Hot | Thinking override read per-request |
| `agents.list[].agency` | Hot | Agency override read per-request |
| `agents.list[].allowedRoots` | Hot | Additional roots merged per-request |
| `agents.list[].domains` | Hot | Domain tags read per-request |
| `agents.list[].default` | Hot | Default flag read per-request |
| `agents.list[].recall` | Hot | Recall override read per-request |

### Channels (`channels`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `channels.signal.enabled` | **Cold** | Signal channel lifecycle managed at startup |
| `channels.signal.accounts` | **Cold** | Account connections established at startup |
| `channels.signal.accounts[].enabled` | **Cold** | Account state managed at startup |
| `channels.signal.accounts[].httpHost` | **Cold** | signal-cli connection established at startup |
| `channels.signal.accounts[].httpPort` | **Cold** | Port binding requires restart |
| `channels.signal.accounts[].autoStart` | **Cold** | Receive loop lifecycle managed at startup |

### Bindings (`bindings`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `bindings` | **Cold** | `MessageRouter` captures one complete binding snapshot at startup |
| `bindings[].channel` | **Cold** | Channel match authority is part of the startup router snapshot |
| `bindings[].source` | **Cold** | Source match authority is part of the startup router snapshot |
| `bindings[].nousId` | **Cold** | Target-agent authority is part of the startup router snapshot |
| `bindings[].sessionKey` | **Cold** | Session routing is part of the startup router snapshot |
| `bindings[].account` | **Cold** | Receiving-account scope is part of the startup router snapshot |
| `bindings[].participants` | **Cold** | Sender allowlist authority is part of the startup router snapshot |

### Messaging authority (`messaging.outbound`, `messaging.commands`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `messaging.outbound.allowlist` | **Cold** | Channel services clone outbound recipient authority at startup |
| `messaging.outbound.defaultDeny` | **Cold** | Channel services clone the default-deny posture at startup |
| `messaging.commands.operators` | **Cold** | The command dispatcher clones operator authority at startup |
| `messaging.commands.publicCommands` | **Cold** | The command dispatcher clones the public-command set at startup |
| `messaging.commands.defaultAllow` | **Cold** | The command dispatcher clones the default-allow posture at startup |

### Embedding (`embedding`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `embedding.provider` | Hot | Provider selection read per-request |
| `embedding.model` | Hot | Model name read per-request |
| `embedding.dimension` | Hot | Vector dimension read per-request (must match index) |

### Data (`data`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `data.retention` | Hot | Retention policies applied during scheduled tasks |

### Maintenance (`maintenance`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `maintenance.traceRotation.enabled` | Hot | Rotation toggle checked by background task |
| `maintenance.traceRotation.maxAgeDays` | Hot | Age limit read by background task |
| `maintenance.traceRotation.maxTotalSizeMb` | Hot | Size limit read by background task |
| `maintenance.traceRotation.compress` | Hot | Compression flag read by background task |
| `maintenance.traceRotation.maxArchives` | Hot | Archive limit read by background task |
| `maintenance.driftDetection.enabled` | Hot | Detection toggle checked by background task |
| `maintenance.driftDetection.alertOnMissing` | Hot | Alert flag read by background task |
| `maintenance.driftDetection.ignorePatterns` | Hot | Ignore patterns read by background task |
| `maintenance.driftDetection.optionalPatterns` | Hot | Optional patterns read by background task |
| `maintenance.dbMonitoring.enabled` | Hot | Monitoring toggle checked by background task |
| `maintenance.dbMonitoring.warnThresholdMb` | Hot | Threshold read by background task |
| `maintenance.dbMonitoring.alertThresholdMb` | Hot | Threshold read by background task |
| `maintenance.diskSpace.enabled` | Hot | Monitoring toggle checked by background task |
| `maintenance.diskSpace.warningThresholdMb` | Hot | Threshold read by background task |
| `maintenance.diskSpace.criticalThresholdMb` | Hot | Threshold read by background task |
| `maintenance.diskSpace.checkIntervalSecs` | Hot | Interval read by background task |
| `maintenance.retention.enabled` | Hot | Retention toggle checked by background task |
| `maintenance.knowledgeMaintenanceEnabled` | Hot | Maintenance toggle checked by background task |
| `maintenance.watchdog.enabled` | Reserved | Per-process watchdog monitor is not wired into runtime startup |
| `maintenance.watchdog.heartbeatTimeoutSecs` | Reserved | Reserved for future per-process watchdog integration |
| `maintenance.watchdog.checkIntervalSecs` | Reserved | Reserved for future per-process watchdog integration |
| `maintenance.watchdog.maxRestarts` | Reserved | Reserved for future per-process watchdog integration |
| `maintenance.cronTasks.evolution.enabled` | Hot | Task toggle checked by scheduler |
| `maintenance.cronTasks.evolution.intervalSecs` | Hot | Interval read by scheduler |
| `maintenance.cronTasks.reflection.enabled` | Hot | Task toggle checked by scheduler |
| `maintenance.cronTasks.reflection.intervalSecs` | Hot | Interval read by scheduler |
| `maintenance.cronTasks.graphCleanup.enabled` | Hot | Task toggle checked by scheduler |
| `maintenance.cronTasks.graphCleanup.intervalSecs` | Hot | Interval read by scheduler |
| `maintenance.cronTasks.graphCleanupAuditRetentionDays` | **Cold** | Captured into the knowledge maintenance adapter when it is built at startup |

### Pricing (`pricing`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `pricing.{model}.inputCostPerMtok` | Hot | Cost metrics read per-request for telemetry |
| `pricing.{model}.outputCostPerMtok` | Hot | Cost metrics read per-request for telemetry |

### Sandbox (`sandbox`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `sandbox.enabled` | **Cold** | Sandbox config is copied into registered tool executors at startup |
| `sandbox.enforcement` | **Cold** | Enforcement mode is captured by the tool registry at startup |
| `sandbox.allowedRoot` | **Cold** | Root path is captured by the tool registry at startup |
| `sandbox.extraReadPaths` | **Cold** | Extra paths are captured by the tool registry at startup |
| `sandbox.extraWritePaths` | **Cold** | Extra paths are captured by the tool registry at startup |
| `sandbox.extraExecPaths` | **Cold** | Extra paths are captured by the tool registry at startup |
| `sandbox.egress` | **Cold** | Egress policy is captured by the tool registry at startup |
| `sandbox.egressAllowlist` | **Cold** | Allowlist is captured by the tool registry at startup |
| `sandbox.nprocLimit` | **Cold** | Process limit is captured by the tool registry at startup |

### Tools (`tools`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `tools.required` | **Cold** | External tools are registered into the tool registry at startup |
| `tools.optional` | **Cold** | External tools are registered into the tool registry at startup |

### Credential (`credential`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `credential.source` | Hot | Source strategy read per-credential-lookup |
| `credential.claudeCodeCredentials` | Hot | Path override read per-credential-lookup |
| `credential.circuitBreaker.failureThreshold` | Hot | Threshold read per-request |
| `credential.circuitBreaker.failureWindowSecs` | Hot | Window read per-request |
| `credential.circuitBreaker.cooldownSecs` | Hot | Cooldown read per-request |
| `credential.circuitBreaker.maxCooldownSecs` | Hot | Max cooldown read per-request |

### Logging (`logging`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `logging.logDir` | Hot | Log directory read by appender (next rotation) |
| `logging.retentionDays` | Hot | Retention read by cleanup task |
| `logging.level` | Hot | Log level applied dynamically to subscriber |
| `logging.redaction.enabled` | Hot | Redaction toggle read per-log-entry |
| `logging.redaction.redactFields` | Hot | Field list read per-log-entry |
| `logging.redaction.truncateFields` | Hot | Field list read per-log-entry |
| `logging.redaction.truncateLength` | Hot | Truncate limit read per-log-entry |

### MCP (`mcp`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `mcp.rateLimit.enabled` | Hot | Rate limit toggle read per-MCP-request |
| `mcp.rateLimit.messageRequestsPerMinute` | Hot | Limit read per-MCP-request |
| `mcp.rateLimit.readRequestsPerMinute` | Hot | Limit read per-MCP-request |

### Local provider (`localProvider`) — deprecated

`localProvider` is a legacy pass-through section that predates the declarative `[[providers]]` model. It is accepted by the config loader but not validated. Use `[[providers]]` with `providerType = "openai-compatible"` for local inference instead (see `docs/AIR-GAPPED.md`). The `localProvider` section will be removed in a future release.

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `localProvider.enabled` | Hot | Toggle read per-request (pass-through; unvalidated) |
| `localProvider.baseUrl` | Hot | URL read per-request (pass-through; unvalidated) |
| `localProvider.model` | Hot | Model name read per-request (pass-through; unvalidated) |
| `localProvider.timeoutSecs` | Hot | Timeout read per-request (pass-through; unvalidated) |

### Packs (`packs`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `packs` | **Cold** | Packs are loaded once at startup into an `Arc<Vec<LoadedPack>>` snapshot. SIGHUP rebuilds agent configs from the existing snapshot; it does not reload manifests, context files, or pack tools from disk. A restart is required to pick up added, removed, or changed packs. |

---

## Restart prefixes are derived from the registry, not hand-copied

`crates/taxis/src/reload.rs`'s `RESTART_PREFIXES` is not a hand-maintained list: it is a
`LazyLock` derived from `crates/taxis/src/registry.rs`'s `all_specs()`, taking the `key` of
every `ParameterSpec` whose `hot_reloadable` field is `false`. A single declaration (the
registry) drives both the parameter metadata and reload behavior.

An earlier version of this document hand-copied that list as a `const RESTART_PREFIXES`
snippet. The copy was a second source for one fact and it drifted from the registry more
than once — at one point it was missing `gateway.cors`, `gateway.rateLimit`,
`workspace.root`, `providerBehavior.nonStreamingTimeoutSecs`, `messaging.pollIntervalMs`,
`messaging.bufferCapacity`, and `apiLimits.idempotencyCapacity`, and told operators that
rate limiting was safe to change via SIGHUP after the registry had already made it cold
(#5173). There is no second copy to keep in sync anymore: read
`crates/taxis/src/registry.rs` for the authoritative list (grep for
`hot_reloadable: false`), or call `taxis::reload::restart_prefixes()` at runtime — both
resolve to the same `REGISTRY` static and cannot disagree with each other.

### Fields cold at runtime but not covered by a registry restart prefix

**`packs` is Cold but not a registry restart prefix:** the runtime does not force a
restart when `packs` changes — SIGHUP proceeds and rebuilds actor configs from the
existing pack snapshot. Pack manifests, context files, and pack tools are not refreshed
from disk. Operators must restart manually after adding, removing, or modifying packs.
Adding `packs` to the registry with `hot_reloadable: false` would make the runtime
enforce restart automatically.

### Cold field detail: `gateway.cors`

**Note:** The `gateway.cors` prefix is cold (#5173). CORS is a security-relevant control
— treating it as hot meant an operator's tightened or widened origin allowlist could sit
unenforced in the running process while the config file (and any `GET` of it) read as
already applied. The whole subtree is cold, covering `allowedOrigins` and `maxAgeSecs`,
rather than caching a value that can silently diverge from what is on disk.

### Cold field detail: `gateway.rateLimit`

**Note:** The `gateway.rateLimit` prefix is cold (#5173) for the same reason: rate
limiting is a security/availability control, and treating it as hot meant a config that
read as "rate limiting enabled" could be silently unenforced after a SIGHUP that changed
it. The whole subtree is cold, including every `perUser.*` field, via the same
`starts_with` prefix match `requires_restart` uses for every other prefix.

### Cold field detail: `gateway.auth.noneRole` / `gateway.auth.signingKey`

**Note:** These two `gateway.auth.*` leaves are registry-declared cold prefixes (#5324),
alongside `gateway.auth.mode`. Before this, a SIGHUP that changed either field updated the
in-memory config value without applying it to the live runtime, and the diff engine's
hot-by-default fallthrough reported no restart required — an operator could believe a role
or key rotation had taken effect when it had not. `AppState.none_role` (set at startup in
`server.rs`) and the JWT `JwtManager`/`auth_facade` (built once at startup) are not
refreshed on reload, so both must stay cold until that runtime state is rebuilt from the
reloaded config instead.

### Cold field detail: `gateway.csrf`

**Note:** The `RESTART_PREFIXES` includes `gateway.csrf` as a cold prefix, meaning the **entire** CSRF config subtree requires restart. The middleware layer and its configured header name/value are initialized at startup, so changes take effect after restart.

### Cold field detail: `channels`

**Note:** The `channels` prefix covers the entire messaging transport configuration. Signal accounts and their connection parameters are established at server startup. Changes to any channel settings require a restart to re-initialize the transport connections.

### Cold field detail: messaging authority

**Note:** `bindings`, `messaging.outbound`, and `messaging.commands` are registry-declared
cold prefixes. The runtime constructs `MessageRouter` from a binding clone, gives channel
services an outbound-policy clone, and gives the command dispatcher a command-policy clone
at startup. PUT, API reload, and SIGHUP persist a changed policy to disk but retain the old
live config value and report the changed paths as restart-required. This is especially
important for revocation: the control plane must never say a route, recipient, or command
permission was removed while the startup object can still exercise it.

### Cold field detail: `sandbox`

**Note:** The `sandbox` prefix is cold because the runtime copies `config.sandbox` into an `organon::sandbox::SandboxConfig` while building the tool registry. Hot reload only rebuilds actor `NousConfig` values, not the registry or the sandbox config captured by registered tools. A process restart is required to apply sandbox policy changes.

### Cold field detail: `tools`

**Note:** The `tools` prefix is cold because external tool entries are registered into the `ToolRegistry` at startup. Adding, removing, or changing an external tool configuration requires a process restart to rebuild the registry.

---

## Guidelines for operators

### Safe to change via SIGHUP (hot reload)

- **Agent settings**: Model selection, token budgets, thinking settings, tool iterations, recall parameters
- **MCP rate limiting**: `mcp.rateLimit.*` settings (the gateway's own rate limiting is cold — see below)
- **Maintenance schedules**: Trace rotation, drift detection, DB monitoring thresholds
- **Logging**: Log levels, retention, redaction settings

### Requires process restart (cold changes)

- **Network binding**: Port, bind address, TLS certificates
- **Authentication mode**: Switching between token/none/JWT modes
- **Anonymous role assignment** (`gateway.auth.noneRole`): stored in `AppState` at startup; change requires restart
- **JWT signing key** (`gateway.auth.signingKey`): baked into `JwtManager` at startup; key rotation requires restart
- **CORS policy** (`gateway.cors.*`): allowed origins and preflight cache duration (#5173)
- **CSRF protection**: Enabling/disabling CSRF middleware
- **Gateway rate limiting** (`gateway.rateLimit.*`, including every `perUser.*` field): enabling, disabling, and changing thresholds all require a restart to take effect — a SIGHUP that changes these values is silently unenforced until the process restarts (#5173)
- **Request limits**: Body size limits (Axum router configuration)
- **Channel transports**: Signal messenger configuration and account settings
- **Inbound routing authority**: `bindings` entries, including account and participant scopes
- **Outbound recipient authority**: `messaging.outbound.*` allowlist and default-deny policy
- **Inbound command authority**: `messaging.commands.*` operator, public-command, and default-allow policy
- **Sandbox policies**: Enforcement mode, egress rules, path allowances
- **External tool registrations**: `[tools.required]` and `[tools.optional]` entries
- **Domain packs**: Adding, removing, or changing pack paths or pack contents requires a restart to reload manifests, context files, and pack tools from disk. SIGHUP rebuilds actor configs from the startup snapshot only.

---

## Related

- Issue #2315: Hot reload support for configuration changes
- `crates/taxis/src/reload.rs`: Implementation of config diff and reload logic
- `crates/taxis/src/config/`: Configuration type definitions
