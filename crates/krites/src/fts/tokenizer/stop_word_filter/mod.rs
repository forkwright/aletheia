//! Stop word removal filter with multi-language support.
//!
//! Two implementations coexist for the krites retirement's land-dark → soak →
//! delete cycle (`PLAN.md` §2): [`derived`] is the CozoDB-derived filter
//! (`PROVENANCE.toml` status `dual`, soaking) and stays the default; `sovereign`
//! is the freshly authored replacement, selected by `--cfg
//! krites_sovereign_stop_word_filter`. Both vendor the identical stopword word
//! lists (verified token-multiset identical, 21,707 literals across 58
//! languages) — the sovereign implementation corrects who the data is actually
//! attributed to (see `sovereign/NOTICE.md`) without re-sourcing it.

// WHY: `krites_sovereign_stop_word_filter` is a raw --cfg, not a Cargo
// feature — a feature would need a `[features]` entry in the crate-root
// Cargo.toml, a conductor-only, train-only hotspot shared by every wave's
// land-dark cfg (dispatch/PROCESS.md's per-wave ownership prefixes exclude
// the crate root). rustc has no way to know this flag exists without either
// a `--check-cfg` (also Cargo.toml-rooted, same problem) or this file-local
// allow.
#![allow(unexpected_cfgs)]

#[cfg(not(krites_sovereign_stop_word_filter))]
mod derived;
#[cfg(not(krites_sovereign_stop_word_filter))]
pub(crate) use derived::StopWordFilter;

#[cfg(krites_sovereign_stop_word_filter)]
mod sovereign;
#[cfg(krites_sovereign_stop_word_filter)]
pub(crate) use sovereign::StopWordFilter;
