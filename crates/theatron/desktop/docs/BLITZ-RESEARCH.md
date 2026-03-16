---
created: 2026-03-14
updated: 2026-03-14
tags:
  - research
  - theatron
  - desktop
  - dioxus
  - blitz
---

# Dioxus 0.7 + Blitz WGPU renderer research

> Research spike for using Dioxus 0.7 with the Blitz native WGPU renderer as the Aletheia desktop shell.

---

## Table of contents

1. [Maturity assessment](#1-maturity-assessment)
2. [Hello-world validation](#2-hello-world-validation)
3. [dx CLI workflow](#3-dx-cli-workflow)
4. [Tailwind integration](#4-tailwind-integration)
5. [NixOS build compatibility](#5-nixos-build-compatibility)
6. [Fallback path: wry webview](#6-fallback-path-wry-webview)
7. [Gotchas and limitations](#7-gotchas-and-limitations)
8. [Recommendation](#8-recommendation)

---

## 1. Maturity assessment

### Status: alpha, not production-ready

Blitz is in **alpha** as of March 2026. The team targets a broadly usable beta by mid-2026 and production readiness later in 2026. Dioxus 0.7.3 ships `dioxus-native` 0.7.3 backed by Blitz.

### Architecture

Blitz combines components from several ecosystems:

| Component | Origin | Role |
|-----------|--------|------|
| Stylo | Firefox/Servo | CSS selector matching and cascade resolution |
| Taffy | Rust UI ecosystem | Flexbox and CSS Grid layout |
| Parley | Linebender | Text shaping and paragraph layout |
| Vello | Linebender | GPU-accelerated 2D rendering |
| WGPU | gfx-rs | Cross-platform GPU abstraction |
| Winit | Rust windowing | Window creation, input events |
| AccessKit | Rust a11y | Accessibility tree |

### Text-heavy layouts

Blitz supports the core text properties needed for a chat/conversation UI:

| Property | Status |
|----------|--------|
| `color` | Supported |
| `font-size`, `font-family`, `font-weight`, `font-style` | Supported |
| `line-height`, `letter-spacing`, `word-spacing` | Supported |
| `text-align` | Supported |
| `text-decoration` | Supported |
| `text-indent` | Supported |
| `word-break`, `overflow-wrap` | Supported |
| `white-space-collapse` | Partial |
| `text-transform` | **Not supported** |
| `text-overflow` (ellipsis) | **Not supported** |
| `-webkit-line-clamp` | **Not supported** |

**Assessment for Aletheia:** The supported set covers conversation messages, markdown-rendered content, and code blocks. Missing `text-overflow` means truncated message previews need a workaround (character-level truncation in Rust). Missing `text-transform` means uppercase labels must be uppercased in code.

### Streaming content

Blitz renders Dioxus virtual DOM diffs the same way the webview renderer does. Signal-driven reactivity (Dioxus 0.7 signals) works identically. Token-by-token streaming from an LLM updates the DOM through signal mutations, and Blitz re-renders the affected subtree.

**Risk:** No benchmarks exist for Blitz rendering throughput with rapid, small DOM updates (the streaming token case). The webview path has years of browser optimization for this pattern. Blitz may stutter on fast token streams until the rendering pipeline matures.

### Complex nested components

Dioxus components compose identically regardless of renderer. The virtual DOM is renderer-agnostic. The `text_panel` component in the hello-world crate validates nested component rendering.

**Risk:** Blitz's DOM implementation (`blitz-dom`) is younger than browser DOMs. Edge cases in event bubbling, focus management, or attribute inheritance may surface with deeply nested component trees.

### Layout support

| Feature | Status | Notes |
|---------|--------|-------|
| Flexbox | Supported | Core layout model, fully functional |
| CSS Grid | Supported | Including `fr`, `min-content`, `max-content`, named lines, `grid-area` |
| Absolute positioning | Partial | Works but relative to immediate parent only, no static position support |
| Fixed positioning | **Not supported** | Cannot pin headers/footers to viewport |
| Sticky positioning | **Not supported** | Cannot create sticky headers |
| Overflow scroll | Supported | `overflow: scroll` works |
| Overflow auto | **Not supported** | Must use explicit `overflow: scroll` |
| CSS transitions | Supported | |
| CSS animations | Supported | |
| 2D transforms | Partial | Visual only, hit-testing incomplete |
| 3D transforms | **Not supported** | |
| CSS variables | Not documented | |
| Subgrid | **Not supported** | |

---

## 2. Hello-world validation

### Crate location

`crates/theatron/desktop/` contains a standalone binary crate that demonstrates:

- Text-heavy panel layout with multiple paragraphs
- Reusable nested components (`text_panel`)
- Interactive button with signal-driven state
- Inline CSS styling

### Dependencies

```toml
[dependencies]
dioxus = "0.7"
dioxus-native = "0.7"
```

The crate is intentionally excluded from the workspace `members` list. Blitz pulls in WGPU, Vello, Winit, and other GPU dependencies that the rest of the workspace does not need. Build it explicitly:

```bash
cargo build -p theatron-desktop
cargo run -p theatron-desktop
```

### System tray and global hotkey: blocker

**System tray and global hotkey are `dioxus-desktop` (webview/wry) features, not `dioxus-native` (Blitz) features.**

- `dioxus-desktop` re-exports `tray-icon` (0.21) and `global-hotkey` (0.7) crates
- `dioxus-desktop` provides `use_tray_icon_event_handler`, `use_tray_menu_event_handler`, and `use_global_shortcut` hooks
- `dioxus-native` uses `blitz-shell` (built on Winit), which does not expose system tray or global hotkey APIs

**Workarounds:**

1. **Use the standalone crates directly.** `tray-icon` and `global-hotkey` are independent crates that work with any Winit event loop. Wire them manually into the Blitz shell's Winit event loop. This requires reaching into `blitz-shell` internals, which is not a stable API.

2. **Run the Blitz app alongside a separate tray process.** Spawn a lightweight process for the system tray that communicates with the main Blitz window via IPC (Unix socket or named pipe).

3. **Fall back to `dioxus-desktop` (webview).** Use the wry webview renderer, which has built-in tray and hotkey support.

**Recommendation:** If system tray and global hotkey are hard requirements for the initial desktop release, start with `dioxus-desktop` (webview) and plan to migrate to Blitz when it gains feature parity.

---

## 3. dx CLI workflow

### Install

```bash
cargo install dioxus-cli
# or on NixOS:
nix profile install nixpkgs#dioxus-cli  # version 0.7.3 in nixpkgs
```

### Development server

```bash
# Webview renderer (default for desktop)
dx serve --platform desktop

# Native renderer (Blitz)
dx serve --platform desktop --renderer native

# With hot-patching (subsecond Rust code patching)
dx serve --platform desktop --hot-patch

# Press 'd' during dx serve to attach LLDB debugger
```

Key `dx serve` flags:

| Flag | Purpose |
|------|---------|
| `--platform desktop` | Target desktop platform |
| `--renderer native` | Use Blitz instead of webview |
| `--hot-patch` | Enable subsecond Rust hot-patching |
| `--open false` | Do not auto-open window |
| `--always-on-top true` | Keep window above other windows |
| `--release` | Optimized build |

### Hot-patching (subsecond)

Dioxus 0.7 ships "Subsecond," a hot-patching system that patches Rust code at runtime without full recompilation. It works across WASM, macOS, Linux, Windows, iOS, and Android.

Usage: wrap patchable call sites with `subsecond::call()`. During `dx serve`, all `subsecond::call` sites are hot-patched when source changes are detected. Hooks can be broken during development without crashing the app.

### Release build

```bash
# Optimized build
dx build --release --platform desktop

# Bundle for distribution
dx bundle --release --platform desktop

# Bundle formats:
#   Linux:   --package-types deb,rpm,appimage
#   macOS:   --package-types dmg,pkg
#   Windows: --package-types msi,nsis
```

`dx bundle` produces self-contained distributables. Web builds get `.avif` generation, `.wasm` compression, and minification. Desktop/mobile apps are typically under 5 MB (webview) or approximately 12 MB (Blitz, due to WGPU).

### Project configuration

`dx new` generates a project with a `Dioxus.toml` config. For existing projects, key `Dioxus.toml` settings:

```toml
[application]
name = "theatron-desktop"
default-platform = "desktop"

[application.desktop]
renderer = "native"  # or "webview"
```

---

## 4. Tailwind integration

### Auto-detection

The `dx` CLI auto-detects Tailwind if a `tailwind.css` (or `input.css`) file exists at the project root. It starts a TailwindCSS watcher automatically during `dx serve`. Both Tailwind v3 and v4 are supported.

### Setup steps

1. Install Tailwind CLI: `npm install -D @tailwindcss/cli` (or use the standalone binary)
2. Create `input.css` at project root:
   ```css
   @import "tailwindcss";
   @source "./src/**/*.{rs,html,css}";
   ```
3. Add the compiled stylesheet to your component:
   ```rust
   use dioxus::prelude::*;
   rsx! {
       document::Stylesheet { href: asset!("./assets/tailwind.css") }
   }
   ```
4. Run `dx serve` and Tailwind classes compile automatically.

### Blitz compatibility: partial

Tailwind generates standard CSS utility classes. Whether those classes work depends on Blitz's CSS property support. Most layout and typography classes work. Key gaps:

| Tailwind utility | CSS property | Blitz support |
|-----------------|-------------|---------------|
| `flex`, `grid`, `block` | `display` | Supported |
| `p-*`, `m-*` | `padding`, `margin` | Supported |
| `text-*` (size) | `font-size` | Supported |
| `font-*` | `font-weight`, `font-family` | Supported |
| `bg-*` | `background-color` | Supported |
| `border-*` | `border` | Supported |
| `rounded-*` | `border-radius` | Supported |
| `shadow-*` | `box-shadow` | Supported |
| `uppercase` | `text-transform` | **Not supported** |
| `truncate` | `text-overflow: ellipsis` | **Not supported** |
| `line-clamp-*` | `-webkit-line-clamp` | **Not supported** |
| `sticky` | `position: sticky` | **Not supported** |
| `fixed` | `position: fixed` | **Not supported** |
| `overflow-auto` | `overflow: auto` | **Not supported** |
| `scroll-*` | `scroll-snap-*` | **Not supported** |
| `transform` (3D) | `transform: rotate3d()` | **Not supported** |

**Assessment:** Approximately 80% of common Tailwind utilities work with Blitz. The missing 20% affects common UI patterns (sticky headers, text truncation, auto-scroll containers). For Aletheia's conversation UI, `overflow-auto` and `sticky` are the most impactful gaps.

**Workaround:** Use `overflow: scroll` instead of `overflow: auto`. Avoid sticky positioning; use flexbox column layouts with fixed-height containers instead. Truncate text in Rust code before rendering.

---

## 5. NixOS build compatibility

### System dependencies

Blitz (via WGPU/Vello) requires GPU drivers and windowing libraries. On NixOS, these must be available in the nix store.

Required packages for the dev shell:

```nix
buildInputs = with pkgs; [
  # WGPU / Vulkan
  vulkan-loader
  vulkan-headers
  vulkan-tools

  # Windowing (Winit)
  wayland
  wayland-protocols
  libxkbcommon
  xorg.libX11
  xorg.libXcursor
  xorg.libXrandr
  xorg.libXi

  # System fonts (Blitz system-fonts feature)
  fontconfig
  freetype

  # TLS (for network features)
  openssl
  pkg-config
];

# WGPU needs to find the Vulkan ICD at runtime
LD_LIBRARY_PATH = lib.makeLibraryPath [
  pkgs.vulkan-loader
  pkgs.wayland
  pkgs.libxkbcommon
];
```

### Crane build

Extend the existing Crane build with Blitz-specific native dependencies:

```nix
let
  blitzDeps = with pkgs; [
    vulkan-loader wayland wayland-protocols
    libxkbcommon fontconfig freetype
    xorg.libX11 xorg.libXcursor xorg.libXrandr xorg.libXi
  ];
in craneLib.buildPackage (commonArgs // {
  inherit cargoArtifacts;
  buildInputs = commonArgs.buildInputs ++ blitzDeps;
  # Runtime library path for WGPU Vulkan loader
  postFixup = ''
    patchelf --set-rpath "${pkgs.lib.makeLibraryPath blitzDeps}" $out/bin/theatron-desktop
  '';
});
```

### GPU driver requirement

WGPU needs a working Vulkan, Metal, or DX12 driver. On NixOS:

- **AMD:** `hardware.graphics.enable = true;` (uses Mesa RADV)
- **NVIDIA:** `hardware.nvidia.open = true;` and `hardware.graphics.enable = true;`
- **Intel:** `hardware.graphics.enable = true;` (uses Mesa ANV)
- **Headless/CI:** WGPU can fall back to `llvmpipe` (software Vulkan), but rendering is slow. Set `WGPU_BACKEND=gl` for OpenGL fallback.

### Dioxus CLI on NixOS

`dioxus-cli` 0.7.3 is packaged in nixpkgs. The Dioxus repository also ships a `flake.nix` for development. For Aletheia, add `dioxus-cli` to the dev shell:

```nix
devShells.default = pkgs.mkShell {
  packages = [ pkgs.dioxus-cli ];
};
```

### Status

- `dioxus-cli` 0.7.3 builds and runs on NixOS (available in nixpkgs)
- WGPU renders on AMD, Intel, and NVIDIA with `hardware.graphics.enable = true`
- Crane can build `dioxus-native` with the dependency set above
- `LD_LIBRARY_PATH` must include `vulkan-loader` and `wayland` for runtime linking

---

## 6. Fallback path: wry webview

If Blitz limitations block the desktop release, fall back to `dioxus-desktop` (wry webview renderer). This is the mature, production-tested path.

### Step 1: swap the dependency

```toml
# Before (Blitz native)
[dependencies]
dioxus = "0.7"
dioxus-native = "0.7"

# After (wry webview)
[dependencies]
dioxus = { version = "0.7", features = ["desktop"] }
```

### Step 2: update launch code

```rust
// No code change needed for basic apps. dioxus::launch(app) works with both
// renderers. For desktop-specific features (tray, hotkey), import from
// dioxus::desktop:

use dioxus::prelude::*;
use dioxus::desktop::{Config, WindowBuilder};

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_window(WindowBuilder::new().with_title("Aletheia"))
        )
        .launch(app);
}
```

### Step 3: add system tray (webview only)

```rust
use dioxus::prelude::*;

fn app() -> Element {
    // System tray is only available with dioxus-desktop
    use_tray_menu_event_handler(move |event| {
        // Handle tray menu clicks
    });

    rsx! {
        // Tray icon defined in Dioxus.toml or via Config
    }
}
```

### Step 4: add global hotkey (webview only)

```rust
use dioxus::prelude::*;
use dioxus::desktop::use_global_shortcut;

fn app() -> Element {
    use_global_shortcut("CommandOrControl+Shift+Space", move || {
        // Toggle window visibility
    });

    rsx! { /* ... */ }
}
```

### Step 5: update dx commands

```bash
# Webview is the default for desktop, no --renderer flag needed
dx serve --platform desktop
dx bundle --release --platform desktop
```

### Step 6: NixOS dependencies change

Replace WGPU/Vulkan deps with WebKitGTK:

```nix
buildInputs = with pkgs; [
  webkitgtk_4_1  # wry's rendering backend on Linux
  gtk3
  glib
  cairo
  pango
  gdk-pixbuf
  libsoup_3
  openssl
  pkg-config
];
```

### Trade-offs

| Aspect | Blitz (native) | Wry (webview) |
|--------|---------------|---------------|
| Binary size | ~12 MB | ~5 MB |
| Startup time | Fast (no browser engine) | Fast (system webview) |
| CSS support | ~80% of properties | 100% (browser engine) |
| System tray | Not available | Built-in hooks |
| Global hotkey | Not available | Built-in hooks |
| Accessibility | AccessKit (early) | Browser a11y (mature) |
| System dependency | Vulkan/WGPU drivers | WebKitGTK (Linux) |
| Hot-patching | Supported | Supported |
| Maturity | Alpha | Production-ready |

---

## 7. Gotchas and limitations

### Critical: system tray and global hotkey not in Blitz

System tray (`tray-icon`) and global hotkey (`global-hotkey`) hooks exist only in `dioxus-desktop` (webview). `dioxus-native` (Blitz) has no equivalent. This is the single largest blocker for using Blitz as the Aletheia desktop shell, because the system tray icon and a global activation hotkey are core UX requirements.

### No `position: fixed` or `position: sticky`

Blitz does not support `fixed` or `sticky` positioning. This affects:

- Sticky conversation headers
- Fixed input bars at the bottom of the chat window
- Floating action buttons

**Workaround:** Use flexbox column layout with explicit height allocation. The input bar lives in a flex-none container at the bottom; the message list lives in a flex-grow container with `overflow: scroll`.

### No `overflow: auto`

Only `overflow: scroll` is supported. Scrollbars always render even when content fits the container.

**Workaround:** Use `overflow: scroll` and accept always-visible scrollbars. Alternatively, set `overflow: hidden` and implement scroll behavior in Rust via Dioxus event handlers.

### No `text-overflow: ellipsis`

Long text cannot be truncated with CSS ellipsis.

**Workaround:** Truncate strings in Rust before rendering. Calculate approximate character limits based on container width.

### No `text-transform`

`uppercase`, `lowercase`, and `capitalize` do not work.

**Workaround:** Transform text in Rust: `text.to_uppercase()`, `text.to_lowercase()`.

### Hit-testing incomplete for 2D transforms

Transformed elements render correctly but may not respond to clicks in the expected position.

**Workaround:** Avoid transforms on interactive elements. Use layout-based positioning instead.

### Absolute positioning is parent-relative only

`position: absolute` positions relative to the immediate parent, not the nearest positioned ancestor. The `position: static` value is not supported.

**Workaround:** Ensure the immediate parent is the intended positioning context.

### No subgrid

CSS subgrid is not supported. Grid children cannot inherit the parent grid's track sizing.

**Workaround:** Use explicit grid definitions or switch to flexbox for the inner layout.

### Binary size increase

Blitz adds WGPU, Vello, Stylo, and Winit. Expect approximately 12 MB binaries versus approximately 5 MB for the webview path.

### GPU driver requirement

WGPU requires a working Vulkan, Metal, or DX12 driver. Headless environments (CI, containers) need software rendering (`llvmpipe` or `WGPU_BACKEND=gl`).

### Rapid development pace

Blitz and dioxus-native are under active development. APIs change between minor versions. Pin exact versions in `Cargo.toml` and test upgrades deliberately.

### Font rendering differences

Blitz uses Parley (Linebender) for text shaping instead of a browser's text stack. Font metrics, line breaking, and glyph selection may differ from browser rendering. Test with the actual fonts the app will use.

### No JavaScript interop

Blitz deliberately excludes JavaScript execution. Any web-based third-party widgets, charting libraries, or markdown renderers that depend on JS do not work. Use Rust-native alternatives.

---

## 8. Recommendation

**Start with `dioxus-desktop` (webview) for the initial Aletheia desktop release. Plan a migration to Blitz when it reaches beta and gains system tray support.**

Rationale:

1. **System tray and global hotkey are hard requirements.** Blitz does not support them. The webview path has built-in hooks.
2. **CSS coverage gaps affect core UI patterns.** Missing `overflow: auto`, `position: sticky`, and `text-overflow` require workarounds that add complexity and reduce visual fidelity.
3. **Blitz is alpha software.** Production stability is not guaranteed. The Aletheia desktop shell needs to be reliable for daily use.
4. **Migration cost is low.** Dioxus components are renderer-agnostic. Switching from webview to Blitz requires changing the dependency and launch configuration, not rewriting the UI. The component tree, signals, and event handlers stay the same.
5. **The Blitz team targets production-readiness in 2026.** Revisit after Blitz reaches beta and the `dioxus-native` crate exposes tray/hotkey APIs.

### Migration trigger criteria

Move to Blitz when all of these are true:

- [ ] `dioxus-native` exposes system tray and global hotkey hooks (or `blitz-shell` supports them directly)
- [ ] `position: fixed` and `position: sticky` are supported
- [ ] `overflow: auto` is supported
- [ ] `text-overflow: ellipsis` is supported
- [ ] Blitz reaches beta status
- [ ] Aletheia's conversation UI renders correctly in Blitz without layout workarounds
