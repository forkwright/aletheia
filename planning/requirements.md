# Aletheia — Requirements Tracking

Functional and non-functional requirements. Updated through Wave 9 (#756).

**Last updated:** 2026-03-12 (Wave 9 complete)

---

## Memory (MEM)

| ID | Requirement | Status |
|----|-------------|--------|
| MEM-01 | Hybrid recall: vector + graph + BM25 with MMR diversity | Done |
| MEM-02 | Mem0 sidecar integration | Inactive — replaced by mneme direct implementation |
| MEM-03 | Explicit forgetting API | Done (#736) |
| MEM-04 | Distillation + memory flush (melete) | Done |

---

## Mneme Phases

| Phase | Description | Status |
|-------|-------------|--------|
| A | SQLite session store — WAL, migrations, retention | Done |
| B | CozoDB engine absorption — vendored Datalog + HNSW | Done |
| C | Embedding provider trait + candle default | Done (#693) |
| D | Hybrid recall pipeline — vector + graph + BM25 + MMR | Done |
| E | Error remediation — snafu migration, BoxErr elimination | Done (#749, #752) |
| F | Engine hygiene — unsafe SAFETY docs, lint suppressions, dead code | Done (#750, #752, #753) |

---

## Engine Hygiene (completed Wave 8–9)

- **Snafu migration:** All library crates use snafu error enums. `anyhow` limited to binary entry points. Done.
- **Unwrap elimination:** No `unwrap()` in library code. Documented exceptions with SAFETY comments. Done.
- **Facade consolidation:** `BoxErr`/`AdhocError`/`DbResult` eliminated. `InternalError` composition enum with `#[snafu(context(false))]`. Done (#749).
- **Lint suppressions:** Zero blanket `#[allow]`. All suppressions use `#[expect]` with reason strings. Done (#752).
- **Unsafe sites:** All 14 unsafe sites (12 blocks + 2 impls) documented with `// SAFETY:` comments. Done (#753).

---

## Skills (SKILL)

| ID | Requirement | Status |
|----|-------------|--------|
| SKILL-01 | Skill extraction from interaction history | Done (#676) |
| SKILL-02 | Skill storage in shared workspace | Done (#676) |
| SKILL-03 | Skill injection into context | Done (#683) |
| SKILL-04 | Skill export/import (autarkeia) | Done (#507) |
| SKILL-05 | Cross-nous skill sharing | Done (#696) |
| SKILL-06 | Skill versioning | Done (#696) |
| SKILL-07 | Skill quality lifecycle — promotion, demotion, expiry | Done (#740) |

---

## Agent Model (NOUS)

| ID | Requirement | Status |
|----|-------------|--------|
| NOUS-01 | NousActor Tokio actor model | Done |
| NOUS-02 | Multi-nous concurrent execution | Done |
| NOUS-03 | Cross-nous session routing | Done |
| NOUS-04 | Daemon background tasks (oikonomos) | Done |
| NOUS-05 | Actor crash recovery + supervisor restart | Done (#739) |
| NOUS-06 | Daemon task backoff + health checks | Done (#738) |

---

## Plugins (PLUG)

| ID | Requirement | Status |
|----|-------------|--------|
| PLUG-01 | WASM plugin host (wasmtime) | Planned — M6 |
| PLUG-02 | Agent export/import (autarkeia) | Done (#507) |

---

## Non-Functional

| ID | Requirement | Status |
|----|-------------|--------|
| NF-01 | Single static binary | Done |
| NF-02 | Zero external services (embedded CozoDB, SQLite, candle) | Done |
| NF-03 | Zero telemetry / phone-home | Done |
| NF-04 | Cross-compile: Linux x86_64 + aarch64, macOS | Done |
| NF-05 | Cargo clippy zero warnings across workspace | Done (#750, #752) |
| NF-06 | No `unwrap()` in library code | Done (#752) |
