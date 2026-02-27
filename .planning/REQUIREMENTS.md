# Requirements: Aletheia v1.2 Onboarding & Mac Support

**Defined:** 2026-02-26
**Core Value:** A personal AI runtime you fully control — persistent across sessions, extensible through modules, and powerful enough to handle complex multi-agent work without losing context.

## v1.2 Requirements

Requirements for this milestone. Each maps to roadmap phases.

### Compatibility Fixes

- [x] **COMPAT-01**: User can run `bin/aletheia` on macOS without errors (bash 3.2 compat — `declare -A` replaced with parallel indexed arrays)
- [x] **COMPAT-02**: User can run memory services on any machine without data loss (docker-compose volumes use `${ALETHEIA_DATA:-$HOME/.aletheia/data}` instead of `/mnt/ssd/`)
- [x] **COMPAT-03**: User can install Aletheia on a fresh Linux clone without a broken memory service (`aletheia-memory.service` uses `ALETHEIA_HOME` template, not hardcoded `ergon` path)
- [x] **COMPAT-04**: User can stop the gateway cleanly under launchd (SIGTERM handler calls `process.exit(0)` so launchd recognizes a clean stop and does not restart)

### Mac Boot Persistence

- [x] **LAUNCHD-01**: User can run `aletheia enable` to wire up boot persistence (installs launchd plists on Mac, enables systemd units on Linux)
- [x] **LAUNCHD-02**: User can run `aletheia disable` to remove boot persistence (launchd `bootout` on Mac, `systemctl disable` on Linux)
- [x] **LAUNCHD-03**: Launchd plist templates exist in `config/services/` for gateway and memory (with placeholder tokens substituted at `aletheia enable` time — captures real `node` path, real `ALETHEIA_HOME`, explicit `PATH` including Homebrew)
- [x] **LAUNCHD-04**: User can start memory services independently with `aletheia start --memory-only` (used by launchd memory plist to start docker-compose without the gateway)

### Doctor & CLI Wizard

- [ ] **DOCTOR-01**: User can see HTTP connectivity status for all services in `aletheia doctor` (gateway, Qdrant, Neo4j, mem0 health endpoints — async checks with 3-second timeout)
- [ ] **DOCTOR-02**: User can see dependency health in `aletheia doctor` (Node 22+, Docker/Podman presence, build artifact existence)
- [ ] **DOCTOR-03**: User can see boot persistence status in `aletheia doctor` (launchd state on Mac, systemd state on Linux — enabled or not)
- [ ] **INIT-01**: User can complete `aletheia init` without manually locating their API key (auto-detects from `~/.claude.json` same as web wizard)
- [ ] **INIT-02**: User can set their profile during `aletheia init` (name/role/style step — matches web wizard, currently missing from CLI path)

### Setup Script Mac Support

- [ ] **SETUP-01**: User can run `setup.sh` on macOS without shell errors (portable port conflict detection — not Linux-specific `lsof -iTCP` flags)
- [ ] **SETUP-02**: User sees Homebrew prerequisite guidance on Mac (check for Homebrew; point to native Qdrant binary and `brew install neo4j` for no-Docker path)
- [ ] **SETUP-03**: User is offered boot persistence at the end of `setup.sh` (optionally calls `aletheia enable` after first build)
- [ ] **SETUP-04**: User knows how to start Aletheia next time after `setup.sh` completes (end-banner: "Next time: `aletheia start`")

### Documentation

- [ ] **DOCS-01**: User can follow a complete Mac deployment guide in DEPLOYMENT.md (launchd plist content, `bootstrap`/`bootout` commands, log location, Homebrew prerequisites, `aletheia enable`/`disable` reference)
- [ ] **DOCS-02**: User can trust QUICKSTART.md as accurate (all described features exist and work — no aspirational/non-existent behavior)

### Bug Fixes

- [x] **BUG-01**: User can complete a Dianoia planning session without stream timeout errors (`sessions_dispatch` long-running sub-agent calls succeed — root cause of #208 investigated and fixed)

## v2 Requirements

Deferred to a future release.

### Enhanced Onboarding

- **ONBOARD-01**: User can use `@clack/prompts` enhanced CLI wizard (richer interactive prompts; polish after daily use patterns emerge)
- **ONBOARD-02**: User can run `aletheia init --reset` for a full guided re-setup
- **ONBOARD-03**: User gets auto-remediation from `aletheia doctor --fix` (beyond hint-only: fix build artifacts, restart services)

### Windows/WSL2

- **WIN-01**: User can run Aletheia on Windows via WSL2 (when user demand confirmed)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Docker Desktop requirement for Mac | Native Qdrant binary + Homebrew Neo4j are available; no Docker required on Mac is the milestone goal |
| pm2 / process manager | launchd and systemd are native OS solutions; avoid npm global dependencies in the operational layer |
| Auto-install of dependencies | Detect and tell the user what to run; never install unbidden software |
| Slack integration (#210) | Different domain — deferred to integrations milestone |
| Dianoia bugs #233 (full list) | Bug fixes tracked on GitHub; not onboarding-blocking |
| Rename issues (#226, #227, #228, #229) | Breaking changes requiring a dedicated milestone |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| COMPAT-01 | Phase 14 | Complete |
| COMPAT-02 | Phase 14 | Complete |
| COMPAT-03 | Phase 14 | Complete |
| COMPAT-04 | Phase 14 | Complete |
| LAUNCHD-01 | Phase 15 | Complete |
| LAUNCHD-02 | Phase 15 | Complete |
| LAUNCHD-03 | Phase 15 | Complete |
| LAUNCHD-04 | Phase 15 | Complete |
| DOCTOR-01 | Phase 16 | Pending |
| DOCTOR-02 | Phase 16 | Pending |
| DOCTOR-03 | Phase 16 | Pending |
| INIT-01 | Phase 16 | Pending |
| INIT-02 | Phase 16 | Pending |
| BUG-01 | Phase 16 | Complete |
| SETUP-01 | Phase 17 | Pending |
| SETUP-02 | Phase 17 | Pending |
| SETUP-03 | Phase 17 | Pending |
| SETUP-04 | Phase 17 | Pending |
| DOCS-01 | Phase 17 | Pending |
| DOCS-02 | Phase 17 | Pending |

**Coverage:**
- v1.2 requirements: 20 total
- Mapped to phases: 20
- Unmapped: 0

---
*Requirements defined: 2026-02-26*
*Last updated: 2026-02-26 — traceability completed after roadmap creation*
