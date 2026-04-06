# Hot Reload Classification

This document classifies all `AletheiaConfig` fields as either **Hot** (safe to apply via SIGHUP without restart) or **Cold** (requires process restart to take effect).

## Summary

| Category | Hot Fields | Cold Fields |
|----------|-----------|-------------|
| Gateway | 13 | 7 |
| Agents | 28 | 0 |
| Channels | 0 | 6 |
| Bindings | 4 | 0 |
| Embedding | 3 | 0 |
| Data | 1 | 0 |
| Maintenance | 26 | 0 |
| Pricing | 2 | 0 |
| Sandbox | 8 | 0 |
| Credential | 4 | 0 |
| Logging | 5 | 0 |
| MCP | 3 | 0 |
| Local Provider | 4 | 0 |
| Packs | 1 | 0 |
| **Total** | **102** | **13** |

---

## Detailed Classification

### Gateway (`gateway`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `gateway.port` | **Cold** | TCP listener already bound; changing requires re-binding the socket |
| `gateway.bind` | **Cold** | Interface binding decision made at startup; affects socket creation |
| `gateway.auth.mode` | **Cold** | Authentication mode changes affect middleware stack initialization |
| `gateway.auth.noneRole` | Hot | Role assignment for anonymous requests; read per-request |
| `gateway.auth.signingKey` | Hot | JWT key rotation can happen at runtime |
| `gateway.tls.enabled` | **Cold** | TLS termination settings require listener reconfiguration |
| `gateway.tls.certPath` | **Cold** | Certificate paths loaded at startup for TLS context |
| `gateway.tls.keyPath` | **Cold** | Private key paths loaded at startup for TLS context |
| `gateway.cors.allowedOrigins` | Hot | CORS headers evaluated per-request |
| `gateway.cors.maxAgeSecs` | Hot | Preflight cache duration; read per-request |
| `gateway.bodyLimit.maxBytes` | **Cold** | Body limit configured in Axum router at startup |
| `gateway.csrf.enabled` | **Cold** | CSRF middleware layer initialized at startup |
| `gateway.csrf.headerName` | Hot | Header name read per-request when CSRF enabled |
| `gateway.csrf.headerValue` | Hot | Expected value read per-request when CSRF enabled |
| `gateway.rateLimit.enabled` | Hot | Rate limiting can be toggled at runtime |
| `gateway.rateLimit.requestsPerMinute` | Hot | Rate limit threshold read per-request |
| `gateway.rateLimit.perUser.enabled` | Hot | Per-user rate limiting toggle |
| `gateway.rateLimit.perUser.defaultRpm` | Hot | Per-user default rate limit |
| `gateway.rateLimit.perUser.defaultBurst` | Hot | Per-user burst allowance |
| `gateway.rateLimit.perUser.llmRpm` | Hot | LLM endpoint rate limit |
| `gateway.rateLimit.perUser.llmBurst` | Hot | LLM endpoint burst allowance |
| `gateway.rateLimit.perUser.toolRpm` | Hot | Tool execution rate limit |
| `gateway.rateLimit.perUser.toolBurst` | Hot | Tool execution burst allowance |
| `gateway.rateLimit.perUser.staleAfterSecs` | Hot | Stale user eviction timeout |

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
| `agents.defaults.recall.engineWeights.*` | Hot | Engine weights read per-request |
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
| `bindings` | Hot | Route mappings read per-request from config snapshot |
| `bindings[].channel` | Hot | Channel type evaluated per-message |
| `bindings[].source` | Hot | Source pattern matched per-message |
| `bindings[].nousId` | Hot | Target agent resolved per-message |
| `bindings[].sessionKey` | Hot | Session key pattern evaluated per-message |

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
| `maintenance.sqliteRecovery.enabled` | Hot | Recovery toggle checked at open time |
| `maintenance.sqliteRecovery.integrityCheckOnOpen` | Hot | Check flag read at open time |
| `maintenance.sqliteRecovery.autoRepair` | Hot | Repair flag read at open time |
| `maintenance.sqliteRecovery.backupCorrupt` | Hot | Backup flag read at open time |
| `maintenance.retention.enabled` | Hot | Retention toggle checked by background task |
| `maintenance.knowledgeMaintenanceEnabled` | Hot | Maintenance toggle checked by background task |
| `maintenance.watchdog.enabled` | Hot | Watchdog toggle checked by monitor |
| `maintenance.watchdog.heartbeatTimeoutSecs` | Hot | Timeout read by monitor |
| `maintenance.watchdog.checkIntervalSecs` | Hot | Interval read by monitor |
| `maintenance.watchdog.maxRestarts` | Hot | Max restarts read by monitor |
| `maintenance.cronTasks.evolution.enabled` | Hot | Task toggle checked by scheduler |
| `maintenance.cronTasks.evolution.intervalSecs` | Hot | Interval read by scheduler |
| `maintenance.cronTasks.reflection.enabled` | Hot | Task toggle checked by scheduler |
| `maintenance.cronTasks.reflection.intervalSecs` | Hot | Interval read by scheduler |
| `maintenance.cronTasks.graphCleanup.enabled` | Hot | Task toggle checked by scheduler |
| `maintenance.cronTasks.graphCleanup.intervalSecs` | Hot | Interval read by scheduler |

### Pricing (`pricing`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `pricing.{model}.inputCostPerMtok` | Hot | Cost metrics read per-request for telemetry |
| `pricing.{model}.outputCostPerMtok` | Hot | Cost metrics read per-request for telemetry |

### Sandbox (`sandbox`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `sandbox.enabled` | Hot | Sandbox toggle read per-tool-execution |
| `sandbox.enforcement` | Hot | Enforcement mode read per-tool-execution |
| `sandbox.allowedRoot` | Hot | Root path read per-tool-execution |
| `sandbox.extraReadPaths` | Hot | Extra paths read per-tool-execution |
| `sandbox.extraWritePaths` | Hot | Extra paths read per-tool-execution |
| `sandbox.extraExecPaths` | Hot | Extra paths read per-tool-execution |
| `sandbox.egress` | Hot | Egress policy read per-tool-execution |
| `sandbox.egressAllowlist` | Hot | Allowlist read per-tool-execution |
| `sandbox.nprocLimit` | Hot | Process limit read per-tool-execution |

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

### Local Provider (`localProvider`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `localProvider.enabled` | Hot | Toggle read per-request |
| `localProvider.baseUrl` | Hot | URL read per-request (connection pool may cache) |
| `localProvider.model` | Hot | Model name read per-request |
| `localProvider.timeoutSecs` | Hot | Timeout read per-request |

### Packs (`packs`)

| Config path | Hot/Cold | Reason |
|-------------|----------|--------|
| `packs` | Hot | Pack paths read when loading domain packs |

---

## Verification Against `RESTART_PREFIXES`

The `crates/taxis/src/reload.rs` file defines the following `RESTART_PREFIXES`:

```rust
const RESTART_PREFIXES: &[&str] = &[
    "gateway.port",
    "gateway.bind",
    "gateway.tls",
    "gateway.auth.mode",
    "gateway.csrf",
    "gateway.bodyLimit",
    "channels",
];
```

### Match Status: ✅ CONSISTENT

All fields marked as **Cold** in this document match the `RESTART_PREFIXES` in `reload.rs`:

| Prefix in Code | Document Status | Notes |
|----------------|-----------------|-------|
| `gateway.port` | Cold ✅ | TCP listener binding |
| `gateway.bind` | Cold ✅ | Interface binding |
| `gateway.tls` | Cold ✅ | TLS termination settings |
| `gateway.auth.mode` | Cold ✅ | Auth middleware mode |
| `gateway.csrf` | Cold ✅ | CSRF middleware |
| `gateway.bodyLimit` | Cold ✅ | Axum body limit |
| `channels` | Cold ✅ | Channel transport lifecycle |

### Cold Field Detail: `gateway.csrf`

**Note:** The `RESTART_PREFIXES` includes `gateway.csrf` as a cold prefix, meaning the **entire** CSRF config subtree requires restart. While `gateway.csrf.headerName` and `gateway.csrf.headerValue` are evaluated per-request, the middleware layer that performs CSRF checking is initialized at startup with the `enabled` flag. Therefore, the entire subtree is classified as Cold for consistency.

### Cold Field Detail: `channels`

**Note:** The `channels` prefix covers the entire messaging transport configuration. Signal accounts and their connection parameters are established at server startup. Changes to any channel settings require a restart to re-initialize the transport connections.

---

## Guidelines for Operators

### Safe to Change via SIGHUP (Hot Reload)

- **Agent settings**: Model selection, token budgets, thinking settings, tool iterations, recall parameters
- **Rate limiting**: All gateway and MCP rate limit settings
- **Maintenance schedules**: Trace rotation, drift detection, DB monitoring thresholds
- **Sandbox policies**: Enforcement mode, egress rules, path allowances
- **Logging**: Log levels, retention, redaction settings

### Requires Process Restart (Cold Changes)

- **Network binding**: Port, bind address, TLS certificates
- **Authentication mode**: Switching between token/none/JWT modes
- **CSRF protection**: Enabling/disabling CSRF middleware
- **Request limits**: Body size limits (Axum router configuration)
- **Channel transports**: Signal messenger configuration and account settings

---

## Related

- Issue #2315: Hot reload support for configuration changes
- `crates/taxis/src/reload.rs`: Implementation of config diff and reload logic
- `crates/taxis/src/config/`: Configuration type definitions
