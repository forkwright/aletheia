# Aletheia C Footprint Audit

> As of v0.13.66 (April 2026). Run `cargo tree --prefix none | grep -i 'sys '` to verify.
>
> **Direct dependencies:** 25 workspace crates  
> **Total transitive dependencies:** ~703 packages

## C Dependencies

### libsqlite3-sys (SQLite3)

- **Chain:** rusqlite (bundled) → libsqlite3-sys → C:sqlite3
- **Why:** Core persistent knowledge graph storage (graphe crate)
- **Elimination:** None planned - acceptable core dependency
- **Status:** Essential

### inotify-sys (Linux inotify)

- **Chain:** notify → inotify-sys → C:libc.inotify
- **Why:** File change notifications for config hot-reload (daemon)
- **Elimination:** None planned - kernel API has no pure-Rust alternative
- **Status:** Essential

### onig_sys (Oniguruma regex)

- **Chain:** tokenizers (HuggingFace) → onig_sys → C:oniguruma
- **Why:** Token boundary detection in embedding pipeline (episteme)
- **Elimination:** Tied to HuggingFace tokenizers crate; out of scope unless HF drops dependency
- **Status:** Essential (transitive)

### dirs-sys (XDG directories)

- **Chain:** dirs → hf-hub → episteme → mneme
- **Why:** Platform-specific home/cache directories for model downloads
- **Elimination:** Low priority; could use `std::env` + platform-specific logic
- **Status:** Acceptable (minor, transitive)

## Pure-Rust Bindings (Not C)

### linux-raw-sys

- **Chain:** rustix → workspace
- **What:** Generated Rust bindings for Linux syscalls - no C code compiled
- **Status:** Not a C dependency; no action needed

## Eliminated

### aws-lc-sys

- **Status:** Not in dependency tree
- **How:** rustls pinned to `ring` backend via `default-features = false, features = ["ring"]`
- **Verified:** `cargo tree --prefix none | grep aws` returns empty

### openssl-sys

- **Status:** Not in dependency tree
- **How:** All TLS via rustls; no OpenSSL anywhere in the stack

## Active Migrations

### ring → RustCrypto (#2288)

- **ring** enters via:
1. symbolon (JWT signing: `ring::hmac`, encryption: `ring::aead`)
2. taxis (config encryption: `ring::aead`)
3. hermeneus (SHA256 fingerprint: `ring::digest`)
4. rustls (TLS - cannot migrate until rustls-rustcrypto stabilizes)

Migration replaces ring with `hmac`, `sha2`, `aes-gcm`, `chacha20poly1305` crates. ring remains as transitive via rustls.

### chrono → jiff (#2287)

chrono enters via:
1. daemon (cron crate requires `chrono::Utc` for schedule iteration)
2. rmcp (transitive via schemars - not controllable)

Migrate by replacing the `cron` crate with a jiff-native alternative.

## Rand Version Reconciliation

**Status:** Three versions of rand coexist in the dependency tree. The workspace uses `rand = "0.9"` as the primary version.

| Version | Dependents | Upgrade Path |
|---------|------------|--------------|
| **rand@0.9.2** | aletheia, episteme, hermeneus, koina, krites, pylon, symbolon, taxis, candle-core, candle-transformers, hf-hub, proptest, tokenizers, twox-hash, rand_distr, float8, half | ✓ Current workspace standard |
| **rand@0.10.0** | rmcp@1.3.0 | Blocked - rmcp requires 0.10; wait for ecosystem alignment or rmcp update |
| **rand@0.8.5** | phf_generator@0.11.3 (transitive via multiple crates) | Indirect - requires upstream updates to phf_generator |

**Impact:** Low - rand is a well-maintained crate with no known security issues across these versions. The duplication adds minor compile-time overhead but no runtime duplication (different major versions are distinct crates).

**Recommendation:** Monitor rmcp for rand 0.9 compatibility. No action needed for rand 0.8.5 (phf_generator is a build-time dependency for perfect hash functions).

---

*Last audit: 2026-04-08*
