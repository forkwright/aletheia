# Changelog

## [0.13.15](https://github.com/forkwright/aletheia/compare/v0.13.14...v0.13.15) (2026-03-26)


### Documentation

* expand v0.13.14 changelog to reflect full audit scope ([e2139f1](https://github.com/forkwright/aletheia/commit/e2139f183f4220131b64a24ca4a06ee08a6c5914))

## [0.13.14](https://github.com/forkwright/aletheia/compare/v0.13.13...v0.13.14) (2026-03-26)

Full QA audit of v0.13.13 followed by systematic fix campaign. 119 issues resolved across 20+ crates. Zero kanon lint violations. ([#2225](https://github.com/forkwright/aletheia/issues/2225))

### Features

* **pylon:** RBAC enforcement with configurable `auth.none_role` ([#2142](https://github.com/forkwright/aletheia/issues/2142), [#2201](https://github.com/forkwright/aletheia/issues/2201), [#2199](https://github.com/forkwright/aletheia/issues/2199))
* **graphe,symbolon:** encryption at rest for sessions (ChaCha20-Poly1305) and credentials (AES-256-GCM) ([#2143](https://github.com/forkwright/aletheia/issues/2143), [#2144](https://github.com/forkwright/aletheia/issues/2144))
* **symbolon:** OS keyring integration behind `keyring` feature flag ([#2213](https://github.com/forkwright/aletheia/issues/2213))
* **nous:** degraded mode auto-recovery (10 min) + manual `POST /nous/{id}/recover` endpoint ([#2153](https://github.com/forkwright/aletheia/issues/2153))
* **aletheia:** RuntimeBuilder collapses duplicated startup into single path with production/minimal/validation presets ([#2224](https://github.com/forkwright/aletheia/issues/2224))
* **docs:** feature flag reference, desktop build guide, CONFIGURATION.md TOC completion ([#2141](https://github.com/forkwright/aletheia/issues/2141), [#2140](https://github.com/forkwright/aletheia/issues/2140), [#2139](https://github.com/forkwright/aletheia/issues/2139))

### Bug Fixes

* **pylon:** idempotency cache mutex poison recovery, search validation, user-scoped cache keys, message acknowledgment ([#2117](https://github.com/forkwright/aletheia/issues/2117), [#2119](https://github.com/forkwright/aletheia/issues/2119), [#2200](https://github.com/forkwright/aletheia/issues/2200), [#2113](https://github.com/forkwright/aletheia/issues/2113))
* **pylon:** rate limiter validates JWT before keying, verification endpoints return 501 ([#2223](https://github.com/forkwright/aletheia/issues/2223), [#2222](https://github.com/forkwright/aletheia/issues/2222))
* **hermeneus:** backoff overflow, version header validation, HTTPS enforcement, retryable classification, SSE spec compliance, cache token costing, configurable streaming timeout, idempotency keys ([#2194](https://github.com/forkwright/aletheia/issues/2194), [#2195](https://github.com/forkwright/aletheia/issues/2195), [#2178](https://github.com/forkwright/aletheia/issues/2178), [#2136](https://github.com/forkwright/aletheia/issues/2136), [#2192](https://github.com/forkwright/aletheia/issues/2192), [#2182](https://github.com/forkwright/aletheia/issues/2182), [#2181](https://github.com/forkwright/aletheia/issues/2181), [#2179](https://github.com/forkwright/aletheia/issues/2179))
* **nous:** tool iteration off-by-one, loop detection blind spot, distillation flag reset, session eviction, streaming channel cleanup, token arithmetic overflow, unreachable panic, silent fallback, inbox timeout, session persistence before spawn ([#2135](https://github.com/forkwright/aletheia/issues/2135), [#2137](https://github.com/forkwright/aletheia/issues/2137), [#2155](https://github.com/forkwright/aletheia/issues/2155), [#2154](https://github.com/forkwright/aletheia/issues/2154), [#2157](https://github.com/forkwright/aletheia/issues/2157), [#2193](https://github.com/forkwright/aletheia/issues/2193), [#2156](https://github.com/forkwright/aletheia/issues/2156), [#2161](https://github.com/forkwright/aletheia/issues/2161), [#2159](https://github.com/forkwright/aletheia/issues/2159), [#2160](https://github.com/forkwright/aletheia/issues/2160))
* **organon:** subprocess timeout (60s), pattern length limit (ReDoS), null-byte rejection, symlink validation, protected files expansion, TOCTOU canonicalize, path sanitization ([#2168](https://github.com/forkwright/aletheia/issues/2168), [#2133](https://github.com/forkwright/aletheia/issues/2133), [#2167](https://github.com/forkwright/aletheia/issues/2167), [#2163](https://github.com/forkwright/aletheia/issues/2163), [#2169](https://github.com/forkwright/aletheia/issues/2169), [#2165](https://github.com/forkwright/aletheia/issues/2165), [#2162](https://github.com/forkwright/aletheia/issues/2162), [#2166](https://github.com/forkwright/aletheia/issues/2166), [#2164](https://github.com/forkwright/aletheia/issues/2164))
* **graphe:** WAL checkpoint retry, schema version reconciliation, orphan cascade, blackboard TTL cleanup, recovery reporting, backup path traversal, missing index ([#2189](https://github.com/forkwright/aletheia/issues/2189), [#2188](https://github.com/forkwright/aletheia/issues/2188), [#2185](https://github.com/forkwright/aletheia/issues/2185), [#2186](https://github.com/forkwright/aletheia/issues/2186), [#2184](https://github.com/forkwright/aletheia/issues/2184), [#2190](https://github.com/forkwright/aletheia/issues/2190), [#2187](https://github.com/forkwright/aletheia/issues/2187))
* **episteme:** embedding dimension validation, HNSW lock recovery, atomic access counter, search overfetch for forgotten-fact filtering, RRF normalization, similarity clamp warning, HNSW capacity enforcement, embedding checksums ([#2134](https://github.com/forkwright/aletheia/issues/2134), [#2171](https://github.com/forkwright/aletheia/issues/2171), [#2170](https://github.com/forkwright/aletheia/issues/2170), [#2172](https://github.com/forkwright/aletheia/issues/2172), [#2175](https://github.com/forkwright/aletheia/issues/2175), [#2196](https://github.com/forkwright/aletheia/issues/2196), [#2173](https://github.com/forkwright/aletheia/issues/2173), [#2174](https://github.com/forkwright/aletheia/issues/2174))
* **krites:** HNSW cache LRU correctness (`peek` to `get`), deserialization panic removal (DecodeError enum), vector cache byte-budget tracking, mmap remap fd validation, fjall explicit fsync, as-cast and indexing safety, expect-to-error propagation across 66 files ([#2203](https://github.com/forkwright/aletheia/issues/2203), [#2204](https://github.com/forkwright/aletheia/issues/2204), [#2208](https://github.com/forkwright/aletheia/issues/2208), [#2220](https://github.com/forkwright/aletheia/issues/2220), [#2207](https://github.com/forkwright/aletheia/issues/2207))
* **taxis:** config fail-fast on load error, required field validation, encrypted field detection, sandbox validation warning ([#2114](https://github.com/forkwright/aletheia/issues/2114), [#2121](https://github.com/forkwright/aletheia/issues/2121), [#2120](https://github.com/forkwright/aletheia/issues/2120), [#2126](https://github.com/forkwright/aletheia/issues/2126))
* **symbolon:** credential write race condition, env token decode warning, refresh token zeroization ([#2118](https://github.com/forkwright/aletheia/issues/2118), [#2125](https://github.com/forkwright/aletheia/issues/2125), [#2209](https://github.com/forkwright/aletheia/issues/2209))
* **aletheia:** rustls provider panic, configurable shutdown timeout, credential error context, token preview reduced to 7 chars ([#2217](https://github.com/forkwright/aletheia/issues/2217), [#2218](https://github.com/forkwright/aletheia/issues/2218), [#2128](https://github.com/forkwright/aletheia/issues/2128), [#2122](https://github.com/forkwright/aletheia/issues/2122))
* **security:** 8 config/credential writes hardened with 0600 perms via `koina::fs::write_restricted` ([#2150](https://github.com/forkwright/aletheia/issues/2150))
* **init:** success message, --api-key wiring, env var alignment, context_tokens comment, credential source help, health URL from config ([#2108](https://github.com/forkwright/aletheia/issues/2108), [#2109](https://github.com/forkwright/aletheia/issues/2109), [#2110](https://github.com/forkwright/aletheia/issues/2110), [#2111](https://github.com/forkwright/aletheia/issues/2111), [#2112](https://github.com/forkwright/aletheia/issues/2112), [#2116](https://github.com/forkwright/aletheia/issues/2116), [#2115](https://github.com/forkwright/aletheia/issues/2115))
* **deploy:** POSIX portability (stat, find), fork detection, health check strictness, rollback verification, token refresh path ([#2124](https://github.com/forkwright/aletheia/issues/2124), [#2130](https://github.com/forkwright/aletheia/issues/2130), [#2129](https://github.com/forkwright/aletheia/issues/2129), [#2131](https://github.com/forkwright/aletheia/issues/2131), [#2123](https://github.com/forkwright/aletheia/issues/2123), [#2132](https://github.com/forkwright/aletheia/issues/2132))
* **daemon:** shutdown task cancellation, cron state persistence before execution ([#2211](https://github.com/forkwright/aletheia/issues/2211), [#2212](https://github.com/forkwright/aletheia/issues/2212))
* **melete:** panic boundary around distillation LLM call ([#2216](https://github.com/forkwright/aletheia/issues/2216))
* **theatron:** state epoch sequencing to prevent stale UI from concurrent SSE updates ([#2214](https://github.com/forkwright/aletheia/issues/2214))
* **agora:** bounded task spawning in message listener ([#2215](https://github.com/forkwright/aletheia/issues/2215))
* **dianoia:** markdown escaping in handoff context IDs ([#2221](https://github.com/forkwright/aletheia/issues/2221))
* **eval:** scenario timeout cancels spawned task ([#2219](https://github.com/forkwright/aletheia/issues/2219))
* **taxis:** binding channel type validation ([#2210](https://github.com/forkwright/aletheia/issues/2210))

### Documentation

* version numbers updated to v0.13.13, CSRF default corrected, crate count fixed, bulk import endpoint added, TECHNOLOGY.md encryption claim corrected, auth mode behavior documented ([#2197](https://github.com/forkwright/aletheia/issues/2197), [#2176](https://github.com/forkwright/aletheia/issues/2176), [#2198](https://github.com/forkwright/aletheia/issues/2198), [#2147](https://github.com/forkwright/aletheia/issues/2147), [#2145](https://github.com/forkwright/aletheia/issues/2145), [#2127](https://github.com/forkwright/aletheia/issues/2127))

### Performance

* **graphe:** index on `sessions.updated_at` for retention queries ([#2187](https://github.com/forkwright/aletheia/issues/2187))

### Code Quality

* zero kanon lint violations (3,734 to 0)
* 450 `pub` to `pub(crate)` visibility tightenings
* 108 `#[must_use]` annotations on Result-returning functions
* 28 import reorderings and format arg inlinings
* `clippy.toml` added to workspace root
* deprecated terms replaced (master, sanity check)
* em-dashes removed from CLAUDE.md files

## [0.13.13](https://github.com/forkwright/aletheia/compare/v0.13.12...v0.13.13) (2026-03-25)


### Bug Fixes

* **aletheia:** set 0600 permissions on config and export writes ([#2106](https://github.com/forkwright/aletheia/issues/2106)) ([6c1c7de](https://github.com/forkwright/aletheia/commit/6c1c7dea043b3224bd4e70af225fb4e3c0b8a63f))


### Documentation

* **crates:** fix 3 module path inaccuracies in per-crate CLAUDE.md ([#2104](https://github.com/forkwright/aletheia/issues/2104)) ([10ed5a3](https://github.com/forkwright/aletheia/commit/10ed5a33da00d7c3ccf8e39c4f4e8ef6af87f5f6))

## [0.13.12](https://github.com/forkwright/aletheia/compare/v0.13.11...v0.13.12) (2026-03-25)


### Bug Fixes

* **fuzz:** repair broken fuzz targets and add weekly CI workflow ([#2099](https://github.com/forkwright/aletheia/issues/2099)) ([41dbc97](https://github.com/forkwright/aletheia/commit/41dbc97f0ae42cbd063d9f4ac75c96b1fa594511))
* **fuzz:** replace indexing/slicing and bare assert in fuzz targets ([#2097](https://github.com/forkwright/aletheia/issues/2097)) ([3e8acfa](https://github.com/forkwright/aletheia/commit/3e8acfa1665f7849a06df41c868c2310d005241f))


### Documentation

* **organon:** fix inconsistent built-in tool count across docs ([#2094](https://github.com/forkwright/aletheia/issues/2094)) ([3c022ca](https://github.com/forkwright/aletheia/commit/3c022ca852d8e5c29886d235f8ac4562bb8bdb31))
* **theatron:** add umbrella CLAUDE.md for presentation crate group ([#2100](https://github.com/forkwright/aletheia/issues/2100)) ([7f4b979](https://github.com/forkwright/aletheia/commit/7f4b9793766754b9690f238a1557c753f035b6b8))
* update hardcoded install version from v0.13.1 to v0.13.11 ([#2096](https://github.com/forkwright/aletheia/issues/2096)) ([7725345](https://github.com/forkwright/aletheia/commit/77253455445695e024f6322b06d0a760a8b91e84))

## [0.13.11](https://github.com/forkwright/aletheia/compare/v0.13.10...v0.13.11) (2026-03-24)


### Documentation

* replace em-dash characters with spaced hyphens ([#2092](https://github.com/forkwright/aletheia/issues/2092)) ([56e4c5f](https://github.com/forkwright/aletheia/commit/56e4c5f1d510c515a9e90104d49cc104124c4d4f))

## [0.13.10](https://github.com/forkwright/aletheia/compare/v0.13.9...v0.13.10) (2026-03-24)


### Documentation

* **general:** fix performative language in voice-interaction research doc ([#2090](https://github.com/forkwright/aletheia/issues/2090)) ([2672ed3](https://github.com/forkwright/aletheia/commit/2672ed338c95aef0b11f4faa0783993929a6de24)), closes [#2076](https://github.com/forkwright/aletheia/issues/2076)

## [0.13.9](https://github.com/forkwright/aletheia/compare/v0.13.8...v0.13.9) (2026-03-24)


### Bug Fixes

* **scripts:** replace hardcoded /tmp path with XDG_STATE_HOME in health-monitor.sh ([#2088](https://github.com/forkwright/aletheia/issues/2088)) ([502e8c2](https://github.com/forkwright/aletheia/commit/502e8c266ab1d14806f46cdd4587bdf5fd63a9c7))
* **theatron-desktop:** replace direct indexing with safe accessors in charts ([#2064](https://github.com/forkwright/aletheia/issues/2064)) ([cb67438](https://github.com/forkwright/aletheia/commit/cb674381fb9719be41f34006371314242c91009a))


### Documentation

* **fuzz:** add .gitignore, README.md, CLAUDE.md, and clippy.toml ([#2087](https://github.com/forkwright/aletheia/issues/2087)) ([b6b28f2](https://github.com/forkwright/aletheia/commit/b6b28f2ceeaa231878548f38a456b1eff2421161))
* **prostheke:** replace minimizer word with precise language ([#2086](https://github.com/forkwright/aletheia/issues/2086)) ([6a13a50](https://github.com/forkwright/aletheia/commit/6a13a503a7b0923dc345516df4b9aeead28da143))

## [0.13.8](https://github.com/forkwright/aletheia/compare/v0.13.7...v0.13.8) (2026-03-24)


### Bug Fixes

* sync Cargo.lock with workspace version 0.13.7 ([#2062](https://github.com/forkwright/aletheia/issues/2062)) ([d8635da](https://github.com/forkwright/aletheia/commit/d8635dac3b18886173437f8826d2928fb9fed5bf))

## [0.13.7](https://github.com/forkwright/aletheia/compare/v0.13.6...v0.13.7) (2026-03-24)


### Bug Fixes

* **pylon:** convert sync-only planning tests from async to sync ([#2060](https://github.com/forkwright/aletheia/issues/2060)) ([8410e34](https://github.com/forkwright/aletheia/commit/8410e34c72df491ed722a737b48dd56fedcea5f8))

## [0.13.6](https://github.com/forkwright/aletheia/compare/v0.13.5...v0.13.6) (2026-03-24)


### Bug Fixes

* **theatron-desktop:** add 8 missing module declarations in views ([#2058](https://github.com/forkwright/aletheia/issues/2058)) ([bc27899](https://github.com/forkwright/aletheia/commit/bc2789944eecdcc0b46212942ac63427fdd5bdce))

## [0.13.5](https://github.com/forkwright/aletheia/compare/v0.13.4...v0.13.5) (2026-03-24)


### Bug Fixes

* remove duplicate module files and fix inner doc comments ([70eb84a](https://github.com/forkwright/aletheia/commit/70eb84ad1d9f0ab3d36364792ec009a82a0ddfcd))
* **security:** add explicit 0600 permissions to config/credential writes ([#2056](https://github.com/forkwright/aletheia/issues/2056)) ([5c4bf4d](https://github.com/forkwright/aletheia/commit/5c4bf4d6c42b3f5d878372e001744201c435fe60))
* **theatron:** instrument all tokio::spawn calls with tracing spans ([#2054](https://github.com/forkwright/aletheia/issues/2054)) ([c3d065a](https://github.com/forkwright/aletheia/commit/c3d065a568d8c15eff4e44fa3129b4f58d1434d4))

## [0.13.4](https://github.com/forkwright/aletheia/compare/v0.13.3...v0.13.4) (2026-03-24)


### Features

* **nous:** conditional workspace file loading based on task context ([#2049](https://github.com/forkwright/aletheia/issues/2049)) ([0e13075](https://github.com/forkwright/aletheia/commit/0e130757e7619d0f848ff0b45ef0c66c96b0b3f7))
* **pylon:** add POST /verification/refresh endpoint for re-verify button ([#2048](https://github.com/forkwright/aletheia/issues/2048)) ([989261a](https://github.com/forkwright/aletheia/commit/989261ae84570b04edbab99f04d812012c480c8a))
* **theatron:** wire SSE checkpoint events in CheckpointsView ([#2050](https://github.com/forkwright/aletheia/issues/2050)) ([eed8234](https://github.com/forkwright/aletheia/commit/eed82344cc4f1a34747fd5fe01076dbad1322907))


### Bug Fixes

* **episteme:** strengthen SAFETY justification for transmute in hnsw_index ([#2052](https://github.com/forkwright/aletheia/issues/2052)) ([a600b62](https://github.com/forkwright/aletheia/commit/a600b628624259569ec1bf54c845a3eac78f1ab1))
* **workspace:** resolve duplicate module paths from file split ([#2046](https://github.com/forkwright/aletheia/issues/2046)) ([6465a11](https://github.com/forkwright/aletheia/commit/6465a11a0c5961deb61a689b452c070c3bc53186))

## [0.13.3](https://github.com/forkwright/aletheia/compare/v0.13.2...v0.13.3) (2026-03-24)


### Bug Fixes

* **theatron-desktop:** add missing module declarations in state and components ([#2044](https://github.com/forkwright/aletheia/issues/2044)) ([6c9cc1c](https://github.com/forkwright/aletheia/commit/6c9cc1c342aedfbc119093a0db2663f665f6c526))

## [0.13.2](https://github.com/forkwright/aletheia/compare/v0.13.1...v0.13.2) (2026-03-24)


### Features

* add health monitoring, integration server test, and RUST_BACKTRACE ([6bca83b](https://github.com/forkwright/aletheia/commit/6bca83bf899a53688c776b85a3db130623f5115a))
* **aletheia,taxis:** add structured JSON file logging with daily rotation ([#1262](https://github.com/forkwright/aletheia/issues/1262)) ([e9fa219](https://github.com/forkwright/aletheia/commit/e9fa219c4d1666c6626cea8293dd12a6cadd5c4a))
* **cli:** add --non-interactive flag to aletheia init ([#1180](https://github.com/forkwright/aletheia/issues/1180)) ([d18a113](https://github.com/forkwright/aletheia/commit/d18a113181b2f2f88215237c62c20215fc34882b))
* **cli:** add session export subcommand ([#960](https://github.com/forkwright/aletheia/issues/960)) ([e6aeed6](https://github.com/forkwright/aletheia/commit/e6aeed63c7df7f09143d958a53eb4a07e0130e5c))
* **cli:** add-nous subcommand for nous scaffolding ([#964](https://github.com/forkwright/aletheia/issues/964)) ([8908716](https://github.com/forkwright/aletheia/commit/89087162696ef71da4810d96555f607924d6c611)), closes [#941](https://github.com/forkwright/aletheia/issues/941)
* **cli:** memory management subcommands — check, consolidate, sample, dedup, patterns ([#1940](https://github.com/forkwright/aletheia/issues/1940)) ([29dbc97](https://github.com/forkwright/aletheia/commit/29dbc97632cd75fa88370c4ad831d14bee7b66e5))
* **daemon:** watchdog process monitor with auto-recovery ([#1933](https://github.com/forkwright/aletheia/issues/1933)) ([947f51c](https://github.com/forkwright/aletheia/commit/947f51c4b626e70b6f667a0490917cb0e6f015e5))
* **deploy:** add backup, rollback, and health check ([577fad2](https://github.com/forkwright/aletheia/commit/577fad24952566eccf2136e001d7da81c013ab48))
* **dianoia:** multi-level parallel research ([#1950](https://github.com/forkwright/aletheia/issues/1950)) ([57e1f08](https://github.com/forkwright/aletheia/commit/57e1f08742c1952412aa69bf935e159b43554ea6)), closes [#1883](https://github.com/forkwright/aletheia/issues/1883)
* **dianoia:** state reconciler and verification workflow ([#1946](https://github.com/forkwright/aletheia/issues/1946)) ([51f361a](https://github.com/forkwright/aletheia/commit/51f361a756189cb97b02048b0b59654247e0302e))
* **dianoia:** stuck detection and handoff protocol ([#1926](https://github.com/forkwright/aletheia/issues/1926)) ([ac231a7](https://github.com/forkwright/aletheia/commit/ac231a79b5b2fcef08c2ddf3eb5302ea592b39eb)), closes [#1869](https://github.com/forkwright/aletheia/issues/1869) [#1870](https://github.com/forkwright/aletheia/issues/1870)
* **diaporeia:** add MCP server crate for external AI agent access ([#904](https://github.com/forkwright/aletheia/issues/904)) ([9970177](https://github.com/forkwright/aletheia/commit/99701770c6a966ee684fbbff483d566c08cff41b))
* **diaporeia:** add rate limiting to MCP bridge ([#1359](https://github.com/forkwright/aletheia/issues/1359)) ([87304ff](https://github.com/forkwright/aletheia/commit/87304ff15945f919d65a331e1b06bc7e6b44aaaa)), closes [#1316](https://github.com/forkwright/aletheia/issues/1316)
* **eval:** cognitive evaluation framework ([#1953](https://github.com/forkwright/aletheia/issues/1953)) ([1d267d6](https://github.com/forkwright/aletheia/commit/1d267d63984b09959d0645ed44160a3273a11abe)), closes [#1885](https://github.com/forkwright/aletheia/issues/1885)
* **hermeneus:** add model fallback chain for LLM requests ([01e43a7](https://github.com/forkwright/aletheia/commit/01e43a7bf2776b15abeffbdf437a8e8456f299bb))
* **hermeneus:** circuit breaker and adaptive concurrency limiter ([#1811](https://github.com/forkwright/aletheia/issues/1811)) ([3c7db5f](https://github.com/forkwright/aletheia/commit/3c7db5faa0ad1a8613c04ea172789bc03be3a7c3))
* **hermeneus:** complexity-based model routing ([#1928](https://github.com/forkwright/aletheia/issues/1928)) ([b73c672](https://github.com/forkwright/aletheia/commit/b73c6720f2bf11e9b2e8af19ff6adb55e30dd4c4)), closes [#1875](https://github.com/forkwright/aletheia/issues/1875)
* **hermeneus:** OpenAI-compatible local LLM provider ([#1846](https://github.com/forkwright/aletheia/issues/1846)) ([6a203eb](https://github.com/forkwright/aletheia/commit/6a203ebd50152aa9f58618ef810dcd9a297940c3))
* **init:** use Pronoea (Noe) as default agent for new instances ([#1672](https://github.com/forkwright/aletheia/issues/1672)) ([3d2dae1](https://github.com/forkwright/aletheia/commit/3d2dae18496574a28f9450319dc898682a3a3ddc))
* **koina,nous:** internal event system and OutputBuffer pattern ([#1813](https://github.com/forkwright/aletheia/issues/1813)) ([f2a5298](https://github.com/forkwright/aletheia/commit/f2a5298d6358c205eb6c9eeba7893034c63344e1))
* **koina:** add disk space monitoring with graceful degradation ([#1547](https://github.com/forkwright/aletheia/issues/1547)) ([404cb52](https://github.com/forkwright/aletheia/commit/404cb52600e24cbf26768311cfa2df10168c510b))
* **koina:** add missing AsRef/From/Borrow/Display impls for newtype IDs ([716e4ea](https://github.com/forkwright/aletheia/commit/716e4eade9c05966f68e126e87ac89081fa8b690))
* **koina:** add RedactingLayer for tracing field redaction ([1c7b172](https://github.com/forkwright/aletheia/commit/1c7b172034e3f61e94f67403ff5869a80e967dfe))
* **koina:** add SecretString newtype, apply to credential fields ([#1571](https://github.com/forkwright/aletheia/issues/1571)) ([b47d7e5](https://github.com/forkwright/aletheia/commit/b47d7e5b4ac97522e2d6e976093127e7e6256363))
* **koina:** trait abstraction layer for filesystem, clock, and environment ([#1803](https://github.com/forkwright/aletheia/issues/1803)) ([6709314](https://github.com/forkwright/aletheia/commit/6709314a28684bffe63236125c308651d9336d05))
* **melete:** similarity pruning and contradiction detection ([#1929](https://github.com/forkwright/aletheia/issues/1929)) ([f57428d](https://github.com/forkwright/aletheia/commit/f57428d241b10dd7dc0ec6953f0fec9b2076d197))
* **metrics:** add Prometheus metrics to 7 crates ([#1966](https://github.com/forkwright/aletheia/issues/1966)) ([5bb630c](https://github.com/forkwright/aletheia/commit/5bb630cf4b85d862594617ab091b412713cde3f5))
* **mneme:** add SQLite corruption recovery with read-only fallback ([#1548](https://github.com/forkwright/aletheia/issues/1548)) ([778e524](https://github.com/forkwright/aletheia/commit/778e524f41b44d9a13ff936bade9f52aca80d565))
* **mneme:** causal reasoning edges and post-merge lesson extraction ([#1814](https://github.com/forkwright/aletheia/issues/1814)) ([9c2fbaf](https://github.com/forkwright/aletheia/commit/9c2fbaf79054b5d1f48887a4e9bd35653d4f0f71))
* **mneme:** HNSW performance optimizations ([#1822](https://github.com/forkwright/aletheia/issues/1822)) ([7735927](https://github.com/forkwright/aletheia/commit/773592783562f0459b0f145ee46dcf7bae719bbd))
* **mneme:** SQL layer hardening — checksum verification, lifecycle hooks, query cache ([#1816](https://github.com/forkwright/aletheia/issues/1816)) ([652cf34](https://github.com/forkwright/aletheia/commit/652cf34995c60fdf37c0176ada98ec199c2b1d13))
* **mneme:** temporal decay algorithms and serendipity engine ([#1941](https://github.com/forkwright/aletheia/issues/1941)) ([88585a4](https://github.com/forkwright/aletheia/commit/88585a459e42f3cd9649ed3f2f6f896e06857e05))
* **nous,hermeneus,pylon,daemon,aletheia:** wire observability gaps (317) ([#914](https://github.com/forkwright/aletheia/issues/914)) ([0cdca3d](https://github.com/forkwright/aletheia/commit/0cdca3dfed4538d2451f842aed94d90edc2f6a01))
* **nous:** add cycle detection for mutual ask() deadlocks ([#1561](https://github.com/forkwright/aletheia/issues/1561)) ([c23b2ba](https://github.com/forkwright/aletheia/commit/c23b2bab5fae4e96449aaaab1b2e97db0bd713ca))
* **nous:** add Pronoea (Noe) as default agent for new instances ([#1658](https://github.com/forkwright/aletheia/issues/1658)) ([b5e3f95](https://github.com/forkwright/aletheia/commit/b5e3f950c82cbc902490fbaa961412deb47b6550))
* **nous:** competence tracking and uncertainty quantification ([#1938](https://github.com/forkwright/aletheia/issues/1938)) ([2aed0ae](https://github.com/forkwright/aletheia/commit/2aed0ae5d773032ff74f7440d6ab4951ce05b2a3))
* **nous:** expand default agent tool permissions ([#1355](https://github.com/forkwright/aletheia/issues/1355)) ([146f54f](https://github.com/forkwright/aletheia/commit/146f54f9dada72a85f18ab409bc06d5438cadaeb)), closes [#1311](https://github.com/forkwright/aletheia/issues/1311)
* **nous:** implement Chiron self-auditing loop via prosoche checks ([#1818](https://github.com/forkwright/aletheia/issues/1818)) ([31c6101](https://github.com/forkwright/aletheia/commit/31c610150beec0208c6fbd01f57a74bed2f05183))
* **nous:** implement tool result truncation limit ([#1545](https://github.com/forkwright/aletheia/issues/1545)) ([5caaffd](https://github.com/forkwright/aletheia/commit/5caaffd1a1f4ef890e161e5f80f1df93c6935e20))
* **nous:** pattern-based loop detection and working state management ([#1936](https://github.com/forkwright/aletheia/issues/1936)) ([ca7bd93](https://github.com/forkwright/aletheia/commit/ca7bd93be54491bd52b10277d5e2518ee35d7b9a)), closes [#1872](https://github.com/forkwright/aletheia/issues/1872) [#1881](https://github.com/forkwright/aletheia/issues/1881)
* **nous:** sub-agent role prompts — coder, researcher, reviewer, explorer, runner ([#1947](https://github.com/forkwright/aletheia/issues/1947)) ([7df6f8c](https://github.com/forkwright/aletheia/commit/7df6f8ca3590de85364140ca2f79d5bd4dc0581e))
* **nous:** use Haiku-tier model for prosoche heartbeat sessions ([#1544](https://github.com/forkwright/aletheia/issues/1544)) ([15fc93e](https://github.com/forkwright/aletheia/commit/15fc93edcdb948166b7feaa7971376450fe03159))
* **organon:** add egress filtering for agent exec tool ([#1565](https://github.com/forkwright/aletheia/issues/1565)) ([8da6365](https://github.com/forkwright/aletheia/commit/8da6365c7f5255f418659211d072ca9e74fb67fe))
* **organon:** agent self-prompted issue triage tools ([#1815](https://github.com/forkwright/aletheia/issues/1815)) ([251ef10](https://github.com/forkwright/aletheia/commit/251ef104c6014345e789e4d3076d6762e1acdb7d))
* **organon:** computer use tool with Landlock sandbox ([#1810](https://github.com/forkwright/aletheia/issues/1810)) ([a0d8a9f](https://github.com/forkwright/aletheia/commit/a0d8a9f67caa0538dca29c20e0fe0c4b4d31f5f3))
* **organon:** tool reversibility tracking and custom slash commands ([#1935](https://github.com/forkwright/aletheia/issues/1935)) ([8b7247f](https://github.com/forkwright/aletheia/commit/8b7247f75fbb3a2114c51f4a019cc9fbb9545dd2))
* **pylon:** add bulk fact import API endpoint ([#1996](https://github.com/forkwright/aletheia/issues/1996)) ([dba5545](https://github.com/forkwright/aletheia/commit/dba554584d373fd55c485a1fc24e68738ffe30ff))
* **pylon:** add per-user rate limiting with endpoint categories ([#1567](https://github.com/forkwright/aletheia/issues/1567)) ([bbb677d](https://github.com/forkwright/aletheia/commit/bbb677da968b7c1f659748c880990721ef111948))
* **pylon:** idempotency key support on send_message (P333) ([#936](https://github.com/forkwright/aletheia/issues/936)) ([f6e9c1f](https://github.com/forkwright/aletheia/commit/f6e9c1f28c59d890b7ed13899c324cbeee7b4bcb))
* startup config validation + --check-config CLI subcommand ([#898](https://github.com/forkwright/aletheia/issues/898)) ([02892e1](https://github.com/forkwright/aletheia/commit/02892e184b9461605b4dac7c45f5436d1a6b18f5))
* **symbolon:** add three-state circuit breaker for OAuth token refresh ([#1546](https://github.com/forkwright/aletheia/issues/1546)) ([83ae0d8](https://github.com/forkwright/aletheia/commit/83ae0d8bcfb44075c838ac54bd2c3d3c51ad91c0))
* **symbolon:** Claude Code OAuth credential provider (P331) ([0fce124](https://github.com/forkwright/aletheia/commit/0fce124fb26afd49019d34d811ecf662b4f7af84)), closes [#915](https://github.com/forkwright/aletheia/issues/915)
* **symbolon:** OAuth auto-refresh from Claude Code credentials ([#1357](https://github.com/forkwright/aletheia/issues/1357)) ([ab6b48d](https://github.com/forkwright/aletheia/commit/ab6b48d06741a0bbe764f9df49d795c6972156f5))
* **taxis:** add encryption at rest for sensitive config fields ([#1507](https://github.com/forkwright/aletheia/issues/1507)) ([cb354c0](https://github.com/forkwright/aletheia/commit/cb354c0356594f823a4ee2e28e696d8a1875332c))
* **taxis:** env var interpolation, preflight checks, workspace schema ([#1820](https://github.com/forkwright/aletheia/issues/1820)) ([835979a](https://github.com/forkwright/aletheia/commit/835979adc34a3dfc0871b714b7f2292a14e8d49c))
* **taxis:** implement config reload without restart ([2008633](https://github.com/forkwright/aletheia/commit/20086334fad67817f26cc948c6f662e457b76c21))
* **taxis:** reverse phantom config — promote hardcoded values to operator config ([#1269](https://github.com/forkwright/aletheia/issues/1269)) ([8bc1063](https://github.com/forkwright/aletheia/commit/8bc10639daffee2d4401a4ab54572e3f3b452f00))
* **test-infra:** test-support feature, nextest config, proptest corpus, mock components, spec validator ([#1821](https://github.com/forkwright/aletheia/issues/1821)) ([4e23772](https://github.com/forkwright/aletheia/commit/4e23772a5ce36f8bbd3c88fbc8c2f169e9a6bf3c))
* **theatron-desktop:** add chat message list and markdown renderer ([#1998](https://github.com/forkwright/aletheia/issues/1998)) ([cd1a456](https://github.com/forkwright/aletheia/commit/cd1a456a2a7ffd9ccd6aa939d12f10f55eebfd09))
* **theatron-desktop:** agent switching, slash commands, distillation indicator ([#2000](https://github.com/forkwright/aletheia/issues/2000)) ([4958aac](https://github.com/forkwright/aletheia/commit/4958aac9bac8e71274e9690912d02217fbcc2dcf))
* **theatron-desktop:** checkpoint approval gates and verification ([#2002](https://github.com/forkwright/aletheia/issues/2002)) ([94cbbf4](https://github.com/forkwright/aletheia/commit/94cbbf435189b0b4977de9da65304a27c89fc3b7))
* **theatron-desktop:** credential management panel for ops view ([#2007](https://github.com/forkwright/aletheia/issues/2007)) ([5511cb5](https://github.com/forkwright/aletheia/commit/5511cb523e9c403ebba9dfe770060a0c07ebb684))
* **theatron-desktop:** design system — tokens, themes, fonts, theme switching ([#1992](https://github.com/forkwright/aletheia/issues/1992)) ([1b2812d](https://github.com/forkwright/aletheia/commit/1b2812d78c13301237566241320106460b3623fe))
* **theatron-desktop:** desktop notifications with rate limiting and DND ([#2013](https://github.com/forkwright/aletheia/issues/2013)) ([f17cb8f](https://github.com/forkwright/aletheia/commit/f17cb8f9138e8ed15f376ca7ee9651d171b51630))
* **theatron-desktop:** desktop polish — virtual scroll, resize, keyboard nav, ARIA, perf ([#2015](https://github.com/forkwright/aletheia/issues/2015)) ([a399eb0](https://github.com/forkwright/aletheia/commit/a399eb02fa32cc7ded2f65788f00df2bb9aceb90))
* **theatron-desktop:** diff viewer and file change notifications ([#2003](https://github.com/forkwright/aletheia/issues/2003)) ([4a1c83e](https://github.com/forkwright/aletheia/commit/4a1c83e17842b70527016a74216a7a3e95b38bb9))
* **theatron-desktop:** discussion panel and execution view ([#2004](https://github.com/forkwright/aletheia/issues/2004)) ([8994622](https://github.com/forkwright/aletheia/commit/89946223257b035af7717e83c87e6947cc9f77e2))
* **theatron-desktop:** file tree explorer and syntax-highlighted viewer ([#2001](https://github.com/forkwright/aletheia/issues/2001)) ([25acc4c](https://github.com/forkwright/aletheia/commit/25acc4c6f5c7f16ec4e2503543338c2b97d299df))
* **theatron-desktop:** knowledge graph — 2D visualization, timeline, drift detection ([#2011](https://github.com/forkwright/aletheia/issues/2011)) ([287d544](https://github.com/forkwright/aletheia/commit/287d544f94f9f80951febec154e23c50a8b3bd75))
* **theatron-desktop:** memory explorer with entity list, detail, and actions ([#2012](https://github.com/forkwright/aletheia/issues/2012)) ([d66c5e6](https://github.com/forkwright/aletheia/commit/d66c5e634ad36c6c15a4f230cf1ab29783b9f86c))
* **theatron-desktop:** meta-insights — agent performance, knowledge growth, system self-reflection ([#2016](https://github.com/forkwright/aletheia/issues/2016)) ([0918306](https://github.com/forkwright/aletheia/commit/09183067e043e72770f647e8e8ca3befc79de419))
* **theatron-desktop:** ops dashboard with agent cards, health panel, and toggle controls ([#2008](https://github.com/forkwright/aletheia/issues/2008)) ([155df32](https://github.com/forkwright/aletheia/commit/155df3260aface9aed3575aac0e03215d260d08f))
* **theatron-desktop:** planning dashboard with projects, requirements, and roadmap ([#2005](https://github.com/forkwright/aletheia/issues/2005)) ([91ab029](https://github.com/forkwright/aletheia/commit/91ab029526abe3be7abbdc1522aaf48b25790733))
* **theatron-desktop:** session management — list, search, detail, archive ([#2006](https://github.com/forkwright/aletheia/issues/2006)) ([a51dec8](https://github.com/forkwright/aletheia/commit/a51dec863ad64673fe3f6f6a8d992728b5469d06))
* **theatron-desktop:** settings views — server connections, appearance, keybindings, setup wizard ([#2009](https://github.com/forkwright/aletheia/issues/2009)) ([f1b22af](https://github.com/forkwright/aletheia/commit/f1b22af85ac63f721e72d1a53102d2f90f9057c7))
* **theatron-desktop:** system tray, global hotkeys, native menus, window state ([#2010](https://github.com/forkwright/aletheia/issues/2010)) ([2f64b38](https://github.com/forkwright/aletheia/commit/2f64b3888539472f571a33605544ae40355c5102))
* **theatron-desktop:** token usage and cost metrics views ([#2017](https://github.com/forkwright/aletheia/issues/2017)) ([0b43a18](https://github.com/forkwright/aletheia/commit/0b43a1835342cf03e1aeaa9b2cc6fee29a520450)), closes [#114](https://github.com/forkwright/aletheia/issues/114)
* **theatron-desktop:** tool call display, approval, and planning cards ([#1999](https://github.com/forkwright/aletheia/issues/1999)) ([1ae3b31](https://github.com/forkwright/aletheia/commit/1ae3b3128d895ce880d40f9afb1a30ec4e35dbd3))
* **theatron-desktop:** tool usage stats — frequency, rates, duration, drill-down ([#2014](https://github.com/forkwright/aletheia/issues/2014)) ([9fd93d9](https://github.com/forkwright/aletheia/commit/9fd93d9227aadd4249923025a5b43ef7dea85424))
* **theatron:** add Nix flake for desktop packaging and dev shell ([#1291](https://github.com/forkwright/aletheia/issues/1291)) ([4821d8e](https://github.com/forkwright/aletheia/commit/4821d8e1d01f325c5c390755c5dec2ac5ae9021d))
* **theatron:** add server connection, SSE stream, and toast system ([#1993](https://github.com/forkwright/aletheia/issues/1993)) ([dc5db22](https://github.com/forkwright/aletheia/commit/dc5db225057c3ca13cab474544a971fbc272dc2f))
* **theatron:** add TUI session UX features ([#1285](https://github.com/forkwright/aletheia/issues/1285)) ([731235b](https://github.com/forkwright/aletheia/commit/731235bc3e773e2fb72bb9d4a427363c23a77381))
* **theatron:** desktop design system — tokens, themes, Tailwind, theme switching ([#1293](https://github.com/forkwright/aletheia/issues/1293)) ([2b1f27d](https://github.com/forkwright/aletheia/commit/2b1f27da6e60a5fd205daf4fcc6830f300df904a))
* **theatron:** desktop server connection layer ([#1292](https://github.com/forkwright/aletheia/issues/1292)) ([1c8a8e2](https://github.com/forkwright/aletheia/commit/1c8a8e2b6e7adea288cced0913537d0d3dbfe2f4))
* **theatron:** display primary arg, error summary, and actual duration in ops panel ([#1277](https://github.com/forkwright/aletheia/issues/1277)) ([50abc92](https://github.com/forkwright/aletheia/commit/50abc9236a4c3ef89c3b48a284e155c4b6be1b4c))
* **theatron:** global SSE event stream for desktop app ([#1294](https://github.com/forkwright/aletheia/issues/1294)) ([11cda0a](https://github.com/forkwright/aletheia/commit/11cda0aef788cf2989c874f9256f5198c52ab4af))
* **theatron:** implement desktop views with real API integration ([#1900](https://github.com/forkwright/aletheia/issues/1900)) ([01a8314](https://github.com/forkwright/aletheia/commit/01a8314531bfbc4c2dadbb8a92712e6465af4c58))
* **theatron:** inline image preview in TUI ([#1287](https://github.com/forkwright/aletheia/issues/1287)) ([f28a011](https://github.com/forkwright/aletheia/commit/f28a01137dd3ab2493570b43e62e91e532aa49bf))
* **theatron:** input bar, streaming, and thinking panels for desktop chat ([#1997](https://github.com/forkwright/aletheia/issues/1997)) ([106e5ed](https://github.com/forkwright/aletheia/commit/106e5edb95f1e964e1abe1d7d5e9689db7db499e))
* **theatron:** light theme support and tool enablement visibility ([#1288](https://github.com/forkwright/aletheia/issues/1288)) ([19506a0](https://github.com/forkwright/aletheia/commit/19506a013211ab46ececef71f610d44ea4db4447))
* **theatron:** ops pane redesign, credential display, and spawn instrumentation ([#1842](https://github.com/forkwright/aletheia/issues/1842)) ([768e9be](https://github.com/forkwright/aletheia/commit/768e9be15baf67cce4e6065b5da5f0d965501cbd))
* **theatron:** path independence and auto-reconnect ([#968](https://github.com/forkwright/aletheia/issues/968)) ([c2a5290](https://github.com/forkwright/aletheia/commit/c2a5290b4f10187df6e31a551299ece843ff9c0f))
* **theatron:** scaffold Dioxus 0.7 desktop app with Blitz native renderer ([#1289](https://github.com/forkwright/aletheia/issues/1289)) ([a1c174b](https://github.com/forkwright/aletheia/commit/a1c174bb492bd3e289a5a3fe15baaabc1e3af011))
* **theatron:** session agent labels and input layout fix ([#962](https://github.com/forkwright/aletheia/issues/962)) ([c3dd492](https://github.com/forkwright/aletheia/commit/c3dd492c1cd5b61e44998a06cbf48ca8776a9057)), closes [#946](https://github.com/forkwright/aletheia/issues/946) [#919](https://github.com/forkwright/aletheia/issues/919)
* **theatron:** TUI visual polish — keybindings, mouse, connection indicator, badges ([#1286](https://github.com/forkwright/aletheia/issues/1286)) ([1b78a89](https://github.com/forkwright/aletheia/commit/1b78a89c3d74a6f0597ca3d7448eae141fca36cc))
* **tui:** CC input keybindings, queued messages, and image paste ([#1952](https://github.com/forkwright/aletheia/issues/1952)) ([a05824f](https://github.com/forkwright/aletheia/commit/a05824f4cc08a2969d0f9db01ee3052bba233a55)), closes [#1892](https://github.com/forkwright/aletheia/issues/1892) [#1893](https://github.com/forkwright/aletheia/issues/1893)
* **tui:** CC-aligned rendering engine, streaming, and tool cards ([#1949](https://github.com/forkwright/aletheia/issues/1949)) ([5aff911](https://github.com/forkwright/aletheia/commit/5aff9114adbb7b94c7a78f08dc6a26bd3c349e91))
* **tui:** context budget visualization and distillation indicators ([#1927](https://github.com/forkwright/aletheia/issues/1927)) ([b639acd](https://github.com/forkwright/aletheia/commit/b639acd1466cbfa65e68b8dbdb3540d4962cc4a5))
* **tui:** execution progress indicators and decision cards ([#1939](https://github.com/forkwright/aletheia/issues/1939)) ([fbd9022](https://github.com/forkwright/aletheia/commit/fbd9022fe94d078f0bdaac5a32be0d2c24fae920))
* **tui:** file editor with syntax highlighting and tabs ([#1951](https://github.com/forkwright/aletheia/issues/1951)) ([1d3da97](https://github.com/forkwright/aletheia/commit/1d3da978af38d506c1e53dfed3b897f993f5ed06)), closes [#1859](https://github.com/forkwright/aletheia/issues/1859)
* **tui:** halt, stall detection, ops pane cleanup, session abstraction ([#1931](https://github.com/forkwright/aletheia/issues/1931)) ([1a63299](https://github.com/forkwright/aletheia/commit/1a63299fb70fbaa0be2b87678e30cf4fa99fa7fc))
* **tui:** knowledge graph visualization and 3D architecture doc ([#1955](https://github.com/forkwright/aletheia/issues/1955)) ([98a3576](https://github.com/forkwright/aletheia/commit/98a357628b2a869a66cb2151c3e7fd12b097c698))
* **tui:** message shading, retrospective view, planning dashboard (closes [#1958](https://github.com/forkwright/aletheia/issues/1958), closes [#1867](https://github.com/forkwright/aletheia/issues/1867), closes [#1856](https://github.com/forkwright/aletheia/issues/1856)) ([65c86b1](https://github.com/forkwright/aletheia/commit/65c86b16287322ca47ceb730894948612d9151b5))
* **tui:** metrics dashboard with token usage and service health ([#1945](https://github.com/forkwright/aletheia/issues/1945)) ([dd3a914](https://github.com/forkwright/aletheia/commit/dd3a9146bc115f7f08761a7591625c2878408129))
* **tui:** setup wizard for first-run instance initialization ([#1943](https://github.com/forkwright/aletheia/issues/1943)) ([61685b3](https://github.com/forkwright/aletheia/commit/61685b3a635adafe07594e16063386ba061f2101))
* **tui:** slash command autocomplete and notification system ([#1934](https://github.com/forkwright/aletheia/issues/1934)) ([ef81053](https://github.com/forkwright/aletheia/commit/ef810533a58d4ea851631f42e3de779b19829682))
* **tui:** tool approval dialog and category icons ([#1930](https://github.com/forkwright/aletheia/issues/1930)) ([26c135b](https://github.com/forkwright/aletheia/commit/26c135b349cedd50e4f8e740e5e0123fc1ca3e35))


### Bug Fixes

* add crypto provider init to all communication tests ([0628c53](https://github.com/forkwright/aletheia/commit/0628c535c165180409b67a90c680327b74b7398f))
* add missing docs to non-Linux sandbox stubs (fixes macOS build) ([b96ec70](https://github.com/forkwright/aletheia/commit/b96ec70742e5a6e6d2941bce080acb7ecd3e9d8e))
* **aletheia,daemon,dianoia,thesauros,eval:** resolve all kanon lint violations ([#1918](https://github.com/forkwright/aletheia/issues/1918)) ([ae53e2d](https://github.com/forkwright/aletheia/commit/ae53e2d786fd8c323e4f362116cc2286776379a7))
* **aletheia:** add embed-candle to default features ([#1263](https://github.com/forkwright/aletheia/issues/1263)) ([944c83e](https://github.com/forkwright/aletheia/commit/944c83e7a83c59fdc593bb89cdbe7e3847055c8c))
* **aletheia:** guard embed-candle default feature against removal ([#1488](https://github.com/forkwright/aletheia/issues/1488)) ([287a984](https://github.com/forkwright/aletheia/commit/287a98465420055acfbbebf178ff05a0e6ffb6e1))
* **aletheia:** health redirect, version bump, auth config warning ([#1261](https://github.com/forkwright/aletheia/issues/1261)) ([9b99e2e](https://github.com/forkwright/aletheia/commit/9b99e2e76b6f76da2468fe986ab56d1245254202))
* **aletheia:** resolve all non-Rust kanon lint violations ([#1916](https://github.com/forkwright/aletheia/issues/1916)) ([aadeb64](https://github.com/forkwright/aletheia/commit/aadeb640abfe4c943313e879f42cd2db57037a48))
* **aletheia:** resolve feature-gated compilation errors from Fact decomposition ([7e339ee](https://github.com/forkwright/aletheia/commit/7e339eef89cb9864c707c063191af83309548267))
* **aletheia:** restore embed-candle to default features ([#1380](https://github.com/forkwright/aletheia/issues/1380)) ([4de7b44](https://github.com/forkwright/aletheia/commit/4de7b4486ebbc4c9fd7d4c8cef23d101d91c4880))
* **aletheia:** sandbox config, credential status, auth warning, drift output (P330) ([6c76943](https://github.com/forkwright/aletheia/commit/6c7694347b1e895efc4fd17094e145b4d76fb7a7))
* **ci:** add RUSTSEC-2025-0134 (rustls-pemfile) to cargo-deny ignore list ([0270d19](https://github.com/forkwright/aletheia/commit/0270d1916f588615cce8bbd9dc26be42597315d4))
* **ci:** correct arg order — -r is a global flag, not subcommand flag ([2b753a7](https://github.com/forkwright/aletheia/commit/2b753a749af89530780eda7d205fc116ebd75b5a))
* **ci:** exclude theatron desktop from workspace until system deps available ([76890b8](https://github.com/forkwright/aletheia/commit/76890b8810635c25a70f30022e6f00911e60abfc))
* **ci:** exclude theatron-desktop from workspace (GTK deps break CI) ([b9dcc0d](https://github.com/forkwright/aletheia/commit/b9dcc0d6957dce4286a6097547915eb0f296efc9))
* **ci:** gate default_features test on all defaults, replace reqwest with raw HTTP in integration test ([2bba3a4](https://github.com/forkwright/aletheia/commit/2bba3a40e2ce974a105c6b6fa3d6bbe8da5c985e))
* **ci:** harden smoke test and split cargo-deny advisories ([#1557](https://github.com/forkwright/aletheia/issues/1557)) ([8c28d10](https://github.com/forkwright/aletheia/commit/8c28d10c4abcf1d5e694793d7105ed2422efa216))
* **ci:** mark integration_server test as #[ignore] for CI ([ecc8323](https://github.com/forkwright/aletheia/commit/ecc8323e4692d4d9ad9eac4368c5292764d41c50))
* **ci:** resolve clippy warnings on main ([#1427](https://github.com/forkwright/aletheia/issues/1427)) ([c53146b](https://github.com/forkwright/aletheia/commit/c53146b695c7662fa60748e693d97b78df2d315b))
* **ci:** use mock embedding provider in integration server test ([18035e9](https://github.com/forkwright/aletheia/commit/18035e9f4da35a3dc1ed03ffdbab767d09b9b27a))
* **cli,pylon:** resolve 5 CLI/server operational bugs ([#1994](https://github.com/forkwright/aletheia/issues/1994)) ([d380eca](https://github.com/forkwright/aletheia/commit/d380eca5f9bdc77299e459d9fa5ac867b13bc6dd))
* **cli:** improve error messages for 5 subcommands ([#1667](https://github.com/forkwright/aletheia/issues/1667)) ([d25bdf9](https://github.com/forkwright/aletheia/commit/d25bdf9485876f860a6057d8ddf3de9c1fff1321))
* **clippy:** remove duplicate non_exhaustive and doc backtick issues ([#1674](https://github.com/forkwright/aletheia/issues/1674)) ([a47a297](https://github.com/forkwright/aletheia/commit/a47a297717184031af57ce4a470c247de78334c9))
* **clippy:** resolve remaining clippy errors for release gate ([1676881](https://github.com/forkwright/aletheia/commit/16768811c53588ea1481d9191fa9c32100c72adb))
* **cli:** TOML preservation, health check, overwrite safety, output format ([#1192](https://github.com/forkwright/aletheia/issues/1192)) ([852b8bb](https://github.com/forkwright/aletheia/commit/852b8bb74b8b96bd8dff5b1806ecdf88362cbc32))
* confidence update, hard session delete, credential encryption ([#1753](https://github.com/forkwright/aletheia/issues/1753)) ([247fdf4](https://github.com/forkwright/aletheia/commit/247fdf4b954a752172b28cc78519cbd7625230ba))
* consolidate copy-paste drift (4 issues) ([#1228](https://github.com/forkwright/aletheia/issues/1228)) ([c509319](https://github.com/forkwright/aletheia/commit/c509319ecf540ac671fb31bc6eadc69e154e17e7))
* correct comment/code divergence (4 issues) ([#1226](https://github.com/forkwright/aletheia/issues/1226)) ([a590f20](https://github.com/forkwright/aletheia/commit/a590f20ad1d9443c693960288765886d72a391d6))
* correct snafu NoneError and unused feature flag ([#1217](https://github.com/forkwright/aletheia/issues/1217)) ([22a6879](https://github.com/forkwright/aletheia/commit/22a6879eb6f7dd75a174a178c82340ddd1c73f5a))
* crypto provider init in communication tests + flake.nix duplicate devShells ([6eb6fa6](https://github.com/forkwright/aletheia/commit/6eb6fa6a73e3e5c240ae4a521a08e7f99c6516c9))
* **deploy:** fix 7 deploy script ergonomics issues ([#1675](https://github.com/forkwright/aletheia/issues/1675)) ([5ae0674](https://github.com/forkwright/aletheia/commit/5ae0674896637c3240a810f27f5bd29a41124e47))
* **diaporeia:** MCP error sanitization (2 issues) ([#1186](https://github.com/forkwright/aletheia/issues/1186)) ([1bcc247](https://github.com/forkwright/aletheia/commit/1bcc247b00f6a5fff68ddfcd72048b7201e16d2a))
* **docs:** deployment guide field names, CSRF docs, auth setup, instance guidance ([#1053](https://github.com/forkwright/aletheia/issues/1053)) ([fa9539e](https://github.com/forkwright/aletheia/commit/fa9539e9815be049c8f1019ce35d82cb1ec1dfea))
* **docs:** resolve writing audit violations — CHANGELOG, em-dashes, config path ([#2036](https://github.com/forkwright/aletheia/issues/2036)) ([7c1f5c7](https://github.com/forkwright/aletheia/commit/7c1f5c7155bb4704c1b093661d4a31fa6ac79f9c))
* **eval:** framework reliability (4 issues) ([#1184](https://github.com/forkwright/aletheia/issues/1184)) ([e7d7949](https://github.com/forkwright/aletheia/commit/e7d79497667477818c9c9615d9ce924593cebb26))
* **graphe,episteme,krites,mneme:** resolve all kanon lint violations ([#1920](https://github.com/forkwright/aletheia/issues/1920)) ([5347732](https://github.com/forkwright/aletheia/commit/534773221791cd9237ca0587feda7050679356f1))
* harden tool executors and add session safety limits ([#899](https://github.com/forkwright/aletheia/issues/899)) ([87cdfc8](https://github.com/forkwright/aletheia/commit/87cdfc8be08dd228763e9edc2736d9b4d99ff19d))
* **hermeneus:** add anthropic-beta OAuth header for Messages API ([73cac0e](https://github.com/forkwright/aletheia/commit/73cac0e43961473ea223990c79b89f335a14652e))
* **hermeneus:** add Haiku 4.5 pricing configuration ([#1369](https://github.com/forkwright/aletheia/issues/1369)) ([73be73b](https://github.com/forkwright/aletheia/commit/73be73b76da8e01e9ccbe3fc7992abdafffcf224)), closes [#1329](https://github.com/forkwright/aletheia/issues/1329)
* **hermeneus:** circuit-breaker, incremental SSE, stop_reason, cache tokens, pricing ([#1198](https://github.com/forkwright/aletheia/issues/1198)) ([22d2bec](https://github.com/forkwright/aletheia/commit/22d2bec8949bbace2eeabea4b018684646c0f4ab))
* **hermeneus:** log full error body with model/token context ([#1678](https://github.com/forkwright/aletheia/issues/1678)) ([7e35510](https://github.com/forkwright/aletheia/commit/7e3551072f423a89f071a8e0ffbc1485d3b75df5))
* **hermeneus:** OAuth system prompt identity for Sonnet/Opus access ([ae5c1d8](https://github.com/forkwright/aletheia/commit/ae5c1d8b8d868f57ff5b6deb74b0667378eae4ba))
* **hermeneus:** remove invalid OAuth beta header causing 400 errors ([#1744](https://github.com/forkwright/aletheia/issues/1744)) ([ce7484b](https://github.com/forkwright/aletheia/commit/ce7484b62ec894c725ffcdbb4807224504be778d))
* **hermeneus:** resolve streaming empty tool_use warn and add missing model pricing ([#1264](https://github.com/forkwright/aletheia/issues/1264)) ([69d336b](https://github.com/forkwright/aletheia/commit/69d336bed282cbd60c4e1d062754334f556e1ce4))
* **init,cli:** resolve 8 init and CLI issues ([#1757](https://github.com/forkwright/aletheia/issues/1757)) ([29e8630](https://github.com/forkwright/aletheia/commit/29e8630d7c10d063402270783f33c4bd93eb591a))
* **koina,eidos,taxis,symbolon:** resolve all kanon lint violations ([#1917](https://github.com/forkwright/aletheia/issues/1917)) ([8bd5749](https://github.com/forkwright/aletheia/commit/8bd57496b5d83f8c346d3df5f7c97ebdc5e383aa))
* **lint:** address as_conversions, indexing_slicing, and string_slice violations ([#1682](https://github.com/forkwright/aletheia/issues/1682)) ([cac3a3e](https://github.com/forkwright/aletheia/commit/cac3a3eb3852e85e1634db72c7ac17712c0c4e7c))
* **lint:** annotate remaining RUST/expect linter hits ([#1574](https://github.com/forkwright/aletheia/issues/1574)) ([b269469](https://github.com/forkwright/aletheia/commit/b269469554e9b732fdc6f9c831dda1be838aa31c))
* **melete:** distillation safety (5 issues) ([#1190](https://github.com/forkwright/aletheia/issues/1190)) ([0f105c0](https://github.com/forkwright/aletheia/commit/0f105c0118b886467c3a12677849ab24d427b04b))
* **melete:** skip distillation for ephemeral sessions ([#1490](https://github.com/forkwright/aletheia/issues/1490)) ([3e924bb](https://github.com/forkwright/aletheia/commit/3e924bb58821f7eba15db017318425781694331f))
* **migrate-memory:** read instance embedding config, fix Qdrant scroll ([#1995](https://github.com/forkwright/aletheia/issues/1995)) ([f80fb44](https://github.com/forkwright/aletheia/commit/f80fb446a4825fb1f40944e7da6f831a167e4578))
* **mneme,nous:** knowledge pipeline fixes and redb removal (P322) ([#907](https://github.com/forkwright/aletheia/issues/907)) ([f0a0e63](https://github.com/forkwright/aletheia/commit/f0a0e6341d713595304e32ceca08bc7d8e182da7))
* **mneme:** accept novel LLM-generated relationship types ([#1496](https://github.com/forkwright/aletheia/issues/1496)) ([703f9b0](https://github.com/forkwright/aletheia/commit/703f9b071c21e44433511228b44deefefb1a928a))
* **mneme:** correct 5 search/embedding algorithmic bugs ([#1197](https://github.com/forkwright/aletheia/issues/1197)) ([8584a75](https://github.com/forkwright/aletheia/commit/8584a752b3edb5388c7ed9b5add40f79c75583bf))
* **mneme:** eliminate engine panics and add safety guards ([#902](https://github.com/forkwright/aletheia/issues/902)) ([6916fda](https://github.com/forkwright/aletheia/commit/6916fda577366f313f3646961a2563d566c5afc1))
* **mneme:** extraction facts queryable via API and distillation UNIQUE constraint ([#1271](https://github.com/forkwright/aletheia/issues/1271)) ([0262d83](https://github.com/forkwright/aletheia/commit/0262d8391a22782415b31eaa0159c6fb274fa11b))
* **mneme:** filter keyword-prefixed words in FTS single-word proptest ([#954](https://github.com/forkwright/aletheia/issues/954)) ([62b7b6e](https://github.com/forkwright/aletheia/commit/62b7b6ef4d2c53eeb7c6904987589b2afb7ee3db))
* **mneme:** knowledge facts API returning empty results ([#1350](https://github.com/forkwright/aletheia/issues/1350)) ([238eb57](https://github.com/forkwright/aletheia/commit/238eb574b1e84c67f18137a098418b9292ed7939)), closes [#1327](https://github.com/forkwright/aletheia/issues/1327)
* **mneme:** make skill_decay test deterministic ([3d4e4bf](https://github.com/forkwright/aletheia/commit/3d4e4bf52d9da8541dafe4f6e22c5172e3361be9))
* **mneme:** race conditions, constraints, and indexes (5 issues) ([#1204](https://github.com/forkwright/aletheia/issues/1204)) ([3b9c6aa](https://github.com/forkwright/aletheia/commit/3b9c6aac9ab1704df21a91261bb729246e40ba80))
* **mneme:** reduce log verbosity and add entity merge proptest ([#967](https://github.com/forkwright/aletheia/issues/967)) ([8602e92](https://github.com/forkwright/aletheia/commit/8602e9262effeade6a8458937900275623e5906b))
* **mneme:** remove remaining unwrap() calls in doc examples ([#1578](https://github.com/forkwright/aletheia/issues/1578)) ([df07bbe](https://github.com/forkwright/aletheia/commit/df07bbe9d5e5f17fe07716f756501ed62704f210))
* **mneme:** replace direct array indexing with bounds-checked access ([399648f](https://github.com/forkwright/aletheia/commit/399648fc309c32e36e6a7efadb03f008eb46b62c))
* **mneme:** session display_name migration and API exposure ([#1363](https://github.com/forkwright/aletheia/issues/1363)) ([6273e7d](https://github.com/forkwright/aletheia/commit/6273e7dc6b42cd43e5eff63fd72d96908b22d485))
* **nous,hermeneus,organon,melete:** resolve all kanon lint violations ([#1921](https://github.com/forkwright/aletheia/issues/1921)) ([b9c6a59](https://github.com/forkwright/aletheia/commit/b9c6a59054982b66c817224ee0e09cd98e7be3c7))
* **nous,organon:** tool spam, path validation, sandbox RLIMIT ([#1991](https://github.com/forkwright/aletheia/issues/1991)) ([541237e](https://github.com/forkwright/aletheia/commit/541237eb9c2e399e65e69347f3615b2ca7fe4b8f))
* **nous:** clean up pending_replies on all ask() exit paths ([#1379](https://github.com/forkwright/aletheia/issues/1379)) ([9897487](https://github.com/forkwright/aletheia/commit/98974877eb422ebc78c40781ed326222e69f387f))
* **nous:** pipeline races and resilience (6 issues) ([#1188](https://github.com/forkwright/aletheia/issues/1188)) ([c2220e1](https://github.com/forkwright/aletheia/commit/c2220e193aaeda0df0e6e5a13e7efcb94699f3c0))
* **nous:** recall and context assembly pipeline (7 issues) ([#1200](https://github.com/forkwright/aletheia/issues/1200)) ([99419e3](https://github.com/forkwright/aletheia/commit/99419e397df08ebe0b69554ee217451afe16f971))
* **nous:** replace .expect() with match in roles test ([f489874](https://github.com/forkwright/aletheia/commit/f489874d3d85e047fa2c020fa5fe598798982e0c))
* **nous:** replace blocking_lock with Handle::block_on(lock().await) in adapters ([#1266](https://github.com/forkwright/aletheia/issues/1266)) ([ab8473c](https://github.com/forkwright/aletheia/commit/ab8473cf4307c0c91693382068d579dba2a90cf4))
* **nous:** resolve session ID divergence causing FK constraint failures (P326) ([1aa1674](https://github.com/forkwright/aletheia/commit/1aa1674e8604b6c65e8c31638041888bdd197403))
* **oikonomos:** daemon wiring and reliability ([#1191](https://github.com/forkwright/aletheia/issues/1191)) ([2ade216](https://github.com/forkwright/aletheia/commit/2ade2164b14b6b45ee098e895baf544c2d74415b))
* **organon,episteme,koina:** resolve expect_used and as_conversions lint violations ([#1957](https://github.com/forkwright/aletheia/issues/1957)) ([4ef84b9](https://github.com/forkwright/aletheia/commit/4ef84b93811fc5fa477ffb61feb3cb57aea7cabb))
* **organon:** Landlock exec Permission Denied on ABI v7 ([#1354](https://github.com/forkwright/aletheia/issues/1354)) ([7464776](https://github.com/forkwright/aletheia/commit/7464776c7f6a44f547e8809039e716f8be7a58d9)), closes [#1304](https://github.com/forkwright/aletheia/issues/1304)
* **organon:** Landlock sandbox fallback with clear ABI errors ([#1218](https://github.com/forkwright/aletheia/issues/1218)) ([30806ae](https://github.com/forkwright/aletheia/commit/30806ae4409fcc919b82e208425ce04752f04abe))
* **organon:** Landlock sandbox fallback with clear ABI errors ([#965](https://github.com/forkwright/aletheia/issues/965)) ([dd669a3](https://github.com/forkwright/aletheia/commit/dd669a393c589003cf1ab5fce8681df7af0a79a5))
* **organon:** remove dead Mem0 tools and fix memory_search routing ([#1368](https://github.com/forkwright/aletheia/issues/1368)) ([0b4f5c0](https://github.com/forkwright/aletheia/commit/0b4f5c02c42aca1ca63ca764c579ced000baff5b))
* **organon:** sandbox exec paths, tilde expansion, permissive init defaults ([#1260](https://github.com/forkwright/aletheia/issues/1260)) ([757a04f](https://github.com/forkwright/aletheia/commit/757a04f5ee2a691ff7a6ec984435495f1239178c))
* **organon:** sandbox safety audit and dead config cleanup ([#1231](https://github.com/forkwright/aletheia/issues/1231)) ([f934c64](https://github.com/forkwright/aletheia/commit/f934c64faeb944f33e02f58f741c0cc67a6952d4))
* **organon:** tool safety and bounds (4 issues) ([#1193](https://github.com/forkwright/aletheia/issues/1193)) ([bbfaecb](https://github.com/forkwright/aletheia/commit/bbfaecbfef2566908b558516cef4889527c3ca24))
* pre-release gate fixes — fmt, view_nav match, workflow sync ([3cd5df6](https://github.com/forkwright/aletheia/commit/3cd5df65e423ac8d6dc85b1456c03333bc80bbfe))
* **pylon,episteme:** cap query limit, tighten episteme visibility (closes [#1963](https://github.com/forkwright/aletheia/issues/1963), closes [#1962](https://github.com/forkwright/aletheia/issues/1962)) ([e9b387d](https://github.com/forkwright/aletheia/commit/e9b387d07ea3173246f7f50821bd87f53cff2b85))
* **pylon,theatron,diaporeia:** resolve all kanon lint violations ([#1919](https://github.com/forkwright/aletheia/issues/1919)) ([595d148](https://github.com/forkwright/aletheia/commit/595d1488b4b54d94e8e71ffea413bced3a25c12a))
* **pylon:** add request_id to CSRF and rate limit responses ([#1356](https://github.com/forkwright/aletheia/issues/1356)) ([aae634a](https://github.com/forkwright/aletheia/commit/aae634a0977096594ec7d8e6c59b282c82fb9099))
* **pylon:** address 9 HTTP layer bugs (P311) ([#903](https://github.com/forkwright/aletheia/issues/903)) ([f497d3b](https://github.com/forkwright/aletheia/commit/f497d3ba5f9bde2454095e495328d8e9c0a377e7))
* **pylon:** API correctness — session limit, duplicate key, archived msg, delete semantics, SSE events ([#1265](https://github.com/forkwright/aletheia/issues/1265)) ([66a5281](https://github.com/forkwright/aletheia/commit/66a528120659cee9855385ed5e4cf90337cd9d1a))
* **pylon:** API correctness — stubs, pagination, filtering (7 issues) ([#1189](https://github.com/forkwright/aletheia/issues/1189)) ([38aea53](https://github.com/forkwright/aletheia/commit/38aea53714e91ece74c4668a644c6f204366d260))
* **pylon:** health check session_store reporting ([#1360](https://github.com/forkwright/aletheia/issues/1360)) ([d493c3a](https://github.com/forkwright/aletheia/commit/d493c3a32ffcc62b1c2c6aa4cfaea547ba8918a8)), closes [#1298](https://github.com/forkwright/aletheia/issues/1298)
* **pylon:** health check tolerates busy actors during message processing ([#1123](https://github.com/forkwright/aletheia/issues/1123)) ([03345a3](https://github.com/forkwright/aletheia/commit/03345a309dc9c4f73fc146c7fd4daa7a93db514e))
* **pylon:** remove double message persistence ([#949](https://github.com/forkwright/aletheia/issues/949)) ([777f3b2](https://github.com/forkwright/aletheia/commit/777f3b28cae142de71a127df217e083480b3b3ec))
* **pylon:** resolve rustdoc and unfulfilled lint expectation errors ([99c35ff](https://github.com/forkwright/aletheia/commit/99c35ffeb68043db346a71cc69e4a2a2b23a2898))
* **pylon:** session safety and error mapping (5 issues) ([#1196](https://github.com/forkwright/aletheia/issues/1196)) ([ae75eeb](https://github.com/forkwright/aletheia/commit/ae75eeb8c128d4add89feee900560bacb4645308))
* **pylon:** validate knowledge API sort/order params ([#1362](https://github.com/forkwright/aletheia/issues/1362)) ([09b9e0c](https://github.com/forkwright/aletheia/commit/09b9e0cdd394b952938dcaad5bed3b69ed93ce6d)), closes [#1321](https://github.com/forkwright/aletheia/issues/1321)
* remove unfulfilled dead_code expects in msg.rs and overlay.rs ([b57cd66](https://github.com/forkwright/aletheia/commit/b57cd66abd35e5afd900c01df56548d449f82844))
* **resilience:** graceful shutdown, OOM, disk, embedding, streaming ([#1758](https://github.com/forkwright/aletheia/issues/1758)) ([742d4fd](https://github.com/forkwright/aletheia/commit/742d4fd6f04b12f849efa04c40751206bd2f6193))
* resolve 6 code quality audit findings ([#1923](https://github.com/forkwright/aletheia/issues/1923)) ([17ec00d](https://github.com/forkwright/aletheia/commit/17ec00ddade286d62783c0dc55ec783a085f6751))
* resolve clippy lint violations across workspace ([9fc0ae8](https://github.com/forkwright/aletheia/commit/9fc0ae8eefcaabd8e39d1cc26313d0749b64943a))
* resolve Rust 1.94 clippy lints blocking CI ([#1166](https://github.com/forkwright/aletheia/issues/1166)) ([658dbf6](https://github.com/forkwright/aletheia/commit/658dbf633851bec68a0cbf900ce9a909db49872f))
* restore flake.nix closing braces after devShells restructure ([be3a035](https://github.com/forkwright/aletheia/commit/be3a03588be77bc310a7be6e9f5a1b894d40867b))
* **runtime:** three runtime behavior fixes ([#1679](https://github.com/forkwright/aletheia/issues/1679)) ([1c326b0](https://github.com/forkwright/aletheia/commit/1c326b01368ded591f436f8f4876337e9002df2b))
* **safety:** replace unsafe indexing with .get() and justified expects in theatron-tui ([#1693](https://github.com/forkwright/aletheia/issues/1693)) ([d6ecf4e](https://github.com/forkwright/aletheia/commit/d6ecf4e6d04fe99f00c0854cc37198a27cf2638d))
* **scripts:** add set -euo pipefail to all shell scripts ([#1476](https://github.com/forkwright/aletheia/issues/1476)) ([fd8e6b1](https://github.com/forkwright/aletheia/commit/fd8e6b1366aae8c628f802c54e3b65a9b99ecf2b))
* **scripts:** fix 8 deploy and operations issues ([#1746](https://github.com/forkwright/aletheia/issues/1746)) ([09b83d1](https://github.com/forkwright/aletheia/commit/09b83d1b147455fed6a2aa8e95dcc6bc63cdcb62))
* **security:** address 10 of 13 CodeQL alerts ([#1597](https://github.com/forkwright/aletheia/issues/1597)) ([67fd666](https://github.com/forkwright/aletheia/commit/67fd66626dd4dc53240ec8a2430244d77b439664))
* **security:** resolve audit findings — size limits, ProcessGuard, struct decomposition ([#1924](https://github.com/forkwright/aletheia/issues/1924)) ([6743a82](https://github.com/forkwright/aletheia/commit/6743a82804563c72c05eb522b9790afaaf4ce99a))
* **security:** resolve CodeQL cleartext alerts (closes [#1956](https://github.com/forkwright/aletheia/issues/1956)) ([7b068ab](https://github.com/forkwright/aletheia/commit/7b068ab2348f0f6fb945c56ea9eb435e71fa12b1))
* **shutdown:** collect fire-and-forget spawns, add cancellation to async loops ([#1673](https://github.com/forkwright/aletheia/issues/1673)) ([1faa2d9](https://github.com/forkwright/aletheia/commit/1faa2d9d3ee52e962bb8de6a01bf611982c691ad))
* **symbolon:** add clock skew tolerance to OAuth token expiry check ([#1497](https://github.com/forkwright/aletheia/issues/1497)) ([787a72e](https://github.com/forkwright/aletheia/commit/787a72eaaa7e0cf7f0f79a4ddc1463062fe07002))
* **symbolon:** fix SecretString type mismatch in auth and JWT tests ([#1577](https://github.com/forkwright/aletheia/issues/1577)) ([0a21a39](https://github.com/forkwright/aletheia/commit/0a21a392f826c5b3b02089451c10c84909327223))
* **symbolon:** handle claudeAiOauth wrapper and fall through expired OAuth env tokens ([#1270](https://github.com/forkwright/aletheia/issues/1270)) ([b05bfad](https://github.com/forkwright/aletheia/commit/b05bfad18cee52df3e0b1b8c94fc67a00d6271c2))
* **symbolon:** harden OAuth refresh chain for standalone operation ([#1985](https://github.com/forkwright/aletheia/issues/1985)) ([2911f81](https://github.com/forkwright/aletheia/commit/2911f81f3604dd79bf5f4a90828a770372ba382b))
* **symbolon:** OAuth refresh uses correct URL and form-urlencoded format ([948dc7e](https://github.com/forkwright/aletheia/commit/948dc7ed36e1289645d8d974b6657870d9946ed8))
* **symbolon:** reject insecure default JWT key at startup ([#1364](https://github.com/forkwright/aletheia/issues/1364)) ([041401e](https://github.com/forkwright/aletheia/commit/041401e645a321c283211dd13345f055e42ef220)), closes [#1315](https://github.com/forkwright/aletheia/issues/1315)
* **taxis,organon:** status false-negative, sandbox HOME default, init pricing camelCase ([#1841](https://github.com/forkwright/aletheia/issues/1841)) ([3c778b2](https://github.com/forkwright/aletheia/commit/3c778b26cb335099d762707200d24add7a8b13f1))
* **taxis:** complete TOML migration cleanup (P325) ([8b9704c](https://github.com/forkwright/aletheia/commit/8b9704c9778a83d8b44ffcc82af1ff2e927ea57c))
* **test:** add test-core/test-full feature tiers ([#1895](https://github.com/forkwright/aletheia/issues/1895)) ([#1937](https://github.com/forkwright/aletheia/issues/1937)) ([5dc57f8](https://github.com/forkwright/aletheia/commit/5dc57f8d842c817a39602c2cca35ea2472b36c94))
* **tests:** resolve lint batch 4 — unwrap, coverage, perms, timeouts ([#1942](https://github.com/forkwright/aletheia/issues/1942)) ([1082945](https://github.com/forkwright/aletheia/commit/108294542143537aab6c9ff253b7cb3deed90c90)), closes [#1915](https://github.com/forkwright/aletheia/issues/1915)
* **test:** wire test-core feature to enable engine tests ([#1965](https://github.com/forkwright/aletheia/issues/1965)) ([bfb074b](https://github.com/forkwright/aletheia/commit/bfb074b534345354792ec92ba309e2d0e24f3b77))
* **theatron-desktop:** resolve audit violations — target/ exclusion, TODO refs, allow→expect ([#2037](https://github.com/forkwright/aletheia/issues/2037)) ([576ab4f](https://github.com/forkwright/aletheia/commit/576ab4f339e4a9e3e2bc9338c6b7ecd80c84a44b))
* **theatron-tui:** scroll, agent switching, tool rendering, session persistence ([#1844](https://github.com/forkwright/aletheia/issues/1844)) ([4bf0388](https://github.com/forkwright/aletheia/commit/4bf0388fd4d8ab17e6031dd07469ebe4ee6a0152))
* **theatron:** command menu navigation and :recall ([#1365](https://github.com/forkwright/aletheia/issues/1365)) ([3ea3827](https://github.com/forkwright/aletheia/commit/3ea3827d9347ea45750ae1b1d11d5a59adf30ce3))
* **theatron:** eliminate TUI panics in event parsing ([#1183](https://github.com/forkwright/aletheia/issues/1183)) ([6d4f02c](https://github.com/forkwright/aletheia/commit/6d4f02c47d4d3fc4c438e19e763e1b1b65cd752e))
* **theatron:** error mapping and confidence persistence ([#963](https://github.com/forkwright/aletheia/issues/963)) ([d886d66](https://github.com/forkwright/aletheia/commit/d886d66a5bba58d4c193627849be93acc8cfac53))
* **theatron:** line-by-line scrolling in TUI ([#1366](https://github.com/forkwright/aletheia/issues/1366)) ([af1edc9](https://github.com/forkwright/aletheia/commit/af1edc956b4cf75b8c822a3be7552c86d8331a1c)), closes [#1337](https://github.com/forkwright/aletheia/issues/1337)
* **theatron:** message persistence on send ([#1371](https://github.com/forkwright/aletheia/issues/1371)) ([881656d](https://github.com/forkwright/aletheia/commit/881656d9192feb9f543d32dd8547b2c1c07525eb)), closes [#1305](https://github.com/forkwright/aletheia/issues/1305)
* **theatron:** repair TUI chat viewport scroll behavior ([#1124](https://github.com/forkwright/aletheia/issues/1124)) ([0be1447](https://github.com/forkwright/aletheia/commit/0be14479839965e0fa506e718eff9069cd752db9))
* **theatron:** scroll_line_down logic — enable auto_scroll when reaching offset 0 ([1febcf5](https://github.com/forkwright/aletheia/commit/1febcf5604baeda17f9bc368ba72cbd0ed1e5d2c))
* **theatron:** SSE connection reliability (3 issues) ([#1203](https://github.com/forkwright/aletheia/issues/1203)) ([87e11b2](https://github.com/forkwright/aletheia/commit/87e11b2b78e2a9f9404c855f9b04005fdd119b0e))
* **theatron:** stale indicator and prosoche session filtering ([#1358](https://github.com/forkwright/aletheia/issues/1358)) ([5f9ecb8](https://github.com/forkwright/aletheia/commit/5f9ecb8b9717887c666fa199d6db3bfd393a3b1f))
* **theatron:** streaming render speed and response truncation ([#1351](https://github.com/forkwright/aletheia/issues/1351)) ([3594262](https://github.com/forkwright/aletheia/commit/3594262e9b5d31459bb92367e3303db920805cb4))
* **theatron:** success routing and non-ASCII highlight bugs ([#966](https://github.com/forkwright/aletheia/issues/966)) ([22c7655](https://github.com/forkwright/aletheia/commit/22c7655ad9b056d5de162344f2066b09fe245401))
* **theatron:** table border artifacts and inline code contrast ([#1367](https://github.com/forkwright/aletheia/issues/1367)) ([35460b6](https://github.com/forkwright/aletheia/commit/35460b641f15b4b273466c7185b500804fd516b4))
* **theatron:** TUI input UX — keybindings, multi-line editing, auto-scroll (P328) ([#937](https://github.com/forkwright/aletheia/issues/937)) ([f240d8e](https://github.com/forkwright/aletheia/commit/f240d8ed841c2b1609a68af41557bbed59834588))
* **theatron:** TUI input/display bugs (5 issues) ([#1205](https://github.com/forkwright/aletheia/issues/1205)) ([ef29778](https://github.com/forkwright/aletheia/commit/ef297783bbd9725f9e90e3cf671aedecb670e28d))
* **theatron:** viewport state, scroll bounds, cache invalidation (5 issues) ([#1195](https://github.com/forkwright/aletheia/issues/1195)) ([45e7347](https://github.com/forkwright/aletheia/commit/45e73476961fc5fd5c9c9b2f354f6dd7c31c8e7e))
* **theatron:** wire missing keybindings ([#1211](https://github.com/forkwright/aletheia/issues/1211)) ([eebc00b](https://github.com/forkwright/aletheia/commit/eebc00b57848f84bd92a165805a8680ab3bee098))
* **thesauros,hermeneus:** resolve 5 async/concurrency issues ([#811](https://github.com/forkwright/aletheia/issues/811)-[#814](https://github.com/forkwright/aletheia/issues/814), [#877](https://github.com/forkwright/aletheia/issues/877)) ([#905](https://github.com/forkwright/aletheia/issues/905)) ([9257d0e](https://github.com/forkwright/aletheia/commit/9257d0ee73a473c6fb1a9cc408ba8f6b3e98efb5))
* **tui:** check reachability not health status for gateway connection ([9f16882](https://github.com/forkwright/aletheia/commit/9f1688214d4f4236c2cdee4cfa53a83e4e0ede1c))
* **tui:** cursor style and raw JSON tool call rendering on reload ([#1932](https://github.com/forkwright/aletheia/issues/1932)) ([bdeefe0](https://github.com/forkwright/aletheia/commit/bdeefe08aecf2034fb5dea1c00befdfce0f4f7c6))
* **tui:** cursor style, paragraph breaks, SSE reconnect, stale docs ([#1987](https://github.com/forkwright/aletheia/issues/1987)) ([3eadaa7](https://github.com/forkwright/aletheia/commit/3eadaa7ca78ea176c65f09ea9e85b2a584391bde))
* **tui:** remove duplicate success_toast field ([#984](https://github.com/forkwright/aletheia/issues/984)) ([1659a66](https://github.com/forkwright/aletheia/commit/1659a666c6a5212c690d507d8354becf82da4e05))
* unresolved rustdoc links in koina event and output_buffer ([18a5e53](https://github.com/forkwright/aletheia/commit/18a5e538182c61b593d1a19f1aa17bf9afabb55d))
* use ALETHEIA_ROOT in macOS LaunchAgent plist ([#958](https://github.com/forkwright/aletheia/issues/958)) ([a2e470a](https://github.com/forkwright/aletheia/commit/a2e470af517ccdb166bc7aa9864c1cf552f58117))
* validation and safety across symbolon, taxis, thesauros (7 issues) ([#1201](https://github.com/forkwright/aletheia/issues/1201)) ([ab3c386](https://github.com/forkwright/aletheia/commit/ab3c386d651d834cc3af7077e4547676393c62af))
* **workspace:** add .instrument() to 21 tokio::spawn calls ([579dda6](https://github.com/forkwright/aletheia/commit/579dda6efae7ccf537898d6dc21c503fadcf74d8))
* **workspace:** deny clippy::unwrap_used and clippy::expect_used (P323) ([#938](https://github.com/forkwright/aletheia/issues/938)) ([3c936b7](https://github.com/forkwright/aletheia/commit/3c936b7496bc71f83c650132bdb9082d4a5e8231))
* **workspace:** remove 11 unwrap() calls in non-test code ([#1538](https://github.com/forkwright/aletheia/issues/1538)) ([30c50fc](https://github.com/forkwright/aletheia/commit/30c50fc0ece10e967f960a49e07a7b6c7d5a5093))
* **workspace:** replace println! calls in library code with tracing macros ([#1537](https://github.com/forkwright/aletheia/issues/1537)) ([51f448b](https://github.com/forkwright/aletheia/commit/51f448b83f5d25f4a9d559376e383f7738ac007c))
* **workspace:** replace string slicing with safe .get() alternatives ([#1539](https://github.com/forkwright/aletheia/issues/1539)) ([c859e83](https://github.com/forkwright/aletheia/commit/c859e837e9032640b2ba635ea484b342f7c33b16))
* **workspace:** unify SecretString type, resolve clippy warnings ([#1587](https://github.com/forkwright/aletheia/issues/1587)) ([11899b4](https://github.com/forkwright/aletheia/commit/11899b464a266e7f4115faaa885f5b08fd0c3550))


### Performance

* **build:** increase codegen-units for faster dev builds ([#1477](https://github.com/forkwright/aletheia/issues/1477)) ([5b4a623](https://github.com/forkwright/aletheia/commit/5b4a623fa01324a3dab80555dbe75ac97cd425bb)), closes [#1420](https://github.com/forkwright/aletheia/issues/1420)
* **build:** replace onig with fancy-regex, remove unused reqwest blocking ([#1688](https://github.com/forkwright/aletheia/issues/1688)) ([f3d0a84](https://github.com/forkwright/aletheia/commit/f3d0a843d2f1b379e706df584ac238e5e864d404))
* **mneme:** iterate get_history_with_budget at SQL level ([#1508](https://github.com/forkwright/aletheia/issues/1508)) ([6eb2695](https://github.com/forkwright/aletheia/commit/6eb2695503c5806ca42219f80b2b4edf182d0ba9))
* **mneme:** replace embedding Mutex with RwLock for concurrent recall ([#1499](https://github.com/forkwright/aletheia/issues/1499)) ([4869cf1](https://github.com/forkwright/aletheia/commit/4869cf1855227f788b542b0a8bf2e4d0eaa68597))
* **theatron:** batch streaming token renders at frame boundary ([#1502](https://github.com/forkwright/aletheia/issues/1502)) ([429bde7](https://github.com/forkwright/aletheia/commit/429bde76211f2ef9b971d1437a8049e66c257165))
* **theatron:** TUI rendering performance (3 issues) ([#1238](https://github.com/forkwright/aletheia/issues/1238)) ([7a3eb3b](https://github.com/forkwright/aletheia/commit/7a3eb3b0e59c3d3baf4dcb3116013e90a06244c4))


### Documentation

* accuracy sweep — 14 issues across 8 files (P319) ([ba6d788](https://github.com/forkwright/aletheia/commit/ba6d788c64c578b6a4913554a2a16255bd064849))
* add # Errors sections to top 20 fallible public functions ([58a50fe](https://github.com/forkwright/aletheia/commit/58a50fe8a80b9d5993931526ea72ad4b2e338a07))
* add deploy script and health monitor to CLAUDE.md and RUNBOOK.md ([2651ab4](https://github.com/forkwright/aletheia/commit/2651ab41d031cc9352f3df156a9af8e1704067d2))
* add per-crate CLAUDE.md and agent navigation improvements ([#1666](https://github.com/forkwright/aletheia/issues/1666)) ([c096ffd](https://github.com/forkwright/aletheia/commit/c096ffdb69efcee34669bfb9beda46b3046ffb64))
* **aletheia:** add browser automation tool research ([#1513](https://github.com/forkwright/aletheia/issues/1513)) ([891584c](https://github.com/forkwright/aletheia/commit/891584c769a271a704ca2db93ddeef4da6ec23d0))
* consolidate and clean up documentation ([#1751](https://github.com/forkwright/aletheia/issues/1751)) ([74bc5c5](https://github.com/forkwright/aletheia/commit/74bc5c50a17f8f2fd30f8484ef013891d52550f8))
* consolidate, deduplicate, and make evergreen ([2394581](https://github.com/forkwright/aletheia/commit/2394581088c77381de1fac2bdcf14cbde3682059))
* convert all config examples from YAML to TOML syntax ([#1660](https://github.com/forkwright/aletheia/issues/1660)) ([ce680f6](https://github.com/forkwright/aletheia/commit/ce680f6e037b1b2e5ab56fb061e537a90ea9e977))
* cutover checklist for TS → Rust migration ([#1290](https://github.com/forkwright/aletheia/issues/1290)) ([2111282](https://github.com/forkwright/aletheia/commit/211128211d177cd071aacd21d4cc937bbf07f961))
* deploy standards/, remove legacy .claude/ and docs/STANDARDS.md ([5b07c40](https://github.com/forkwright/aletheia/commit/5b07c406b676495f02103e8c66b0b6856bcfc093))
* deployment documentation fixes (5 issues) ([#1194](https://github.com/forkwright/aletheia/issues/1194)) ([338198f](https://github.com/forkwright/aletheia/commit/338198f76788e49f6f73af9e3d8819929926d34a))
* document shared state lock invariants across 6 crates ([#1671](https://github.com/forkwright/aletheia/issues/1671)) ([82a7f96](https://github.com/forkwright/aletheia/commit/82a7f9612ae8903314567e6de465f985491f15e3))
* fix 16 writing standard v2 violations ([#1747](https://github.com/forkwright/aletheia/issues/1747)) ([f64b435](https://github.com/forkwright/aletheia/commit/f64b435f4f4e5174b6f3eebe28a524c4a537c5f6))
* fix 20 writing standard violations ([#1485](https://github.com/forkwright/aletheia/issues/1485)) ([88edf95](https://github.com/forkwright/aletheia/commit/88edf95ad0d6dedd8a7d255681649997a0a2a2a2))
* fix 3 broken links (VENDORING.md, ALETHEIA.md, planning/) ([813fcca](https://github.com/forkwright/aletheia/commit/813fcca0ea34c6e03696fb856af464d3e21a2679))
* fix mechanical writing violations across 22 files ([#1659](https://github.com/forkwright/aletheia/issues/1659)) ([95ab897](https://github.com/forkwright/aletheia/commit/95ab8977e4396e7ef0aa76db9ff8bfa78a29ed60))
* fix QA audit findings — tool counts, test counts, version, banned words ([99dfa3c](https://github.com/forkwright/aletheia/commit/99dfa3cc4dba5e2bea5011dc6a722f5cbbbf301a))
* fix README quickstart tarball instructions and port PLUGINS-DESIGN.md ([#1925](https://github.com/forkwright/aletheia/issues/1925)) ([743709a](https://github.com/forkwright/aletheia/commit/743709a58a387c81947a13bbb4ede7422301de66))
* fix stale architecture, counts, and per-crate CLAUDE.md ([#1922](https://github.com/forkwright/aletheia/issues/1922)) ([cae4a66](https://github.com/forkwright/aletheia/commit/cae4a669b327f9d4fc6116cc315d2c9c697d23fe))
* fix stale references and update status across all planning docs ([#895](https://github.com/forkwright/aletheia/issues/895)) ([37f4388](https://github.com/forkwright/aletheia/commit/37f438855535fa3a398e87fc0d3ca2b22b424a74))
* fix stale references to docs/STANDARDS.md and .claude/rules/ ([b2784fa](https://github.com/forkwright/aletheia/commit/b2784fa94ad40ecaf636d61d3e8775c6c9a00637))
* **mneme:** crate split decomposition plan ([#1272](https://github.com/forkwright/aletheia/issues/1272)) ([bc6f045](https://github.com/forkwright/aletheia/commit/bc6f0457ac672a2bf59b8ade5ba2fb78b65326f2))
* pylon handler reference and project glossary ([#1807](https://github.com/forkwright/aletheia/issues/1807)) ([3df1b33](https://github.com/forkwright/aletheia/commit/3df1b33a2564f4d15a63ebd92af053cf2590c0fb))
* **pylon:** complete CLI and API route documentation ([#961](https://github.com/forkwright/aletheia/issues/961)) ([f7805f0](https://github.com/forkwright/aletheia/commit/f7805f0030fe33a76bbc71403a70459bb90a8a7a))
* remove stale CozoDB vendoring references ([#951](https://github.com/forkwright/aletheia/issues/951)) ([7f684a6](https://github.com/forkwright/aletheia/commit/7f684a628a5a3f42907e0dd508f817475217b96e))
* rename mneme split crates to gnomon names (eidos, krites, graphe, episteme) ([4a4381a](https://github.com/forkwright/aletheia/commit/4a4381af7a78eed19d6c549fdc4828de39594412))
* replace gnomon.md with canonical version ([209db36](https://github.com/forkwright/aletheia/commit/209db3689e93228d66734327127bd4ef8ab66d04))
* **research:** active forgetting system design (R717) ([#1484](https://github.com/forkwright/aletheia/issues/1484)) ([a66aa7a](https://github.com/forkwright/aletheia/commit/a66aa7a9952898417697fe8ea9f0fb9fda27b856))
* **research:** add voice interaction research (R711) ([#1483](https://github.com/forkwright/aletheia/issues/1483)) ([69712ea](https://github.com/forkwright/aletheia/commit/69712ead4a9845ededc6d97e7dd917780a218c53))
* **research:** analyze syntect replacement options ([#1479](https://github.com/forkwright/aletheia/issues/1479)) ([4be3dca](https://github.com/forkwright/aletheia/commit/4be3dcad1efd466830a674cfefcb9b5ad9cb9502))
* **research:** cross-agent knowledge sharing design (R716) ([#1512](https://github.com/forkwright/aletheia/issues/1512)) ([e1d6614](https://github.com/forkwright/aletheia/commit/e1d6614cd6d9ad3b9c805187461e36154237207b))
* **research:** design predictive recall system ([#1480](https://github.com/forkwright/aletheia/issues/1480)) ([3904046](https://github.com/forkwright/aletheia/commit/3904046415597995c0a49404ddcf75c7551b9c04))
* **research:** design vision (image input) support ([#1474](https://github.com/forkwright/aletheia/issues/1474)) ([500747a](https://github.com/forkwright/aletheia/commit/500747ae6869cce153310df92fccb0339665f679))
* **research:** document observability trace architecture ([#1481](https://github.com/forkwright/aletheia/issues/1481)) ([fa259ec](https://github.com/forkwright/aletheia/commit/fa259ec46c1e6c815fb0fc9093cd7fe056af38f5))
* **research:** MCP client support design (R712) ([#1486](https://github.com/forkwright/aletheia/issues/1486)) ([8d1c1e1](https://github.com/forkwright/aletheia/commit/8d1c1e1588c313a7c3e083497d3263b5eb8cd392))
* **research:** multi-provider LLM routing design (R708) ([#1511](https://github.com/forkwright/aletheia/issues/1511)) ([8319898](https://github.com/forkwright/aletheia/commit/831989845de0160dc4f1fc32144822e78d0563fb))
* **research:** R1662 — evaluate Qwen3.5-397B-A17B as local aletheia model ([7478429](https://github.com/forkwright/aletheia/commit/7478429cdb9a6136f7f7dc3a65016f727b967331))
* **research:** R1663 — evaluate Claude Code code review feature for dispatch integration ([ea6e86e](https://github.com/forkwright/aletheia/commit/ea6e86ed8d0225cc5366de0579a7f5a08faec86d))
* **research:** R1664 — mine agency-agents repo for applicable patterns ([63d1334](https://github.com/forkwright/aletheia/commit/63d1334f6ad4ae8f3b071b2c4746cbd9f97901cc))
* **research:** R1665 — evaluate VeraCrypt hidden containers for instance data ([5144440](https://github.com/forkwright/aletheia/commit/514444066dac554d11f95747bcd50776043d4202))
* rewrite lexicon.md in standardized format ([0113e31](https://github.com/forkwright/aletheia/commit/0113e31b88259ddd1402a9724c3dc551ff70a3e6))
* rewrite user-facing docs (README, quickstart, deployment) ([#1661](https://github.com/forkwright/aletheia/issues/1661)) ([60e4a7e](https://github.com/forkwright/aletheia/commit/60e4a7ebccfa25a08c793ef9be070b0f38198d9b))
* **runbook:** add coverage for watchdog, roles, dianoia, melete, config reload ([#1964](https://github.com/forkwright/aletheia/issues/1964)) ([1cfcb59](https://github.com/forkwright/aletheia/commit/1cfcb59b4aab8251c7546e01244fdcae6f98613d)), closes [#1959](https://github.com/forkwright/aletheia/issues/1959)
* **runbook:** add DB inspection, credential rotation, perf, backup/restore, log analysis ([#1749](https://github.com/forkwright/aletheia/issues/1749)) ([7fd719d](https://github.com/forkwright/aletheia/commit/7fd719deb98297e1def6d61744f9ad111763d677)), closes [#1728](https://github.com/forkwright/aletheia/issues/1728) [#1729](https://github.com/forkwright/aletheia/issues/1729)
* split gnomon.md into naming system doc + lexicon registry ([#897](https://github.com/forkwright/aletheia/issues/897)) ([6dd44b4](https://github.com/forkwright/aletheia/commit/6dd44b44d878a39e9085a30b3f6b1caf22bb1e4b))
* split oversized documentation files (2 issues) ([#1229](https://github.com/forkwright/aletheia/issues/1229)) ([ef36ba6](https://github.com/forkwright/aletheia/commit/ef36ba67c33138c4bb78a43fb29d02fc7ac73b46))
* **theatron:** research Dioxus 0.7 Blitz WGPU renderer for desktop ([#1279](https://github.com/forkwright/aletheia/issues/1279)) ([207ba0c](https://github.com/forkwright/aletheia/commit/207ba0ca4f19b9af5da729972186759fc62c8539))
* **theatron:** research Dioxus state architecture for desktop UI ([#1280](https://github.com/forkwright/aletheia/issues/1280)) ([337ff79](https://github.com/forkwright/aletheia/commit/337ff79dd9fda5ceacd66546812a8d13c80911be))
* **theatron:** research markdown rendering for Dioxus desktop ([#1281](https://github.com/forkwright/aletheia/issues/1281)) ([4f78a92](https://github.com/forkwright/aletheia/commit/4f78a92a64f6308ba1fe8a5f26987627ef9f025f))
* **theatron:** research SSE and streaming architecture for Dioxus desktop ([#1283](https://github.com/forkwright/aletheia/issues/1283)) ([e8d4d48](https://github.com/forkwright/aletheia/commit/e8d4d488c28c3388d98ceb5c9391f06688e82942))
* **theatron:** theatron-core extraction plan ([#1274](https://github.com/forkwright/aletheia/issues/1274)) ([8e96f59](https://github.com/forkwright/aletheia/commit/8e96f598d636882c82f7c800beef756f949a2a0f))
* **thesauros:** rewrite PACKS.md with TOML examples and starter pack (P334) ([#933](https://github.com/forkwright/aletheia/issues/933)) ([ae1738b](https://github.com/forkwright/aletheia/commit/ae1738bd6d3d48dfae8efdd32c5151e012c4759d)), closes [#772](https://github.com/forkwright/aletheia/issues/772)
* update CONFIGURATION.md with missing sections ([#1352](https://github.com/forkwright/aletheia/issues/1352)) ([f88e223](https://github.com/forkwright/aletheia/commit/f88e22338b036fce1e8bd289cf7ba96a0787d2e1)), closes [#1322](https://github.com/forkwright/aletheia/issues/1322)
* update stale standards references to standards/ paths ([#950](https://github.com/forkwright/aletheia/issues/950)) ([5c3318c](https://github.com/forkwright/aletheia/commit/5c3318cf14fb9bdb99ac726efc5dda50073fc03b))
* Wave 10+ feature research ([#1457](https://github.com/forkwright/aletheia/issues/1457), [#1465](https://github.com/forkwright/aletheia/issues/1465), [#1466](https://github.com/forkwright/aletheia/issues/1466), [#1470](https://github.com/forkwright/aletheia/issues/1470), [#1471](https://github.com/forkwright/aletheia/issues/1471), [#1472](https://github.com/forkwright/aletheia/issues/1472)) ([#1792](https://github.com/forkwright/aletheia/issues/1792)) ([8b8e24a](https://github.com/forkwright/aletheia/commit/8b8e24af5e7f808df8f800d3465532670d678e40))

## [0.13.1](https://github.com/forkwright/aletheia/compare/v0.13.0...v0.13.1) (2026-03-23)


### Features

* **cli:** memory management subcommands — check, consolidate, sample, dedup, patterns ([#1940](https://github.com/forkwright/aletheia/issues/1940)) ([29dbc97](https://github.com/forkwright/aletheia/commit/29dbc97632cd75fa88370c4ad831d14bee7b66e5))
* **daemon:** watchdog process monitor with auto-recovery ([#1933](https://github.com/forkwright/aletheia/issues/1933)) ([947f51c](https://github.com/forkwright/aletheia/commit/947f51c4b626e70b6f667a0490917cb0e6f015e5))
* **dianoia:** multi-level parallel research ([#1950](https://github.com/forkwright/aletheia/issues/1950)) ([57e1f08](https://github.com/forkwright/aletheia/commit/57e1f08742c1952412aa69bf935e159b43554ea6)), closes [#1883](https://github.com/forkwright/aletheia/issues/1883)
* **dianoia:** state reconciler and verification workflow ([#1946](https://github.com/forkwright/aletheia/issues/1946)) ([51f361a](https://github.com/forkwright/aletheia/commit/51f361a756189cb97b02048b0b59654247e0302e))
* **dianoia:** stuck detection and handoff protocol ([#1926](https://github.com/forkwright/aletheia/issues/1926)) ([ac231a7](https://github.com/forkwright/aletheia/commit/ac231a79b5b2fcef08c2ddf3eb5302ea592b39eb)), closes [#1869](https://github.com/forkwright/aletheia/issues/1869) [#1870](https://github.com/forkwright/aletheia/issues/1870)
* **eval:** cognitive evaluation framework ([#1953](https://github.com/forkwright/aletheia/issues/1953)) ([1d267d6](https://github.com/forkwright/aletheia/commit/1d267d63984b09959d0645ed44160a3273a11abe)), closes [#1885](https://github.com/forkwright/aletheia/issues/1885)
* **hermeneus:** complexity-based model routing ([#1928](https://github.com/forkwright/aletheia/issues/1928)) ([b73c672](https://github.com/forkwright/aletheia/commit/b73c6720f2bf11e9b2e8af19ff6adb55e30dd4c4)), closes [#1875](https://github.com/forkwright/aletheia/issues/1875)
* **melete:** similarity pruning and contradiction detection ([#1929](https://github.com/forkwright/aletheia/issues/1929)) ([f57428d](https://github.com/forkwright/aletheia/commit/f57428d241b10dd7dc0ec6953f0fec9b2076d197))
* **metrics:** add Prometheus metrics to 7 crates ([#1966](https://github.com/forkwright/aletheia/issues/1966)) ([5bb630c](https://github.com/forkwright/aletheia/commit/5bb630cf4b85d862594617ab091b412713cde3f5))
* **mneme:** temporal decay algorithms and serendipity engine ([#1941](https://github.com/forkwright/aletheia/issues/1941)) ([88585a4](https://github.com/forkwright/aletheia/commit/88585a459e42f3cd9649ed3f2f6f896e06857e05))
* **nous:** competence tracking and uncertainty quantification ([#1938](https://github.com/forkwright/aletheia/issues/1938)) ([2aed0ae](https://github.com/forkwright/aletheia/commit/2aed0ae5d773032ff74f7440d6ab4951ce05b2a3))
* **nous:** pattern-based loop detection and working state management ([#1936](https://github.com/forkwright/aletheia/issues/1936)) ([ca7bd93](https://github.com/forkwright/aletheia/commit/ca7bd93be54491bd52b10277d5e2518ee35d7b9a)), closes [#1872](https://github.com/forkwright/aletheia/issues/1872) [#1881](https://github.com/forkwright/aletheia/issues/1881)
* **nous:** sub-agent role prompts — coder, researcher, reviewer, explorer, runner ([#1947](https://github.com/forkwright/aletheia/issues/1947)) ([7df6f8c](https://github.com/forkwright/aletheia/commit/7df6f8ca3590de85364140ca2f79d5bd4dc0581e))
* **organon:** tool reversibility tracking and custom slash commands ([#1935](https://github.com/forkwright/aletheia/issues/1935)) ([8b7247f](https://github.com/forkwright/aletheia/commit/8b7247f75fbb3a2114c51f4a019cc9fbb9545dd2))
* **theatron:** implement desktop views with real API integration ([#1900](https://github.com/forkwright/aletheia/issues/1900)) ([01a8314](https://github.com/forkwright/aletheia/commit/01a8314531bfbc4c2dadbb8a92712e6465af4c58))
* **tui:** CC input keybindings, queued messages, and image paste ([#1952](https://github.com/forkwright/aletheia/issues/1952)) ([a05824f](https://github.com/forkwright/aletheia/commit/a05824f4cc08a2969d0f9db01ee3052bba233a55)), closes [#1892](https://github.com/forkwright/aletheia/issues/1892) [#1893](https://github.com/forkwright/aletheia/issues/1893)
* **tui:** CC-aligned rendering engine, streaming, and tool cards ([#1949](https://github.com/forkwright/aletheia/issues/1949)) ([5aff911](https://github.com/forkwright/aletheia/commit/5aff9114adbb7b94c7a78f08dc6a26bd3c349e91))
* **tui:** context budget visualization and distillation indicators ([#1927](https://github.com/forkwright/aletheia/issues/1927)) ([b639acd](https://github.com/forkwright/aletheia/commit/b639acd1466cbfa65e68b8dbdb3540d4962cc4a5))
* **tui:** execution progress indicators and decision cards ([#1939](https://github.com/forkwright/aletheia/issues/1939)) ([fbd9022](https://github.com/forkwright/aletheia/commit/fbd9022fe94d078f0bdaac5a32be0d2c24fae920))
* **tui:** file editor with syntax highlighting and tabs ([#1951](https://github.com/forkwright/aletheia/issues/1951)) ([1d3da97](https://github.com/forkwright/aletheia/commit/1d3da978af38d506c1e53dfed3b897f993f5ed06)), closes [#1859](https://github.com/forkwright/aletheia/issues/1859)
* **tui:** halt, stall detection, ops pane cleanup, session abstraction ([#1931](https://github.com/forkwright/aletheia/issues/1931)) ([1a63299](https://github.com/forkwright/aletheia/commit/1a63299fb70fbaa0be2b87678e30cf4fa99fa7fc))
* **tui:** knowledge graph visualization and 3D architecture doc ([#1955](https://github.com/forkwright/aletheia/issues/1955)) ([98a3576](https://github.com/forkwright/aletheia/commit/98a357628b2a869a66cb2151c3e7fd12b097c698))
* **tui:** message shading, retrospective view, planning dashboard (closes [#1958](https://github.com/forkwright/aletheia/issues/1958), closes [#1867](https://github.com/forkwright/aletheia/issues/1867), closes [#1856](https://github.com/forkwright/aletheia/issues/1856)) ([65c86b1](https://github.com/forkwright/aletheia/commit/65c86b16287322ca47ceb730894948612d9151b5))
* **tui:** metrics dashboard with token usage and service health ([#1945](https://github.com/forkwright/aletheia/issues/1945)) ([dd3a914](https://github.com/forkwright/aletheia/commit/dd3a9146bc115f7f08761a7591625c2878408129))
* **tui:** setup wizard for first-run instance initialization ([#1943](https://github.com/forkwright/aletheia/issues/1943)) ([61685b3](https://github.com/forkwright/aletheia/commit/61685b3a635adafe07594e16063386ba061f2101))
* **tui:** slash command autocomplete and notification system ([#1934](https://github.com/forkwright/aletheia/issues/1934)) ([ef81053](https://github.com/forkwright/aletheia/commit/ef810533a58d4ea851631f42e3de779b19829682))
* **tui:** tool approval dialog and category icons ([#1930](https://github.com/forkwright/aletheia/issues/1930)) ([26c135b](https://github.com/forkwright/aletheia/commit/26c135b349cedd50e4f8e740e5e0123fc1ca3e35))


### Bug Fixes

* **aletheia,daemon,dianoia,thesauros,eval:** resolve all kanon lint violations ([#1918](https://github.com/forkwright/aletheia/issues/1918)) ([ae53e2d](https://github.com/forkwright/aletheia/commit/ae53e2d786fd8c323e4f362116cc2286776379a7))
* **aletheia:** resolve all non-Rust kanon lint violations ([#1916](https://github.com/forkwright/aletheia/issues/1916)) ([aadeb64](https://github.com/forkwright/aletheia/commit/aadeb640abfe4c943313e879f42cd2db57037a48))
* **aletheia:** resolve feature-gated compilation errors from Fact decomposition ([7e339ee](https://github.com/forkwright/aletheia/commit/7e339eef89cb9864c707c063191af83309548267))
* **ci:** exclude theatron-desktop from workspace (GTK deps break CI) ([b9dcc0d](https://github.com/forkwright/aletheia/commit/b9dcc0d6957dce4286a6097547915eb0f296efc9))
* **clippy:** resolve remaining clippy errors for release gate ([1676881](https://github.com/forkwright/aletheia/commit/16768811c53588ea1481d9191fa9c32100c72adb))
* **graphe,episteme,krites,mneme:** resolve all kanon lint violations ([#1920](https://github.com/forkwright/aletheia/issues/1920)) ([5347732](https://github.com/forkwright/aletheia/commit/534773221791cd9237ca0587feda7050679356f1))
* **koina,eidos,taxis,symbolon:** resolve all kanon lint violations ([#1917](https://github.com/forkwright/aletheia/issues/1917)) ([8bd5749](https://github.com/forkwright/aletheia/commit/8bd57496b5d83f8c346d3df5f7c97ebdc5e383aa))
* **nous,hermeneus,organon,melete:** resolve all kanon lint violations ([#1921](https://github.com/forkwright/aletheia/issues/1921)) ([b9c6a59](https://github.com/forkwright/aletheia/commit/b9c6a59054982b66c817224ee0e09cd98e7be3c7))
* **nous:** replace .expect() with match in roles test ([f489874](https://github.com/forkwright/aletheia/commit/f489874d3d85e047fa2c020fa5fe598798982e0c))
* **organon,episteme,koina:** resolve expect_used and as_conversions lint violations ([#1957](https://github.com/forkwright/aletheia/issues/1957)) ([4ef84b9](https://github.com/forkwright/aletheia/commit/4ef84b93811fc5fa477ffb61feb3cb57aea7cabb))
* pre-release gate fixes — fmt, view_nav match, workflow sync ([3cd5df6](https://github.com/forkwright/aletheia/commit/3cd5df65e423ac8d6dc85b1456c03333bc80bbfe))
* **pylon,episteme:** cap query limit, tighten episteme visibility (closes [#1963](https://github.com/forkwright/aletheia/issues/1963), closes [#1962](https://github.com/forkwright/aletheia/issues/1962)) ([e9b387d](https://github.com/forkwright/aletheia/commit/e9b387d07ea3173246f7f50821bd87f53cff2b85))
* **pylon,theatron,diaporeia:** resolve all kanon lint violations ([#1919](https://github.com/forkwright/aletheia/issues/1919)) ([595d148](https://github.com/forkwright/aletheia/commit/595d1488b4b54d94e8e71ffea413bced3a25c12a))
* **pylon:** resolve rustdoc and unfulfilled lint expectation errors ([99c35ff](https://github.com/forkwright/aletheia/commit/99c35ffeb68043db346a71cc69e4a2a2b23a2898))
* resolve 6 code quality audit findings ([#1923](https://github.com/forkwright/aletheia/issues/1923)) ([17ec00d](https://github.com/forkwright/aletheia/commit/17ec00ddade286d62783c0dc55ec783a085f6751))
* resolve clippy lint violations across workspace ([9fc0ae8](https://github.com/forkwright/aletheia/commit/9fc0ae8eefcaabd8e39d1cc26313d0749b64943a))
* **security:** resolve audit findings — size limits, ProcessGuard, struct decomposition ([#1924](https://github.com/forkwright/aletheia/issues/1924)) ([6743a82](https://github.com/forkwright/aletheia/commit/6743a82804563c72c05eb522b9790afaaf4ce99a))
* **security:** resolve CodeQL cleartext alerts (closes [#1956](https://github.com/forkwright/aletheia/issues/1956)) ([7b068ab](https://github.com/forkwright/aletheia/commit/7b068ab2348f0f6fb945c56ea9eb435e71fa12b1))
* **test:** add test-core/test-full feature tiers ([#1895](https://github.com/forkwright/aletheia/issues/1895)) ([#1937](https://github.com/forkwright/aletheia/issues/1937)) ([5dc57f8](https://github.com/forkwright/aletheia/commit/5dc57f8d842c817a39602c2cca35ea2472b36c94))
* **tests:** resolve lint batch 4 — unwrap, coverage, perms, timeouts ([#1942](https://github.com/forkwright/aletheia/issues/1942)) ([1082945](https://github.com/forkwright/aletheia/commit/108294542143537aab6c9ff253b7cb3deed90c90)), closes [#1915](https://github.com/forkwright/aletheia/issues/1915)
* **test:** wire test-core feature to enable engine tests ([#1965](https://github.com/forkwright/aletheia/issues/1965)) ([bfb074b](https://github.com/forkwright/aletheia/commit/bfb074b534345354792ec92ba309e2d0e24f3b77))
* **tui:** cursor style and raw JSON tool call rendering on reload ([#1932](https://github.com/forkwright/aletheia/issues/1932)) ([bdeefe0](https://github.com/forkwright/aletheia/commit/bdeefe08aecf2034fb5dea1c00befdfce0f4f7c6))


### Refactoring

* convert #[allow(dead_code)] to #[expect] with reasons across workspace ([af37f5a](https://github.com/forkwright/aletheia/commit/af37f5a4555794fa57f34cf53fd777977a5b2b21))
* **mneme:** extract Datalog engine into krites crate ([#1899](https://github.com/forkwright/aletheia/issues/1899)) ([dbaae76](https://github.com/forkwright/aletheia/commit/dbaae7632cf3a5fd7e93946d34430350eb3e05cd))
* **mneme:** extract graphe and episteme crates ([#1901](https://github.com/forkwright/aletheia/issues/1901)) ([bbbdd39](https://github.com/forkwright/aletheia/commit/bbbdd396c1b0ee3a6803977a2d3174b2987c939a))
* **mneme:** extract shared knowledge types into eidos crate ([#1896](https://github.com/forkwright/aletheia/issues/1896)) ([3e28fcc](https://github.com/forkwright/aletheia/commit/3e28fcccd6cf3ac12c72c30f00660ecb6d634c15))


### Documentation

* fix QA audit findings — tool counts, test counts, version, banned words ([99dfa3c](https://github.com/forkwright/aletheia/commit/99dfa3cc4dba5e2bea5011dc6a722f5cbbbf301a))
* fix README quickstart tarball instructions and port PLUGINS-DESIGN.md ([#1925](https://github.com/forkwright/aletheia/issues/1925)) ([743709a](https://github.com/forkwright/aletheia/commit/743709a58a387c81947a13bbb4ede7422301de66))
* fix stale architecture, counts, and per-crate CLAUDE.md ([#1922](https://github.com/forkwright/aletheia/issues/1922)) ([cae4a66](https://github.com/forkwright/aletheia/commit/cae4a669b327f9d4fc6116cc315d2c9c697d23fe))
* **runbook:** add coverage for watchdog, roles, dianoia, melete, config reload ([#1964](https://github.com/forkwright/aletheia/issues/1964)) ([1cfcb59](https://github.com/forkwright/aletheia/commit/1cfcb59b4aab8251c7546e01244fdcae6f98613d)), closes [#1959](https://github.com/forkwright/aletheia/issues/1959)
