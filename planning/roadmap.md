# Aletheia: implementation roadmap

Tracks the prompt-driven implementation plan. Each Wave is a batch of PRs shipped together.

**Status as of 2026-03-12:** Waves 1–9 complete. P019 (cutover) on hold.

---

## Summary

| Waves | PRs | Status |
|-------|-----|--------|
| Wave 1–5 | #693–#731 | Done |
| Wave 6 | #732–#737 | Done |
| Wave 7 | #738–#742 | Done |
| Wave 8 | #743–#752 | Done |
| Wave 9 | #753–#756 | Done |

---

## Wave 1–5 (#693–#731)

Foundation, memory engine, agent pipeline, gateway, channels, embedding switch (fastembed → candle, #693).

All done.

---

## Wave 6 (#732–#737)

| PR | Title |
|----|-------|
| #732 | Runtime snafu: error enum migration across runtime crates |
| #733 | TUI error recovery: graceful degradation on API errors |
| #734 | Core pipeline safety: cancel-safe select! branches |
| #735 | Deployment flow: systemd unit, NixOS module skeleton |
| #736 | Explicit forgetting: knowledge deletion API |
| #737 | Cross-crate integration tests: pylon + mneme + nous end-to-end |

---

## Wave 7 (#738–#742)

| PR | Title |
|----|-------|
| #738 | Daemon task backoff + health checks |
| #739 | Actor crash recovery: NousActor supervisor restart |
| #740 | Skill quality lifecycle: promotion, demotion, expiry |
| #741 | HNSW bounded cache + panic elimination |
| #742 | lru dep bump: update to current release |

---

## Wave 8 (#743–#752)

| PR | Title |
|----|-------|
| #743 | CHANGELOG generation: automated from conventional commits |
| #744 | Smoke test suite: binary-level integration checks |
| #749 | Engine facade consolidation: BoxErr → InternalError |
| #750 | Workspace hygiene: lint cleanup, unused deps |
| #751 | Deploy bug fixes: config cascade edge cases |
| #752 | Final validation: zero blanket suppressions. Engine error remediation COMPLETE. |

---

## Wave 9 (#753–#756)

| PR | Title |
|----|-------|
| #753 | Unsafe SAFETY docs: all 14 unsafe sites documented (#753 = P202 verification) |
| #754 | Pylon handler tests: unit tests for all HTTP endpoints |
| #755 | CLI modularization: main.rs 2245 → 172 lines, commands/ submodule |
| #756 | Monolith decomposition: 7 large files split into focused modules |

---

## In flight

None. All waves complete.

---

## Remaining

| Prompt | Title | Status |
|--------|-------|--------|
| P019 | Cutover validation | HELD: pending operator readiness |
