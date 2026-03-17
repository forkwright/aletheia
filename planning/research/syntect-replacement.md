# R707: Syntect Replacement Options

## Question

Syntect pulls in a large dependency tree including C FFI bindings (Oniguruma), unmaintained crates (yaml-rust, bincode), and rarely-used features (HTML output, walkdir, plist loading). Is there a lighter alternative, or can syntect's footprint be reduced while preserving the highlighting quality needed for the TUI?

## Findings

### Current Usage Audit

Syntect is used in exactly one location: `crates/theatron/tui/src/highlight.rs` (78 lines of production code).

**Features actually used:**
- `SyntaxSet::load_defaults_newlines()` for bundled syntax definitions
- `ThemeSet::load_defaults()` for bundled themes (base16-ocean light/dark only)
- `HighlightLines::highlight_line()` for per-line tokenization
- `FontStyle` checking (BOLD, ITALIC)
- `LinesWithEndings` iterator
- Language lookup by token and extension with plaintext fallback

**Features NOT used:**
- HTML output (`html` feature)
- Custom theme/syntax loading from files (`yaml-load`, `plist-load`, `walkdir`)
- Binary dump creation (`dump-create`)
- Custom syntax set construction
- Any dynamic theme loading at runtime

**Languages observed in tests:** Rust, Python, plaintext fallback. The TUI renders LLM responses containing fenced code blocks, so any language is possible, but Rust/Python/TOML/JSON/Bash cover the majority of real usage.

### Current Dependency Cost

Syntect 5.3.0 with default features pulls 13 direct dependencies and ~35 unique transitive crates. Key cost centers:

| Dependency | Unique to syntect? | Notes |
|---|---|---|
| `onig` + `onig_sys` | No (shared with `tokenizers`) | C FFI, builds Oniguruma from source |
| `yaml-rust` | Yes | Unmaintained (RUSTSEC-2024-0320), used for .sublime-syntax loading |
| `bincode` v1 | Yes | Unmaintained (RUSTSEC-2025-0141), used for dump serialization |
| `plist` | Yes | XML property list parsing for .tmTheme files |
| `walkdir` | Yes | Filesystem traversal for syntax/theme discovery |
| `flate2` | No (shared with tower-http, daemon) | Compression for binary dumps |
| `serde`, `serde_json` | No (used widely) | Serialization |
| `thiserror` | Yes | syntect's internal errors (not our standard; we use snafu) |
| `once_cell` | No (used widely) | Lazy initialization |

**Syntect-exclusive crates:** yaml-rust, bincode, plist, walkdir, thiserror (~8 crates including transitives like linked-hash-map, time, quick-xml, base64).

**Security advisories bypassed:** Two entries in `deny.toml` exist solely for syntect's transitive deps (yaml-rust, bincode).

### Option 1: Syntect with `default-fancy` (Minimal Change)

Switch from the default Oniguruma engine to the pure-Rust `fancy-regex` engine.

**Configuration:** `syntect = { version = "5", default-features = false, features = ["default-fancy"] }`

**What changes:**
- Drops `onig` + `onig_sys` (C FFI build). However, `onig` stays in the workspace because `tokenizers` also depends on it. No net reduction in workspace build.
- Adds `fancy-regex` (pure Rust, already a transitive dep via `regex`).
- All other deps remain: yaml-rust, bincode, plist, walkdir still pulled by default-fancy.

**Trade-offs:**
- (+) Eliminates syntect's C FFI path, simplifying the theatron-tui build
- (+) No code changes needed
- (-) ~50% slower highlighting (fancy-regex vs onig). Negligible for code blocks in a TUI.
- (-) Extremely slow in debug builds (Rust regex in debug mode). Mitigated by `[profile.dev.package."*"] opt-level = 2` which we already set.
- (-) Does not remove yaml-rust or bincode advisories
- (-) onig remains in workspace via tokenizers

**Effort:** ~30 minutes. One-line Cargo.toml change + verify tests pass.

### Option 2: Syntect with Minimal Features

Disable all features we don't use, keeping only what `highlight.rs` needs.

**Configuration:**
```toml
syntect = { version = "5", default-features = false, features = [
    "default-syntaxes",
    "default-themes",
    "parsing",
    "regex-fancy",
] }
```

**What this drops:**
- `onig` + `onig_sys` (C FFI)
- `html` (HTML output)
- `yaml-load` (drops `yaml-rust`) -- eliminates RUSTSEC-2024-0320
- `plist-load` (drops `plist`, `quick-xml`, `time`, `base64`) -- no runtime theme loading
- `dump-create` (we only load defaults, never create dumps)
- `walkdir` (no filesystem discovery)

**What remains:**
- `bincode` + `flate2` (still needed by `dump-load` for loading bundled default syntax/theme sets)
- `serde`, `serde_json` (serialization for dump loading)
- `fancy-regex` (pure Rust regex)
- `fnv`, `once_cell` (internal)

**Trade-offs:**
- (+) Removes yaml-rust advisory entirely
- (+) Drops ~6 syntect-exclusive crates (plist, walkdir, yaml-rust, linked-hash-map, time, quick-xml, base64, same-file)
- (+) No code changes needed (we only use defaults loaded from bundled dumps)
- (+) No C FFI in the syntect path
- (-) bincode advisory remains (needed for dump-load of bundled defaults)
- (-) ~50% slower highlighting (fancy-regex). Still negligible for TUI.
- (-) Cannot load custom .sublime-syntax or .tmTheme files at runtime (not needed today)

**Effort:** ~1 hour. Feature flag configuration + verify no runtime failures + update deny.toml.

### Option 3: tree-sitter-highlight

Replace syntect with tree-sitter's highlighting library.

**Architecture:** tree-sitter-highlight provides event-based highlighting. You supply a compiled grammar (C code linked as a library) and highlight query files (S-expression patterns). It returns `HighlightEvent` variants (`Source`, `HighlightStart`, `HighlightEnd`) that you map to styles.

**Required per language:**
- `tree-sitter` (core library, ~1 crate)
- `tree-sitter-highlight` (highlighting layer)
- Per-language grammar crate: `tree-sitter-rust`, `tree-sitter-python`, etc. Each is a C parser compiled at build time.
- Per-language highlight query files (typically vendored from the grammar repo)

**Dependency impact:**
- Drops all syntect deps (yaml-rust, bincode, plist, etc.)
- Adds `tree-sitter` (C library, built from source) + each language grammar (C code)
- Each grammar adds 100KB-2MB of C code compiled at build time
- For 5-6 languages: ~2-5MB of C source total

**Trade-offs:**
- (+) More accurate highlighting (full parse tree vs regex patterns)
- (+) No unmaintained deps (yaml-rust, bincode)
- (+) tree-sitter is actively maintained, used by Helix, Zed, Neovim
- (-) Each language is a separate C compilation unit, increasing build time
- (-) No bundled "all languages" default; must explicitly add each language
- (-) Highlight queries must be vendored or loaded from files
- (-) Significant API redesign: event-based instead of line-based
- (-) Plaintext fallback requires explicit handling
- (-) Theme mapping is manual (no built-in theme format)
- (-) `tree-sitter-highlight` is pre-1.0 (currently 0.25.x)
- (-) MSRV unverified for tree-sitter 0.25 against our Rust 1.94

**Effort:** ~2-3 days. New highlight module, vendored queries, theme mapping, testing per language.

### Option 4: inkjet (tree-sitter wrapper)

inkjet bundles 70+ tree-sitter grammars into a single crate with a simpler API.

**Critical finding:** The repository was archived in September 2025. It is no longer maintained.

**Trade-offs:**
- (+) Simpler API than raw tree-sitter-highlight
- (+) Batteries-included language support
- (-) **Archived/unmaintained** -- disqualifying under our dependency policy
- (-) 23M+ lines of bundled C code, massive binary size increase
- (-) All languages compiled even if unused (unless feature-gated individually)

**Recommendation:** Do not use. Unmaintained.

### Option 5: Minimal Custom Highlighter

Build a simple keyword-based highlighter for the languages we care about.

**Approach:** Regex-based token matching for keywords, strings, comments, and numbers. Map to a fixed set of styles.

**Trade-offs:**
- (+) Zero external dependencies for highlighting
- (+) Full control over output format
- (+) Fast compilation, minimal binary size
- (-) Poor highlighting quality (no scope nesting, no multi-line strings, no context-aware coloring)
- (-) Must maintain per-language rules manually
- (-) Each new language requires custom regex rules
- (-) Significant regression in UX quality vs syntect

**Effort:** ~3-5 days for Rust + Python + a few others. Ongoing maintenance per language.

### Option 6: No Highlighting

Remove syntax highlighting entirely. Render code blocks as monochrome text.

**Trade-offs:**
- (+) Drops syntect and all its deps entirely
- (+) Simplifies the rendering pipeline
- (-) Significant UX regression: code blocks lose readability
- (-) Users comparing to tools like bat, delta, or any modern terminal app will notice

**Effort:** ~1 hour. Delete highlight.rs, update markdown.rs to emit plain text.

### Dependency Comparison

| Option | Deps added | Deps removed | C FFI | Security advisories |
|---|---|---|---|---|
| Status quo | 0 | 0 | onig (shared) | yaml-rust, bincode |
| 1: fancy-regex | fancy-regex | onig (syntect only) | onig stays (tokenizers) | yaml-rust, bincode |
| 2: Minimal features | fancy-regex | ~8 crates | onig stays (tokenizers) | bincode only |
| 3: tree-sitter | tree-sitter + grammars | all syntect deps | tree-sitter C lib + grammars | None |
| 4: inkjet | **Archived** | -- | -- | -- |
| 5: Custom | 0 | all syntect deps | None | None |
| 6: No highlighting | 0 | all syntect deps | None | None |

### Compile Time Estimate

Precise measurement requires benchmarking, but directional estimates:

- **Option 2** saves ~5-10s on clean build (fewer crates to compile, no plist/walkdir/yaml-rust)
- **Option 3** adds ~10-30s on clean build (C grammar compilation) but tree-sitter caches well
- **Option 5/6** save ~10-15s on clean build (no syntect at all)
- Incremental builds are unaffected for all options (syntect only rebuilds when its deps change)

## Recommendations

### Primary: Option 2 (Syntect with Minimal Features)

**Justification:**
1. Zero code changes to `highlight.rs`. The API surface we use is identical.
2. Eliminates the yaml-rust advisory entirely.
3. Drops ~8 syntect-exclusive crates from the dependency tree.
4. Preserves full highlighting quality with 100+ bundled languages.
5. Low risk, low effort (~1 hour).
6. The remaining bincode advisory is acceptable: it's used only for loading pre-built binary dumps (no untrusted input).

### Secondary: Option 3 (tree-sitter-highlight) as a future migration

tree-sitter is the direction the ecosystem is moving (Helix, Zed, Neovim all use it). Consider migrating when:
- theatron-tui needs language-aware features beyond highlighting (folding, indentation, bracket matching)
- tree-sitter-highlight reaches 1.0
- The tokenizers crate drops onig (eliminating the shared C FFI dependency)

### Not recommended:
- **Option 4 (inkjet):** Archived. Dead project.
- **Option 5 (custom):** High effort, poor quality, ongoing maintenance burden.
- **Option 6 (no highlighting):** UX regression with no technical benefit beyond dependency removal.

## Gotchas

1. **Cargo feature unification:** If any workspace crate depends on syntect without `default-features = false`, the default onig features get pulled back in. Verify with `cargo tree -f '{p} {f}' -p syntect` after the change.

2. **Debug build performance:** fancy-regex is slow in unoptimized builds. Our `[profile.dev.package."*"] opt-level = 2` already mitigates this, but developers should be aware.

3. **dump-load requires bincode:** The bundled default syntax/theme sets are serialized with bincode. Removing dump-load means losing `load_defaults()` and needing to parse .sublime-syntax files at startup (which requires yaml-load). The binary dump approach is the right trade-off.

4. **onig stays regardless:** tokenizers (used by mneme for embeddings) depends on onig independently. Removing syntect's onig usage does not eliminate onig from the workspace build.

5. **tree-sitter grammar MSRV:** tree-sitter grammar crates have varying MSRV policies. Some lag behind. Verify compatibility before migrating.

6. **syntect-tui exists:** The `syntect-tui` crate provides syntect-to-ratatui conversion, but our current 78-line implementation is simpler and more tailored. Adding a dependency to save 20 lines violates the "10 lines, write it" policy.

## References

- syntect repo: github.com/trishume/syntect
- syntect feature flags: `default-fancy`, `default-syntaxes`, `default-themes`, `parsing`, `regex-fancy`
- RUSTSEC-2024-0320 (yaml-rust unmaintained): rustsec.org/advisories/RUSTSEC-2024-0320
- RUSTSEC-2025-0141 (bincode unmaintained): rustsec.org/advisories/RUSTSEC-2025-0141
- tree-sitter-highlight docs: docs.rs/tree-sitter-highlight
- inkjet (archived): github.com/Colonial-Dev/inkjet
- fancy-regex: github.com/fancy-regex/fancy-regex
- Current implementation: `crates/theatron/tui/src/highlight.rs`
- deny.toml advisory overrides: `deny.toml` lines for RUSTSEC-2024-0320, RUSTSEC-2025-0141
