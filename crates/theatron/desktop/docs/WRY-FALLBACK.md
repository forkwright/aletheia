# Fallback path to wry webview

Concrete steps to switch from Blitz native to the wry webview renderer.

---

## Why fallback

Blitz is pre-alpha with hard blockers for Aletheia (no scrolling, NixOS blank window, Linux segfault on close). The wry webview renderer is production-ready and supports all platform APIs (system tray, global hotkey, file dialogs).

---

## Step 1: Change feature flags

In `crates/theatron/desktop/Cargo.toml`:

```toml
[features]
default = ["webview"]          # was: ["native"]
native = ["dioxus/native"]
webview = ["dioxus/desktop"]
```

No other Cargo.toml changes needed. The `dioxus` crate re-exports the correct renderer based on the feature.

---

## Step 2: No code changes required for basic UI

All RSX, components, signals, hooks, and context work identically across renderers. The hello-world and any component-based UI code is shared.

```rust
// This works with both renderers
fn app() -> Element {
    rsx! {
        div { class: "container",
            h1 { "Aletheia Desktop" }
        }
    }
}
```

---

## Step 3: Add platform APIs (webview-only)

System tray and global hotkey are only available with the `desktop` (webview) feature.

### System tray

```rust
use dioxus::desktop::tray::*;
use dioxus::desktop::muda;

fn app() -> Element {
    // Initialize tray icon
    init_tray_icon(|| {
        let menu = muda::Menu::new();
        let show = muda::MenuItem::new("Show", true, None);
        let quit = muda::MenuItem::new("Quit", true, None);
        menu.append(&show).ok();
        menu.append(&quit).ok();
        TrayIconBuilder::new()
            .with_tooltip("Aletheia")
            .with_menu(Box::new(menu))
            .build()
    });

    // Handle tray events
    use_tray_icon_event_handler(|event| {
        // Handle click, double-click, menu item activation
    });

    rsx! { /* ... */ }
}
```

**Linux dependency:** glib >= 2.70 required (Dioxus #4477).

### Global hotkey

```rust
use dioxus::desktop::use_global_shortcut;

fn app() -> Element {
    use_global_shortcut("CommandOrCtrl+Shift+Space", move || {
        // Toggle window visibility, focus, etc.
    });

    rsx! { /* ... */ }
}
```

**Constraints:**
- Only one `GlobalHotkeyManager` at a time (creating another crashes)
- Must run on main thread
- Linux: X11 only (no Wayland global hotkey support)

---

## Step 4: Conditional compilation (optional)

To maintain both renderers in one codebase:

```rust
#[cfg(feature = "webview")]
mod platform {
    use dioxus::desktop::tray::*;
    use dioxus::desktop::use_global_shortcut;

    pub fn setup_tray() { /* tray setup */ }
    pub fn setup_hotkey() { /* hotkey setup */ }
}

#[cfg(feature = "native")]
mod platform {
    pub fn setup_tray() { /* no-op: Blitz lacks tray support */ }
    pub fn setup_hotkey() { /* no-op: Blitz lacks hotkey support */ }
}
```

---

## Step 5: Build and bundle

```bash
# Dev
dx serve --platform desktop
# or
cargo run --manifest-path crates/theatron/desktop/Cargo.toml \
    --features webview --no-default-features

# Release bundle
cd crates/theatron/desktop && dx bundle --platform desktop
```

---

## Migration effort

| Aspect | Effort | Notes |
|--------|--------|-------|
| Feature flag switch | Minutes | One-line change in Cargo.toml |
| UI code migration | Zero | RSX is renderer-agnostic |
| Platform API addition | Hours | System tray + global hotkey are new code |
| Tailwind | Zero | Full support in webview |
| NixOS | Working | WebKitGTK deps, no blank-window bug |

**Total: a few hours of additive work, zero UI rewrite.**
