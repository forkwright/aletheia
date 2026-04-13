# Constants Audit — #2306 Wave 7 (Completion)

**Issue:** forkwright/aletheia#2306
**Date:** 2026-04-13
**Method:** `grep -rn 'const [A-Z_]*:' crates/ --include='*.rs'` across all crates (excluding krites), then manual categorization of every match. Constants inside `#[cfg(test)]` blocks, `_tests.rs`, `/tests/`, `/benches/`, `distill_tests/`, `roundtrip_tests/` modules excluded as test-only. Type assertions (`const _:`) and const generics excluded as non-declarations.
**Scope:** All Rust crates under `crates/` except `krites` (vendored CozoDB).

---

## Summary

| Category | Count |
|----------|------:|
| Total `const` declarations (excl. krites) | 1232 |
| Excluded: type assertions (`const _:`) | 46 |
| Excluded: const generics | 1 |
| Excluded: test-only (test files + inline `#[cfg(test)]` + test submodules) | 60 |
| **Auditable total** | **1125** |
| Parameterized (taxis config) | 143 |
| True invariants | 979 |
| **Missed (needs follow-up)** | **3** |

Parameterized + invariant + missed = 1125. All non-test constants accounted for.

---

## Missed constants (needs follow-up)

These are behavioral constants that should be parameterized through taxis config but were not caught in waves 1-4.

| Crate | File | Const | Value | Purpose | Suggested config section |
|-------|------|-------|-------|---------|--------------------------|
| eidos | `src/knowledge/fact.rs:10` | `MAX_CONTENT_LENGTH` | `102_400` | Maximum byte length for fact content strings | `[knowledge] max_fact_content_length = 102400` |
| episteme | `src/manifest.rs:18` | `MAX_MEMORY_ENTRIES` | `200` | Maximum memory entries in side-query manifest | `[knowledge] manifest_max_memory_entries = 200` |
| symbolon | `src/credential/mod.rs:51` | `REFRESH_THRESHOLD_SECS` | `3600` | Seconds remaining before OAuth token refresh triggers | `[credential] refresh_threshold_secs = 3600` |

---

## Parameterized constants (taxis config) — 143 total

These constants serve as compile-time defaults for taxis config fields. The runtime value is read from the resolved config; the `const` remains as the fallback default.

### koina::defaults (13)

| Const | Value | Taxis field |
|-------|-------|-------------|
| `DEFAULT_CONFIG_PATH` | `"config/aletheia.toml"` | loader path |
| `DEFAULT_MODEL` | `"claude-sonnet-4-20250514"` | `agents.defaults.model.primary` |
| `DEFAULT_MODEL_SHORT` | `"claude-sonnet-4-6"` | `agents.defaults.model.primary` |
| `MAX_OUTPUT_TOKENS` | `16_384` | `agents.defaults.maxOutputTokens` |
| `BOOTSTRAP_MAX_TOKENS` | `40_000` | `agents.defaults.bootstrapMaxTokens` |
| `CONTEXT_TOKENS` | `200_000` | `agents.defaults.contextTokens` |
| `OPUS_CONTEXT_TOKENS` | `1_000_000` | `capacity.opusContextTokens` |
| `MAX_TOOL_ITERATIONS` | `200` | `agents.defaults.maxToolIterations` |
| `MAX_TOOL_RESULT_BYTES` | `32_768` | `agents.defaults.maxToolResultBytes` |
| `TIMEOUT_SECONDS` | `300` | `timeouts.llmCallSecs` |
| `HISTORY_BUDGET_RATIO` | `0.6` | `agents.defaults.historyBudgetRatio` |
| `CHARS_PER_TOKEN` | `4` | `agents.defaults.charsPerToken` |
| `MAX_OUTPUT_BYTES` | `50 * 1024` | `capacity.maxToolOutputBytes` |

### daemon (5) — `DaemonBehaviorConfig`

| Const | Value | Taxis field |
|-------|-------|-------------|
| `DEFAULT_HEARTBEAT_TIMEOUT` | `1 min` | `maintenance.watchdog.heartbeatTimeoutSecs` |
| `DEFAULT_CHECK_INTERVAL` | `10s` | `maintenance.watchdog.checkIntervalSecs` |
| `DEFAULT_MAX_RESTARTS` | `5` | `maintenance.watchdog.maxRestarts` |
| `BACKOFF_BASE` | `2s` | `daemonBehavior.watchdogBackoffBaseSecs` |
| `BACKOFF_CAP` | `5 min` | `daemonBehavior.watchdogBackoffCapSecs` |

### daemon (3) — prosoche + runner

| Const | Value | Taxis field |
|-------|-------|-------------|
| `ANOMALY_SAMPLE_SIZE` | `15` | `daemonBehavior.prosocheAnomalySampleSize` |
| `DEFAULT_BRIEF_HEAD_LINES` | `5` | `daemonBehavior.runnerOutputBriefHeadLines` |
| `DEFAULT_BRIEF_TAIL_LINES` | `3` | `daemonBehavior.runnerOutputBriefTailLines` |

### nous (16) — `NousBehaviorConfig`

Includes actor thresholds, inbox capacity, spawned task limits, session caps, GC interval, manager health/restart parameters, loop detection, and self-audit event threshold.

### nous (30+) — `AgentBehaviorDefaults`

Includes safety (loop detection, error threshold, token cap), hooks (cost control, scope, correction, audit), distillation triggers, competence scoring, drift detection, uncertainty calibration, skills, working state, planning stuck-detection, knowledge tuning (instinct, surprise, rules, dedup), fact lifecycle, similarity, dream consolidation, tool behavior, bootstrap, and corrections.

### episteme (17) — `KnowledgeConfig`

Includes conflict resolution (LLM calls, dedup threshold, distance threshold, max candidates), decay (reinforcement boost, cross-agent bonus), extraction (confidence, fact length), instinct (tool calls, param/context length), and side-query (max results, cache TTL/capacity).

### hermeneus (8) — `ProviderBehaviorConfig` + `RetrySettings`

| Const | Value | Taxis field |
|-------|-------|-------------|
| `NON_STREAMING_TIMEOUT` | `2 min` | `providerBehavior.nonStreamingTimeoutSecs` |
| `SSE_DEFAULT_RETRY_MS` | `1000` | `providerBehavior.sseDefaultRetryMs` |
| `DEFAULT_EWMA_ALPHA` | `0.8` | `providerBehavior.concurrencyEwmaAlpha` |
| `DEFAULT_LATENCY_THRESHOLD_SECS` | `30.0` | `providerBehavior.concurrencyLatencyThresholdSecs` |
| `DEFAULT_LOW_THRESHOLD` | `30` | `providerBehavior.complexityLowThreshold` |
| `DEFAULT_HIGH_THRESHOLD` | `70` | `providerBehavior.complexityHighThreshold` |
| `DEFAULT_MAX_RETRIES` | `3` | `retry.maxAttempts` |
| `BACKOFF_BASE_MS` / `BACKOFF_MAX_MS` | `1000` / `30000` | `retry.backoffBaseMs` / `retry.backoffMaxMs` |

### organon (11) — `ToolLimitsConfig` + `AgentBehaviorDefaults`

Includes filesystem limits (pattern length, subprocess timeout, read/write bytes, command length), communication limits (message length, inter-session message/timeout), agent dispatch, datalog query defaults, and view-file size limits.

### agora (8) — `MessagingConfig`

| Const | Value | Taxis field |
|-------|-------|-------------|
| `DEFAULT_POLL_INTERVAL` | `2s` | `messaging.pollIntervalMs` |
| `DEFAULT_BUFFER_CAPACITY` | `100` | `messaging.bufferCapacity` |
| `CIRCUIT_BREAKER_THRESHOLD` | `5` | `messaging.circuitBreakerThreshold` |
| `HALTED_HEALTH_CHECK_INTERVAL` | `1 min` | `messaging.haltedHealthCheckIntervalSecs` |
| `RPC_TIMEOUT` | `10s` | `messaging.rpcTimeoutSecs` |
| `HEALTH_TIMEOUT` | `2s` | `messaging.healthTimeoutSecs` |
| `RECEIVE_TIMEOUT` | `15s` | `messaging.receiveTimeoutSecs` |
| `MAX_CONCURRENT_HANDLERS` | `64` | `messaging.maxConcurrentHandlers` |

### pylon (3) — `ApiLimitsConfig`

| Const | Value | Taxis field |
|-------|-------|-------------|
| `DEFAULT_TTL` | `5 min` | `apiLimits.idempotencyTtlSecs` |
| `DEFAULT_CAPACITY` | `10_000` | `apiLimits.idempotencyCapacity` |
| `DEFAULT_MAX_KEY_LENGTH` | `64` | `apiLimits.idempotencyMaxKeyLength` |

### dianoia (2) — `AgentBehaviorDefaults`

| Const | Value | Taxis field |
|-------|-------|-------------|
| `DEFAULT_MAX_ITERATIONS` | `10` | `behavior.planningMaxIterations` |
| `DEFAULT_TIMESTAMP_TOLERANCE_SECS` | `5` | `behavior.planningReconcilerTimestampToleranceSecs` |

### koina disk_space (2) — `DiskSpaceSettings`

| Const | Value | Taxis field |
|-------|-------|-------------|
| `DEFAULT_WARNING_BYTES` | `1 GB` | `maintenance.diskSpace.warningThresholdMb` |
| `DEFAULT_CRITICAL_BYTES` | `100 MB` | `maintenance.diskSpace.criticalThresholdMb` |

### eidos (3) — `AgentBehaviorDefaults`

| Const | Value | Taxis field |
|-------|-------|-------------|
| `DEFAULT_STAGE_ACTIVE_THRESHOLD` | `0.7` | `behavior.factActiveThreshold` |
| `DEFAULT_STAGE_FADING_THRESHOLD` | `0.3` | `behavior.factFadingThreshold` |
| `DEFAULT_STAGE_DORMANT_THRESHOLD` | `0.1` | `behavior.factDormantThreshold` |

### melete (6) — `AgentBehaviorDefaults`

Dream consolidation, distillation backoff, similarity, and tool result length thresholds.

### episteme instinct/skill/dedup/surprise/rules (15+) — `AgentBehaviorDefaults`

Instinct observations, success rate, stability hours; skill promotion/similarity thresholds; dedup weights; surprise threshold/EMA; rule proposal thresholds.

---

## True invariants (verified) — 979 total

### By category

| Category | Count | Description |
|----------|------:|-------------|
| CSS style literals | ~470 | Dioxus desktop (proskenion) component/view styles |
| UI layout constants | ~165 | TUI (koilon) and desktop layout dimensions, heights, widths |
| Classification pattern lists | ~65 | Keyword lists, regex patterns, stop words, blocked hosts |
| Datalog query strings | ~25 | CozoDB DDL/DML query constants |
| LLM prompt templates | ~20 | System prompts, extraction prompts, role prompts |
| Protocol/format constants | ~45 | API paths, content types, prefixes, filenames, versions |
| Mathematical constants | ~20 | Unit conversions, PDF dimensions, PageRank damping |
| Storage key prefixes | ~10 | Fjall/energeia partition and key prefix strings |
| Scaffold templates | ~15 | Init scaffold file content (SOUL, IDENTITY, etc.) |
| Color/theme palettes | ~10 | Chart colors, accent presets, series colors |
| Icon/glyph literals | ~10 | Unicode icons for status indicators |
| Enum variant arrays | ~8 | `ALL`, `FIXED` variant lists for iteration |
| Metric name/description | ~12 | Health metric NAME/DESC trait constants |
| Lookup tables | ~5 | Month names, day-of-week names, encoding tables |
| Validation allowlists | ~10 | Valid auth modes, channel types, sort fields |
| Crypto/encoding constants | ~15 | Nonce length, key length, ULID encoding table |
| Concurrency outcome codes | 3 | `OUTCOME_NEUTRAL`, `OUTCOME_SUCCESS`, `OUTCOME_OVERLOAD` |
| Config validation boundaries | ~10 | Max token budget, max backoff, restart prefixes |
| File export safety limits | ~5 | Max file size, binary probe size, ignore dirs |
| Eval framework constants | 4 | Self-assessment prompt, recall K values, sycophancy patterns |
| Other invariants | ~50 | Landlock ABI version, model identifiers, credential service names |

### theatron/proskenion — 648 constants

All CSS style string literals and UI rendering constants for the Dioxus desktop app. These define the visual presentation layer and are not behavioral parameters. Examples: `CARD_STYLE`, `BUTTON_STYLE`, `SECTION_LABEL_STYLE`, `OVERLAY_BACKDROP`, `TOAST_STYLE`.

### theatron/koilon — 110 constants

TUI layout constants (sidebar width, min terminal dimensions, sparkline capacity, scroll buffer, blink intervals), theme characters, command palette limits, editor settings. All rendering-layer invariants.

### theatron/skene — 6 constants

Discovery port, probe/total timeouts, LAN hostnames, Tailscale IPs, SSE read timeout.

### energeia — 30 constants

Dispatch store scan limits, schema prefixes, metric names/descriptions, QA keyword lists, cost pricing formulas, concurrency outcome codes.

### episteme — 91 constants (74 invariant after parameterized)

Datalog queries (entity neighborhood, semantic search, hybrid search, temporal diffs, consolidation candidates, graph scores), extraction system prompt, refinement pattern lists (correction cues, trivial patterns, turn type appendices), vocabulary control lists, staleness stop words, causal cue table, knowledge DDL, schema version.

### nous — 51 constants (35 invariant after parameterized)

Bootstrap workspace file specs, role prompts (coder, researcher, reviewer, explorer, runner), keyword lists (coding, research, planning, conversation), model name aliases, hook priority ordering, correction filename/prefixes, summarization/micro-cleared prompts.

### hermeneus — 25 constants (17 invariant after parameterized)

Model name constants (OPUS, SONNET, HAIKU), API version/base URL, supported CC models list, Anthropic wire cache breakpoints, pricing discount/premium factors, CC profile fingerprint salt, core betas list.

### organon — 23 constants (12 invariant after parameterized)

Protected files/extensions/prefixes/dot-prefixes/substrings lists, shell metacharacters, blocked research hostnames, Landlock ABI version, mutation keywords for Datalog safety.

### graphe — 10 constants

Schema DDL, valid categories list, sequence width, agent file version, export limits (max file size, binary probe size, ignore dirs, binary extensions).

### koina — 24 constants (22 invariant after defaults)

HTTP content types, bearer prefix, API health path, ULID encoding/decoding tables, secret redaction string, output buffer dead letter key, disk space byte conversions.

### eidos — 9 constants (6 invariant after parameterized)

Knowledge scope ALL/PathValidationLayer ALL variant arrays, path validation FS layers count, symlink hop limit (mirrors Linux ELOOP), max ID length.

### pylon — 9 constants (6 invariant after parameterized)

Metrics content type, discovery file name, valid sort/order fields, valid config sections, CSRF default, handler-specific limits already in ApiLimitsConfig.

### symbolon — 19 constants (18 invariant after missed 1)

API key prefix, hex chars, JWT insecure default key, OAuth client ID/token URL/token prefix, encrypted sentinel, nonce/key length, credential keyring service/username, refresh check interval, file mtime check interval, min/max expires_in, clock skew leeway.

### taxis — 16 constants (all invariant)

Validation boundaries (max token budget, max tool output bytes, max backoff), restart-requiring config prefixes, redaction sentinel string, sensitive field/key/TOML key lists, known channel types, valid auth modes, encryption prefix/nonce/key/tag lengths.

### dianoia — 6 constants (4 invariant after parameterized)

Research depth ALL variant array, handoff filenames (`.continue-here.json`, `.continue-here.md`), intents filename.

### aletheia — 18 constants (all invariant)

Scaffold template content (SOUL, IDENTITY, AGENTS, etc.), single-agent sandbox TOML template, binary name, REPL banner/help text, academic API constants.

### agora — 8 constants (all parameterized, see above)

### poiesis/text — 9 constants (all invariant)

PDF page dimensions, margins, font sizes, leading, paragraph/heading spacing. A4 rendering constants.

### thesauros — 1 constant (invariant)

`MANIFEST_FILENAME = "pack.toml"` — domain pack manifest filename.

---

## Test-only constants (excluded) — 60 total

| Location | Count | Examples |
|----------|------:|---------|
| `*_tests.rs` and `/tests/` files | 24 | Standard test module constants |
| `benches/` files | 8 | Benchmark fixtures (`SHORT_TEXT`, `MEDIUM_TEXT`, `SHORT_BODY`) |
| `distill_tests/` + `roundtrip_tests/` | 6 | `MOCK_SUMMARY`, `FULL_SUMMARY` test fixtures |
| Inline `#[cfg(test)]` in production files | 22 | `MIN_RESPONSES_FOR_QUALITY`, `SAMPLE_DIFF`, `TEST_MAX_KEY_LEN`, `DIM`, etc. |

---

## Type assertions and const generics (excluded) — 47 total

| Pattern | Count | Purpose |
|---------|------:|---------|
| `const _: fn() = \|\| { ... }` | 46 | Compile-time trait bound assertions |
| `fn split_vertical<const N: usize>` | 1 | Const generic parameter (not a const declaration) |

---

## Methodology notes

1. **Grep scope:** All `.rs` files under `crates/` excluding `crates/krites/` (vendored CozoDB with hundreds of internal constants outside project control).

2. **Parameterization verification:** Cross-referenced each constant name against taxis config struct fields in `crates/taxis/src/config/mod.rs` and `crates/taxis/src/config/maintenance.rs`. A constant is "parameterized" if its value serves as the `Default` impl for a taxis config field, and runtime code reads from the resolved config.

3. **Invariant criteria:** A constant is a true invariant if it falls into one of: (a) protocol/format constant (API versions, content types, schema DDL), (b) mathematical constant or unit conversion, (c) classification pattern list (keywords, regex, stop words), (d) UI presentation literal (CSS, layout dimensions, icons), (e) LLM prompt template, (f) compile-time structural constant (enum variant array, const generic dependency), (g) security/crypto constant (key length, nonce size).

4. **Missed criteria:** A constant is "missed" if it controls runtime behavior (thresholds, limits, timeouts, capacities) and an operator could reasonably want to tune it per-deployment or per-agent, but it has no corresponding taxis config field.
