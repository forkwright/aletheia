# dx CLI workflow

Dioxus 0.7 CLI (`dx`) workflow for desktop development.

---

## Installation

```bash
cargo install dioxus-cli
# or
cargo binstall dioxus-cli
```

Verify: `dx --version` (requires 0.7+).

---

## Project creation

```bash
dx new aletheia-desktop
# Prompts: platform → Desktop, tailwind → yes/no, template → default
```

Or manually: add `dioxus = { version = "0.7", features = ["desktop"] }` to `Cargo.toml`.

---

## Dev server

### Webview renderer (production path)

```bash
dx serve --platform desktop
# or
dx serve --renderer webview
```

### Blitz native renderer (research)

```bash
dx serve --renderer native
```

### Hot-patching (subsecond)

Dioxus 0.7's headline feature. Uses a custom incremental linker to hot-patch Rust code changes in sub-second time without losing app state.

- RSX markup changes: instant hot-reload (no recompile)
- Rust logic changes: sub-second hot-patch via incremental linking
- Press `d` during `dx serve` to launch VSCode debugger

**Known issues with hot-patching (as of March 2026):**
- Hot-patching breaks hot-reloading (Dioxus #5317)
- Workspace crate monitoring incomplete (#5314)
- Stack overflow in `dioxus_devtools::serve_subsecond` (#5311)
- Windows 10 build panics after hot reload (#5279)
- v0 mangling breaks hot-patching (#5005)

---

## Release build

```bash
dx bundle --platform desktop
```

Output formats by platform:
- **macOS:** `.app`, `.dmg`
- **Linux:** `.appimage`, `.rpm`, `.deb`
- **Windows:** `.msi`, `.exe`

No cross-compilation. Must build on target platform.

---

## Other useful commands

| Command | Purpose |
|---------|---------|
| `dx fmt` | Auto-format RSX markup |
| `dx check` | Validate project configuration |
| `dx doctor` | Diagnose missing toolchains/dependencies |
| `dx self-update` | Update the CLI to latest |

---

## Integration with Aletheia build system

The `theatron-desktop` crate is standalone (not in workspace). Build directly:

```bash
# Blitz native (research)
cargo build --manifest-path crates/theatron/desktop/Cargo.toml

# Webview (production)
cargo build --manifest-path crates/theatron/desktop/Cargo.toml \
    --features webview --no-default-features

# Release bundle
cd crates/theatron/desktop && dx bundle --platform desktop
```

When the desktop crate is promoted to workspace membership, standard `cargo build -p theatron-desktop` will work.
