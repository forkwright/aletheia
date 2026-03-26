# Desktop application

`theatron-desktop` is a Dioxus desktop UI for Aletheia providing chat, planning, memory browsing, metrics, and ops views.

## System dependencies

The desktop crate uses Dioxus with a WebView backend, which requires GTK3 and webkit2gtk system libraries.

**Debian/Ubuntu:**

```bash
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev
```

**Fedora:**

```bash
sudo dnf install gtk3-devel webkit2gtk4.1-devel
```

**macOS:** No additional dependencies. WebKit is bundled with the OS.

## Build

The desktop crate is excluded from the workspace because its GTK/webkit2gtk dependencies are not available in CI and would cause build failures and cargo-deny advisory noise.

Build it standalone using the manifest path:

```bash
cargo build -p theatron-desktop --manifest-path crates/theatron/desktop/Cargo.toml
cargo build -p theatron-desktop --manifest-path crates/theatron/desktop/Cargo.toml --release
```

## Why excluded from workspace

1. **CI compatibility.** GTK3 and webkit2gtk are not installed in the CI environment. Including the crate in the workspace would fail `cargo build --workspace` and `cargo clippy --workspace`.
2. **Dependency advisories.** GTK bindings pull in crates with known advisories that are acceptable for a desktop app but would block cargo-deny checks for the rest of the workspace.
3. **Independent versioning.** The desktop crate tracks its own version rather than inheriting from `[workspace.package]`, since it ships on a separate release cadence.

## Architecture

The desktop crate depends on `theatron-core` for the shared API client, domain types, and SSE infrastructure. It connects to a running Aletheia server over HTTP, the same as the TUI.

```
theatron-core  (shared: API client, types, SSE)
    ^
    |
theatron-desktop  (Dioxus desktop app)
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full crate dependency graph.
