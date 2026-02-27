# Roadmap: Aletheia

## Milestones

- ✅ **v1.0 Dianoia MVP** — Phases 1-9 (shipped 2026-02-25) — [archive](milestones/v1.0-ROADMAP.md)
- ✅ **v1.1 Standards & Hardening** — Phases 10-13 (shipped 2026-02-26) — [archive](milestones/v1.1-ROADMAP.md)
- 🚧 **v1.2 Onboarding & Mac Support** — Phases 14-17 (in progress)

## Phases

<details>
<summary>✅ v1.0 Dianoia MVP (Phases 1-9) — SHIPPED 2026-02-25</summary>

See [milestones/v1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md)

</details>

<details>
<summary>✅ v1.1 Standards & Hardening (Phases 10-13) — SHIPPED 2026-02-26</summary>

See [milestones/v1.1-ROADMAP.md](milestones/v1.1-ROADMAP.md)

</details>

### 🚧 v1.2 Onboarding & Mac Support (In Progress)

**Milestone Goal:** Make it trivially easy for a team member to get Aletheia to a working deployment on Linux or Mac — native, no Docker required.

- [x] **Phase 14: Portability Fixes** - Fix three concrete breakage points that prevent Aletheia from running on any machine that is not the original developer's Linux workstation (completed 2026-02-26)
- [ ] **Phase 15: Mac Boot Persistence** - Add launchd-backed `aletheia enable`/`disable` so Aletheia survives reboots on macOS with the same reliability as systemd on Linux
- [ ] **Phase 16: Doctor, CLI Wizard, and Bug Fix** - Extend `aletheia doctor` with connectivity and dependency checks, bring `aletheia init` to parity with the web wizard, and fix the Dianoia stream timeout bug
- [ ] **Phase 17: Setup Script Mac Support and Documentation** - Make `setup.sh` run end-to-end on macOS and write accurate deployment docs for both platforms

## Phase Details

### Phase 14: Portability Fixes
**Goal**: Aletheia runs without errors on any machine — macOS or a fresh Linux clone — with no hardcoded paths to the original developer's environment
**Depends on**: Nothing (first v1.2 phase)
**Requirements**: COMPAT-01, COMPAT-02, COMPAT-03, COMPAT-04
**Success Criteria** (what must be TRUE):
  1. User can run `bin/aletheia` on macOS without a crash or error at startup (bash 3.2 compat — `declare -A` replaced with parallel indexed arrays)
  2. User can start memory services on any machine and confirm data persists across restarts (docker-compose volumes use `${ALETHEIA_DATA:-$HOME/.aletheia/data}` — no hardcoded `/mnt/ssd/` path)
  3. User can clone Aletheia to a new Linux machine and run `aletheia enable` without a broken memory service (`aletheia-memory.service` contains no reference to `ergon` — path is templated and resolved at install time)
  4. User can stop the gateway under launchd and confirm it does not immediately restart (SIGTERM handler calls `process.exit(0)` so launchd reads exit code 0 as a clean stop)
**Plans**: 2 plans

Plans:
- [ ] 14-01-PLAN.md — Bash 3.2 compat (declare -A → parallel arrays) + portable docker-compose volume paths
- [ ] 14-02-PLAN.md — Service file template (remove ergon paths) + SIGTERM exit code test

### Phase 15: Mac Boot Persistence
**Goal**: Users on macOS can wire up Aletheia as a login-persistent service with the same two commands that Linux users have always had
**Depends on**: Phase 14
**Requirements**: LAUNCHD-01, LAUNCHD-02, LAUNCHD-03, LAUNCHD-04
**Success Criteria** (what must be TRUE):
  1. User can run `aletheia enable` on macOS and confirm that Aletheia gateway and memory services start automatically after a reboot (launchd plists installed to `~/Library/LaunchAgents/`, `node` path captured at install time so launchd can find it)
  2. User can run `aletheia disable` on macOS and confirm that Aletheia no longer starts at login (plists unloaded via `launchctl bootout` and removed from `~/Library/LaunchAgents/`)
  3. Launchd plist templates exist in `config/services/` with placeholder tokens that `aletheia enable` substitutes with real paths at install time (gateway plist + memory plist, `KeepAlive.SuccessfulExit: false` not `KeepAlive: true`)
  4. User can start memory services independently with `aletheia start --memory-only` (docker-compose launched without gateway — used by the memory launchd plist)
**Plans**: TBD

### Phase 16: Doctor, CLI Wizard, and Bug Fix
**Goal**: `aletheia doctor` gives a complete picture of stack health, `aletheia init` matches the web wizard experience, and Dianoia planning sessions complete without stream timeouts
**Depends on**: Phase 14 (SIGTERM fix needed for clean connectivity check behavior; independent of Phase 15)
**Requirements**: DOCTOR-01, DOCTOR-02, DOCTOR-03, INIT-01, INIT-02, BUG-01
**Success Criteria** (what must be TRUE):
  1. User can run `aletheia doctor` and see pass/fail HTTP connectivity status for gateway, Qdrant, Neo4j, and mem0 within 3 seconds (async checks with 3-second timeout — no hanging on unreachable services)
  2. User can run `aletheia doctor` and see dependency health (Node 22+, Docker/Podman presence, build artifact existence) alongside connectivity results
  3. User can run `aletheia doctor` and see whether boot persistence is configured for their platform (launchd enabled/disabled on Mac, systemd enabled/disabled on Linux)
  4. User can complete `aletheia init` without manually locating their API key (auto-detected from `~/.claude.json` — same source as web wizard)
  5. User can set their profile during `aletheia init` (name/role/style step present — matches web wizard, not skipped in CLI path)
  6. User can complete a Dianoia planning session with long sub-agent dispatches without hitting a stream timeout error (root cause of #208 investigated and fixed)
**Plans**: TBD

### Phase 17: Setup Script Mac Support and Documentation
**Goal**: A new user on macOS can run `setup.sh`, follow the terminal output, and arrive at a working Aletheia installation — and then find accurate written documentation for every step
**Depends on**: Phase 14, Phase 15
**Requirements**: SETUP-01, SETUP-02, SETUP-03, SETUP-04, DOCS-01, DOCS-02
**Success Criteria** (what must be TRUE):
  1. User can run `setup.sh` on macOS without shell errors (OS detection present, port conflict detection uses portable `nc -z` or portable `lsof` flags — not Linux-specific syntax)
  2. User on macOS sees Homebrew prerequisite guidance during `setup.sh` (check for Homebrew, native Qdrant binary and `brew install neo4j` paths offered as alternatives to Docker)
  3. User is offered boot persistence at the end of `setup.sh` and can opt in with a single keypress (optional `aletheia enable` call — not forced, not skipped silently)
  4. User sees a clear end-banner after `setup.sh` completes telling them how to start Aletheia next time (`aletheia start` — not another `setup.sh` run)
  5. User can follow DEPLOYMENT.md for a complete Mac deployment (launchd plist content, `bootstrap`/`bootout` commands, log location, Homebrew prerequisites, `aletheia enable`/`disable` reference — all describing working behavior)
  6. User can trust every feature described in QUICKSTART.md exists and works (no aspirational or forward-looking claims — all described behavior verified against implementation)
**Plans**: TBD

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 10. Standards Definition + Agent Context Infrastructure | v1.1 | 2/2 | Complete | 2026-02-25 |
| 11. Tooling Configuration + Pre-commit Coverage | v1.1 | 5/5 | Complete | 2026-02-25 |
| 12. Codebase Audit Against New Standards | v1.1 | 3/3 | Complete | 2026-02-25 |
| 13. Violation Remediation | v1.1 | 14/13 | Complete | 2026-02-26 |
| 14. Portability Fixes | 2/2 | Complete    | 2026-02-26 | - |
| 15. Mac Boot Persistence | v1.2 | 0/TBD | Not started | - |
| 16. Doctor, CLI Wizard, and Bug Fix | v1.2 | 0/TBD | Not started | - |
| 17. Setup Script Mac Support and Documentation | v1.2 | 0/TBD | Not started | - |
