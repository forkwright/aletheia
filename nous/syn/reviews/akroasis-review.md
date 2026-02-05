# Akroasis Project Review

**Repository:** [forkwright/akroasis](https://github.com/forkwright/akroasis)  
**Reviewed:** 2026-01-28  
**License:** GPL-3.0

---

## 1. What the Project Does

**Akroasis** (Greek: Ἀκρόασις - "a hearing") is a unified media player for:
- **Music** (Primary) — Bit-perfect playback, gapless, hi-res audio, EQ, ReplayGain
- **Audiobooks** — Chapter navigation, position tracking, sleep timer, speed control
- **Podcasts** — Episode management, position sync, chapter markers
- **Ebooks** (Secondary) — EPUB support, position sync, basic annotations

### Core Philosophy
- **Bit-perfect audio**: High-res playback (24/96, 24/192, DSD), exclusive mode, gapless
- **Self-hosted first**: Connects to [Mouseion](https://github.com/forkwright/mouseion) backend (C# .NET 8.0)
- **Privacy-first**: Zero telemetry, no tracking, local-first data storage
- **Sony Walkman optimized**: Priority platform for portable audiophile playback

### Platform Support
| Platform | Stack | Status |
|----------|-------|--------|
| **Android** | Kotlin + Jetpack Compose + Rust core (JNI) | Primary, feature-rich |
| **Web/Desktop** | Tauri 2 + React 19 + TypeScript | MVP complete |
| **Audio Core** | Rust (claxon FLAC, gapless buffers, ReplayGain) | Shared via JNI/FFI |

### Key Features Implemented
- 5-band parametric EQ with AutoEQ profiles (HD600, HD650, DT770 Pro, ATH-M50x)
- Crossfeed engine (Low/Medium/High presets)
- Last.fm + ListenBrainz scrobbling
- Signal path visualization
- Queue management with 50-state undo/redo
- Media session integration (notification/lock screen controls)
- Voice search (Google Assistant / Android Auto)
- PWA with offline support (Web)

---

## 2. Current State & Issues

### Development Status: **Active, Production-Ready Path**

**Completed Phases:** 0, 1, 2, 3, 5, 6, 7  
**Test Coverage:** 473+ tests, 80%+ instruction coverage (Jacoco enforced)

### Recent Activity (Last 10 PRs)
- #136: Progress tracking and session management
- #130-134: SonarCloud cognitive complexity refactoring
- #128: Voice search integration in PlaybackService
- #126: Massive test suite expansion (110 → 473 tests)
- #124-125: Phase 5 Ebook Reader + GitHub Releases workflow

### Open Issues (4 total)
| # | Title | Type |
|---|-------|------|
| #138 | Listening history and continue feed polish | Feature |
| #137 | Speed up PR workflow feedback | Performance |
| #119 | Comprehensive security audit | Security (deferred) |
| #114 | Integration tests for service/network layers | Testing |

### Technical Debt / Blockers
- **Web limitations accepted**: Browser resampling (not bit-perfect), format support varies
- **Security audit** (#119) deferred from Phase 0 — marked for future
- **Upsampling & Convolution DSP** deferred post-MVP

### Project Maturity
- **Solo project with AI pair programming** (all AI code reviewed before merge)
- Active commits (136+ PRs merged)
- Well-documented roadmap and changelog
- CI/CD fully operational (Android lint, Rust builds, Web tests, CodeQL, SonarCloud)

---

## 3. Coding Standards Used

### Git Workflow
- **Branches:** `main` (stable releases), `develop` (integration target), `feature/*`, `fix/*`
- **Merge strategy:** Squash merge to keep history clean
- **PR target:** Always `develop`, never direct to `main`

### Commit Convention
**Conventional Commits** required:
```
feat(scope): description
fix(scope): description
docs: description
refactor: description
perf: description
test: description
```

### PR Template
Required sections:
- Summary
- Type of Change (checkboxes)
- Changes (bullet list)
- Testing checklist (builds, tests pass, manual testing)
- Notes

### Issue Templates
- **Bug Report:** Environment, reproduction steps, expected/actual, logs/screenshots
- **Feature Request:** Use case, proposed solution, alternatives

### Code Style

**Kotlin (Android):**
- Kotlin style guide + ktlint
- Version catalog (`libs.versions.toml`) for dependency management
- Hilt for DI, Room for persistence, Retrofit + OkHttp for API
- Jetpack Compose for UI

**TypeScript/JavaScript (Web):**
- ESLint + Prettier
- React 19 + Vite + Zustand (state)
- Tailwind CSS for styling
- Vitest for testing

**Rust (Audio Core):**
- Standard Rust formatting (rustfmt)
- Clippy linting in CI

### CI/CD Pipeline
| Workflow | Purpose |
|----------|---------|
| `android-ci.yml` | Kotlin lint, APK build, Jacoco coverage |
| `web-ci.yml` | ESLint, Vite build, Vitest coverage |
| `rust.yml` | Cargo build, clippy, tests |
| `codeql.yml` | Security analysis |
| `security-scan.yml` | OWASP dependency check |
| `release.yml` | APK distribution via GitHub Releases |
| `pr-check.yml` | PR validation |

### Quality Gates
- **Coverage threshold:** 80% instruction, 75% branch (enforced on PRs)
- **SonarCloud:** Cognitive complexity actively refactored
- **Security:** Encrypted SharedPreferences for tokens, OWASP checks

### Documentation Standards
- Comprehensive ROADMAP.md with phase-by-phase breakdown
- Detailed CHANGELOG.md for both root and android/
- MANUAL_TESTING.md and TEST_COVERAGE.md for QA
- Inline code comments expected to be concise/technical

---

## Summary

Akroasis is a well-architected, actively developed audiophile media player with solid engineering practices. The solo developer uses AI assistance but maintains rigorous code review and testing standards. The project has completed most major phases and is approaching production readiness.

**Strengths:**
- Clear vision and scope (no feature creep)
- Excellent test coverage and CI enforcement
- Privacy-focused, self-hosted architecture
- Multi-platform with shared Rust audio core

**Watch Areas:**
- Security audit still pending
- Single maintainer (bus factor = 1)
- Web playback not bit-perfect (browser limitation)

---

*Review generated by subagent for main session*
