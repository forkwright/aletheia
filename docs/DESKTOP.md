# Desktop application

`proskenion` is a Dioxus desktop UI for Aletheia providing chat, planning, memory browsing, metrics, and ops views.

## System dependencies

The desktop crate uses Dioxus with a WebView backend, which requires GTK3 and webkit2gtk system libraries.

**Debian/Ubuntu:**

```bash
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev
```

**Fedora:**

```bash
sudo dnf install gtk3-devel webkit2gtk4.1-devel libxdo-devel
```

**macOS:** No additional dependencies. WebKit is bundled with the OS.

## Build

The desktop crate is excluded from the root workspace because its GTK/webkit2gtk dependency set needs dedicated system packages and carries desktop-only cargo-deny advisory noise. The path-filtered desktop CI job installs those packages and compiles, lints, and tests `proskenion` through its standalone manifest.

For the standard local install flow, run:

```bash
scripts/install-proskenion.sh
```

The installer verifies Linux GTK/webkit2gtk system libraries, builds the release binary through the standalone manifest, and installs `proskenion` to `~/.cargo/bin/`.

Build it standalone using the manifest path:

```bash
cargo build -p proskenion --manifest-path crates/theatron/proskenion/Cargo.toml
cargo build -p proskenion --manifest-path crates/theatron/proskenion/Cargo.toml --release
```

For a Nix development environment, allow direnv or enter the shell directly:

```bash
direnv allow
nix develop .#proskenion
```

The flake package and shell both target the standalone `proskenion` manifest.

## Contract and smoke checks

The desktop crate is outside the main workspace, so acceptance uses two focused checks instead of a full GUI driver.

> Maintainer/CI variant. Prerequisites: install the pinned toolchain (`rustup toolchain install 1.94`) and build the desktop binary first with `scripts/install-proskenion.sh`, which places `proskenion` on `~/.cargo/bin`. The contract test compiles from a fresh checkout; the smoke invocation below needs that installed binary.

```bash
cargo +1.94 test -p integration-tests --features test-core proskenion_contract -- --nocapture

bash -n scripts/smoke-proskenion.sh
scripts/smoke-proskenion.sh --proskenion-binary ~/.cargo/bin/proskenion
```

The `proskenion_contract` integration test exercises the protocol surface the app consumes: agent list/status/tool envelopes, knowledge browse endpoints, metrics/cost/token envelopes, session create/list/history, and `POST /api/v1/sessions/stream` SSE event names, terminal events, and JSON field shape. If it fails, file the failure as a server/client runtime-contract mismatch and include the assertion text, full response body printed by the test, endpoint, and expected proskenion field or event name.

The smoke script starts a local server when no `--server-url` is supplied, or connects to the supplied URL. Use the default gateway port when targeting an already running local server:

```bash
scripts/smoke-proskenion.sh --server-url http://127.0.0.1:18789 --proskenion-binary ~/.cargo/bin/proskenion
```

The script writes a temporary desktop config, uses `xvfb-run` when no `DISPLAY` is available, enforces a bounded runtime, captures logs, and fails on known display/startup/connectivity patterns. Missing Xvfb or a missing `proskenion` binary exits with a clear skip status instead of silently passing.

## Pin discipline

`proskenion` has its own standalone workspace, so its theatron git dependencies cannot inherit from the root `[workspace.dependencies]` block. Keep the mirrored pins in `crates/theatron/proskenion/Cargo.toml` aligned with the root manifest.

Run the pin check before changing the desktop pins:

```bash
scripts/check-proskenion-pins.py
```

The standard installer runs this check before building, and the release workflow runs it before the release test suite.

## Why excluded from workspace

1. **Root workspace compatibility.** GTK3 and webkit2gtk are installed only by the dedicated desktop CI job. Keeping `proskenion` outside the root workspace avoids forcing every workspace gate to install desktop system packages, while the desktop job still runs compile, clippy, and tests through the standalone manifest.
2. **Dependency advisories.** GTK bindings pull in crates with known advisories that are acceptable for a desktop app but would block cargo-deny checks for the rest of the workspace.
3. **Independent versioning.** The desktop crate tracks its own version instead of inheriting from `[workspace.package]`, since it ships on a separate release cadence.

## Architecture

The desktop crate depends on `skene` for the shared API client, domain types, and SSE infrastructure. It connects to a running Aletheia server over HTTP, the same as the TUI.

```
skene  (shared: API client, types, SSE)
    ^
    |
proskenion  (Dioxus desktop app)
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
