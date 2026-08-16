# Feature maturity matrix

What is safe to build on, what may still change, and what is internal —
across crates, routes, providers, and app surfaces (aletheia#4537).

## Maturity states

- **Stable** — intended public surface with compatibility expectations.
- **Experimental** — usable but may change; feature-gated or clearly marked.
- **Internal** — implementation detail, not a public contract.
- **Planned** — design exists but not implemented or not wired.
- **Deprecated** — retained temporarily with migration guidance.
- **Removed/Superseded** — historical reference only.
- **Undeclared** — no maturity metadata exists yet for this surface. Treat
  as unclassified, not as an implicit `Stable`.

## Crates

Generated from each crate's own `[package.metadata.kanon]` block in its
`Cargo.toml` — see `scripts/generate-maturity-doc.py`'s module doc for why
that is the canonical source rather than a hand-maintained list here. A
crate declares its own maturity by adding that block; there is no separate
place to register it.

<!-- BEGIN GENERATED CRATE MATURITY -- run `python3 scripts/generate-maturity-doc.py` to refresh, do not hand-edit -->

| Crate | Path | Maturity | Since | Exit criteria |
|---|---|---|---|---|
| `agora` | `crates/agora` | Undeclared | — | — |
| `aletheia` | `crates/aletheia` | Undeclared | — | — |
| `aletheia-classify` | `crates/aletheia-classify` | Undeclared | — | — |
| `aletheia-lexica` | `crates/aletheia-lexica` | Undeclared | — | — |
| `aletheia-memory-mcp` | `crates/aletheia-memory-mcp` | Undeclared | — | — |
| `aletheia-routing` | `crates/aletheia-routing` | Undeclared | — | — |
| `aletheia-sessions-migrate` | `crates/aletheia-sessions-migrate` | Undeclared | — | — |
| `dianoia` | `crates/dianoia` | Undeclared | — | — |
| `diaporeia` | `crates/diaporeia` | Undeclared | — | — |
| `dokimion` | `crates/eval` | Undeclared | — | — |
| `eidos` | `crates/eidos` | Undeclared | — | — |
| `energeia` | `crates/energeia` | Undeclared | — | — |
| `episteme` | `crates/episteme` | Undeclared | — | — |
| `gnosis` | `crates/gnosis` | Undeclared | — | — |
| `graphe` | `crates/graphe` | Undeclared | — | — |
| `hermeneus` | `crates/hermeneus` | Undeclared | — | — |
| `integration-tests` | `crates/integration-tests` | Undeclared | — | — |
| `koilon` | `crates/theatron/koilon` | Undeclared | — | — |
| `koina` | `crates/koina` | Undeclared | — | — |
| `krites` | `crates/krites` | Experimental | 2026-06-23 | public API is stable |
| `melete` | `crates/melete` | Undeclared | — | — |
| `mneme` | `crates/mneme` | Stable | 2025-06-01 | stable facade; exit when sub-crate decomposition is revisited or facade is retired |
| `nous` | `crates/nous` | Undeclared | — | — |
| `oikonomos` | `crates/daemon` | Undeclared | — | — |
| `organon` | `crates/organon` | Stable | 2024-01-01 | decommission with the aletheia tool execution layer |
| `poiesis` | `crates/poiesis` | Undeclared | — | — |
| `poiesis-charts` | `crates/poiesis/charts` | Undeclared | — | — |
| `poiesis-core` | `crates/poiesis/core` | Undeclared | — | — |
| `poiesis-deck` | `crates/poiesis/deck` | Undeclared | — | — |
| `poiesis-diff` | `crates/poiesis/diff` | Experimental | 2026-06-23 | diff API is stable and integrated into a release workflow |
| `poiesis-doc` | `crates/poiesis/doc` | Experimental | 2026-06-23 | public API is stable |
| `poiesis-inspect` | `crates/poiesis/inspect` | Experimental | 2026-06-23 | inspection API is stable and covers all supported formats |
| `poiesis-intake` | `crates/poiesis/intake` | Undeclared | — | — |
| `poiesis-lint` | `crates/poiesis/lint` | Undeclared | — | — |
| `poiesis-ooxml-parse` | `crates/poiesis/ooxml-parse` | Experimental | 2026-06-23 | OOXML parsing API is stable and consumed by at least two released crates |
| `poiesis-printer-chromium` | `crates/poiesis/printer-chromium` | Undeclared | — | — |
| `poiesis-scaffold` | `crates/poiesis/scaffold` | Undeclared | — | — |
| `poiesis-sheet` | `crates/poiesis/sheet` | Undeclared | — | — |
| `poiesis-slides` | `crates/poiesis/slides` | Undeclared | — | — |
| `poiesis-text` | `crates/poiesis/text` | Undeclared | — | — |
| `poiesis-theme` | `crates/poiesis/theme` | Undeclared | — | — |
| `poiesis-typst` | `crates/poiesis/typst` | Undeclared | — | — |
| `poiesis-verify` | `crates/poiesis/verify` | Undeclared | — | — |
| `pylon` | `crates/pylon` | Stable | 2024-01-01 | decommission with the aletheia HTTP gateway |
| `skene` | `crates/theatron/skene` | Experimental | 2026-06-23 | public API stabilized with full newtype coverage for all domain identifiers |
| `symbolon` | `crates/symbolon` | Undeclared | — | — |
| `taxis` | `crates/taxis` | Undeclared | — | — |
| `thesauros` | `crates/thesauros` | Undeclared | — | — |

9 of 48 crates declare `[package.metadata.kanon]` maturity metadata. The rest render `Undeclared`, not an implicit `Stable` -- declare maturity in the crate's own `Cargo.toml` to close that gap for one crate at a time.

<!-- END GENERATED CRATE MATURITY -->

## Known gaps

This matrix currently covers workspace crates only. Per aletheia#4537's full
scope, not yet covered here:

- **HTTP routes** — `crates/pylon/src/openapi.rs` is the generated route
  inventory; no per-route maturity label exists yet.
- **Provider integrations** — `crates/hermeneus/src` implements each
  provider; no per-provider maturity label exists yet.
- **TUI/desktop surfaces** — `docs/GOLDEN-PATH.md` classifies its own
  numbered steps Implemented/Experimental/Planned, which is the closest
  existing signal until a per-surface classification lands.
- **Observability/ops tooling** — `docs/OBSERVABILITY-AUDIT.md` inventories
  metrics/alerting/health-check surfaces without a maturity label per row.

Each is a separately-scoped follow-up, not blocked on this crate table.

## Regenerating

```bash
python3 scripts/generate-maturity-doc.py         # rewrite the crate table
python3 scripts/generate-maturity-doc.py --check # verify it matches Cargo.toml (CI gate)
```
