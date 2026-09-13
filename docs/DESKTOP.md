# Desktop application

`proskenion` is a Dioxus desktop UI for Aletheia providing chat, planning, memory browsing, metrics, and ops views.

## System dependencies

The desktop crate uses Dioxus with a WebView backend (wry, over GTK3 and
webkit2gtk on Linux), which requires GTK3, webkit2gtk, libxdo, and librsvg
system libraries. This list is derived from — and must be kept identical to —
the packages `.github/workflows/desktop.yml` installs for desktop CI; there is
no `nix flake check`-style enforcement across the two, so re-derive from that
workflow's `Install GTK/WebKit system dependencies` step if this drifts.

**Debian/Ubuntu:**

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev librsvg2-dev
```

**Fedora:**

```bash
sudo dnf install webkit2gtk4.1-devel gtk3-devel libxdo-devel librsvg2-devel
```

**macOS:** No additional dependencies. WebKit is bundled with the OS.

**Nix:** `nix develop .#proskenion` (see below) provisions the same GTK3/
webkit2gtk/libxdo/librsvg stack via `flake.nix`'s `gtkWebkitNativeDeps`,
rather than system packages.

## Build

`proskenion` is a full member of the root cargo workspace, but not a
*default* one (see `[workspace].default-members` in the root `Cargo.toml`):
its GTK/webkit2gtk dependency set needs dedicated system packages, so a bare
`cargo build`/`cargo check` (no `-p`/`--workspace`) on a GTK-less machine
must not try to compile it. Opt in explicitly with `-p proskenion` or
`--workspace`. The path-filtered desktop CI job (`.github/workflows/desktop.yml`)
installs the GTK/webkit2gtk system packages and compiles, lints, and tests
`proskenion` this same way; `gate-attestation.yml`'s full-workspace jobs
install them too, since their `--workspace` commands compile it regardless
of default-members.

For the standard local install flow, run:

```bash
scripts/install-proskenion.sh
```

The installer verifies Linux GTK/webkit2gtk system libraries, builds the release binary, and installs `proskenion` to `~/.cargo/bin/`.

Build it directly with `-p`, from the repo root (no `--manifest-path` needed,
though the installer above still passes one for explicitness):

```bash
cargo build -p proskenion
cargo build -p proskenion --release
```

For a Nix development environment, allow direnv or enter the shell directly:

```bash
direnv allow
nix develop .#proskenion
```

The flake package and shell both target the `proskenion` package within the
root workspace (`flake.nix` derives its name/version from the root manifest
now that `proskenion`'s own `[package]` fields inherit via
`version.workspace = true` rather than declaring their own).

## Contract and smoke checks

`proskenion` is a non-default workspace member, so acceptance uses two focused checks instead of a full GUI driver.

> Maintainer/CI variant. Prerequisites: install the pinned toolchain (`rustup toolchain install 1.94`) and build the desktop binary first with `scripts/install-proskenion.sh`, which places `proskenion` on `~/.cargo/bin`. The contract test compiles from a fresh checkout; the smoke invocation below needs that installed binary.

```bash
cargo +1.94 test -p integration-tests --features test-core proskenion_contract -- --nocapture

bash -n scripts/smoke-proskenion.sh
scripts/smoke-proskenion.sh --proskenion-binary ~/.cargo/bin/proskenion
```

The `proskenion_contract` integration test exercises the protocol surface the app consumes: agent list/status/tool envelopes, knowledge browse endpoints, metrics/cost/token envelopes, session create/resolve/list/history, and `POST /api/v1/sessions/stream` SSE event names, terminal events, and JSON field shape. If it fails, file the failure as a server/client runtime-contract mismatch and include the assertion text, full response body printed by the test, endpoint, and expected proskenion field or event name.

The smoke script starts a local server when no `--server-url` is supplied, or connects to the supplied URL. Use the default gateway port when targeting an already running local server:

```bash
scripts/smoke-proskenion.sh --server-url http://127.0.0.1:18789 --proskenion-binary ~/.cargo/bin/proskenion
```

The script writes a temporary desktop config, uses `xvfb-run` when no `DISPLAY` is available, enforces a bounded runtime, captures logs, and fails on known display/startup/connectivity patterns. Missing Xvfb or a missing `proskenion` binary exits with a clear skip status instead of silently passing.

## Pin discipline

`proskenion` inherits `[workspace.dependencies]`, `[workspace.package]`, and
its safety/deny-level lints from the root `Cargo.toml` — its `themelion`,
`skeue`, `gramma`, `bathron`, and `keryx` theatron dependencies are
`{ workspace = true }` entries, the same single tag pin every other
workspace member uses. There is no separate pin file to keep in sync and no
pin-check script to run; a re-pin of the theatron tag in the root
`[workspace.dependencies]` block updates `proskenion` automatically, the
same as it does for `skene`/`koilon`.

## Default-members, not workspace exclusion

`proskenion` is a full workspace member (`[workspace].members` in the root
`Cargo.toml`) but is left out of `[workspace].default-members`, so a bare
`cargo build`/`cargo check`/`cargo test` on a GTK-less machine never tries to
compile it:

1. **GTK-less-by-default.** GTK3 and webkit2gtk are installed by the
   path-filtered desktop CI job and by every full-workspace (`--workspace`)
   job in `gate-attestation.yml` and `bench-gate.yml`'s compile-check job
   excludes it instead (it has no benches of its own). Contributors and CI
   jobs that never pass `-p proskenion`/`--workspace` stay GTK-free.
2. **Dependency advisories are triaged, not avoided.** GTK/Dioxus-desktop
   bindings pull in a handful of unmaintained-but-not-vulnerable crates and
   one NCSA-licensed transitive dependency; `deny.toml` allowlists each with
   a WHY comment (forkwright/aletheia#4726) rather than hiding the whole
   crate from cargo-deny's view.
3. **No hand-maintained version literal.** `proskenion`'s `[package]` fields
   (`version`, `edition`, `license`, `rust-version`) are all
   `{ workspace = true }` — it is same-tree by construction (it consumes
   `skene` and `koina` by `path`), so tracking its own version by hand was
   pure duplicated bookkeeping. A root version bump now updates it for free.

## Architecture

The desktop crate depends on `skene` for domain types and (as of #4565) event
parsing — `api/sse.rs` re-exports `skene::api::sse::parse_sse_event` directly.
As of #4925, per-turn streaming and health checks are owned by `skene`
end-to-end too: `skene::api::streaming::stream_message` takes a
`CancellationToken` directly (the one capability gap that used to justify a
local connect-and-poll loop here), and `skene::api::health` is the sole
health-parsing implementation. `proskenion` no longer carries local copies of
either. It connects to a running Aletheia server over HTTP, the same as the
TUI.

As of waves 3a-3c (#7230, #7232, proskenion-skene-wave3c), every VIEW-layer
call site (planning, ops, credentials, metrics, memory, sessions, files,
chat) reads and writes through `skene::api::client::ApiClient` typed
methods and route-contract-backed DTOs, not raw `reqwest` or hand-built
`/api/v1/` strings — `scripts/check-client-boundary.py` mechanically
enforces this as a per-file ceiling that may only shrink
(`scripts/client-boundary-baseline.toml`).

4 infrastructure files below the view layer still build requests locally,
each blocked on a skene primitive that does not exist yet rather than on
migration effort — tracked precisely in #7251:

- `api/client.rs`'s `authenticated_client()` family builds a raw
  `reqwest::Client` skene has no public non-streaming equivalent for
  (`raw_client()` is intentionally `#[cfg(test)]`-only, per #4925).
- `api/sse.rs` keeps its own reconnect loop for Dioxus-coroutine-specific
  debounced loss reporting skene's own `SseConnection` doesn't do.
- `api/system_status.rs` wraps `GET /api/v1/system/status`, a route skene
  has never modeled (it only wraps the flat `/api/v1/system/health`).
- `services/connection.rs`'s liveness probe needs the parsed body/status
  `ApiClient::health()` collapses away by returning only a bare `bool`.

```
skene  (shared: domain types, event parsing, typed ApiClient — the boundary
        for every view-layer call)
    ^
    |
proskenion  (Dioxus desktop app; 4 sub-view-layer files still build
             requests locally — see #7251)
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full crate dependency graph.

## Platform limitations

### Wayland: no remote launch over SSH

The desktop app cannot be launched over SSH on Wayland compositors. WebKitWebProcess spawns as a subprocess that cannot inherit the Wayland display socket from an SSH session - the compositor only allows processes from the local session to connect.

Symptom:
```
(WebKitWebProcess:95849): Gtk-WARNING: cannot open display:
** (aletheia-desktop:95722): ERROR: readPIDFromPeer: Unexpected short read from PID socket.
```

Workarounds:
1. **Run locally, point at remote server.** Launch the desktop app on the machine with the display, configure it to connect to the remote Aletheia instance via the server URL.
2. **Use X11 forwarding.** `ssh -X` works with X11/Xwayland, though performance is limited.
3. **Use the TUI.** The terminal interface works over any SSH session.

This is a WebKit/GTK limitation, not an Aletheia issue. Proskenion currently
uses in-window shortcuts only; it does not register native global hotkeys.
