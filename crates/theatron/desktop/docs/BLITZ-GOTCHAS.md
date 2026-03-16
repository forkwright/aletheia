# Blitz gotchas and limitations

Known issues, workarounds, and limitations of the Dioxus 0.7 Blitz WGPU renderer. Current as of March 2026.

---

## Critical (blocks adoption)

### No scrolling

Blitz does not implement scrollable containers or render scroll bars (Blitz #339, #353). Any content exceeding the viewport is clipped and inaccessible. There is no programmatic `scrollTo` either (Dioxus #4479).

**Impact:** Conversation history, long documents, and any vertically-scrolling UI is unusable.

**Workaround:** None within Blitz. Use the webview renderer.

### NixOS blank window

Apps render a blank white window on NixOS (Dioxus #5133). Root cause is in wgpu/winit surface initialization.

**Workaround:** Use the webview renderer on NixOS.

### Linux segfault on window close

Closing a Blitz window on Linux triggers a segfault (Blitz #355, Dioxus #5128).

**Workaround:** None. The process crashes on exit.

### Windows instant crash

Minimal Blitz apps crash immediately on Windows (Dioxus #4901).

**Workaround:** Use the webview renderer on Windows.

---

## High (significant UX degradation)

### No system tray or global hotkey

The `tray-icon` and `global-hotkey` crates are only wired into `dioxus-desktop` (webview), not `dioxus-native` (Blitz). Tracked in Dioxus #4479.

**Workaround:** Feature-gate platform APIs behind `#[cfg(feature = "webview")]`.

### No clipboard events

Copy, cut, and paste events are not implemented in Blitz.

**Workaround:** None within Blitz.

### Tailwind hover utilities silently fail

Tailwind wraps `hover:*` classes in `@media(hover: hover)` which Blitz does not support (Blitz #252). Hover styles are silently ignored.

**Workaround:** Use inline `:hover` CSS or signal-driven hover state with `onmouseenter`/`onmouseleave`.

### No CSS animations or transitions

`transition-*`, `animate-*`, `@keyframes` — none are implemented.

**Workaround:** Use Dioxus signals with `use_future` and manual style interpolation for animations.

---

## Medium (functionality gaps)

### Missing DOM APIs

No `scrollTo`, `getElementById`, `querySelector`, or `eval` (Dioxus #4479). JavaScript evaluation is not applicable since there is no JS engine.

### Incomplete form controls

Missing: `<select>`, `<input type=password>`, `<input type=email>`, `<input type=url>`, `<input type=color>`, `<meter>`, `<progress>`.

**Workaround:** Build custom components for dropdowns and specialized inputs.

### Checkbox signal bug

Checkbox `checked` property does not update via signals (Dioxus #5282).

**Workaround:** Track state externally and use conditional styling.

### Partial box shadows

Box shadows render but are blocked on Vello drop shadow support for full fidelity.

### No drag-and-drop

Not implemented in the Blitz event system.

---

## Low (cosmetic or edge cases)

### No `vertical-align`

Affects subscript/superscript and baseline alignment in mixed-size text.

### No `text-shadow`

Text shadow CSS property is not implemented.

### No filters or blurs

`backdrop-blur`, `blur()`, `brightness()`, `contrast()` — not implemented.

### No `<video>` or `<audio>`

Media elements are not supported. Use platform-native media playback if needed.

### Emoji rendering inconsistencies

Color emoji rendering varies across platforms (Blitz #308).

### Windows DPI scaling issues

High-DPI displays on Windows may render at incorrect scale (Blitz #307).

---

## Compilation gotchas

### Incompatible with sea-orm

Blitz's dependency tree conflicts with `sea-orm` (Dioxus #4866). If the desktop crate needs database access, use a separate process or IPC.

### Heavy compile times

Stylo (CSS engine from Firefox) alone pulls ~200 crate dependencies. First build of a Blitz app takes significantly longer than webview.

### Git dependencies

Blitz uses git dependencies for Parley (Linebender). This can cause resolution issues with `cargo publish` and reproducibility concerns.

---

## When to re-evaluate

Track these milestones for Blitz adoption readiness:

1. **Scrolling implemented** — Blitz #339, #353
2. **NixOS blank window fixed** — Dioxus #5133
3. **Linux close segfault fixed** — Blitz #355
4. **`@media(hover: hover)` supported** — Blitz #252
5. **Beta release** with stability commitment from maintainers
6. **Form controls complete** — `<select>`, password inputs
7. **Clipboard events** — copy/cut/paste
