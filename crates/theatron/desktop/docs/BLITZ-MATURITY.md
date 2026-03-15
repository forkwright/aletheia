# Blitz maturity assessment

Assessment of the Dioxus 0.7 Blitz WGPU renderer for Aletheia's desktop surface. Evaluated March 2026.

---

## Overall verdict: not production-ready

Blitz is **pre-alpha** (v0.2.0). The maintainers explicitly recommend against production use. The renderer has a capable core but critical gaps block Aletheia's use case.

---

## Text-heavy layouts

Blitz uses **Parley** (Linebender) for text shaping/line-breaking and **Vello** for GPU rendering. Basic text rendering is described as "often indistinguishable from Chrome and Safari" for simple layouts.

| Feature | Status | Notes |
|---------|--------|-------|
| Dense paragraphs | Works | Parley handles line-breaking correctly |
| Inline styles | Works | `font-weight`, `font-size`, `color`, `line-height` |
| Font fallback | Works | Fontique handles system font discovery |
| `vertical-align` | Missing | Breaks sub/superscript, baseline alignment |
| Scrollable text | Missing | Blitz issue #339 — no scroll implementation |
| Scroll bars | Missing | Blitz issue #353 — not rendered at all |

**Risk for Aletheia:** Streaming LLM responses produce long, scrollable text. Without scrolling, the desktop app cannot display conversation history. This is a **hard blocker** until resolved.

---

## Streaming content

Dioxus signals and `use_coroutine` drive reactive updates identically across renderers — the reactive layer is renderer-agnostic. Streaming itself is not a Blitz concern.

However, streaming content typically requires:
- Scrollable containers (missing in Blitz)
- Auto-scroll to bottom (no `scrollTo` API — tracked in Dioxus issue #4479)
- Clipboard copy of streamed text (no clipboard events in Blitz)

---

## Complex nested components

Dioxus virtual DOM composition works identically across renderers. Component nesting, props, signals, hooks, and context all function correctly.

| Feature | Status | Notes |
|---------|--------|-------|
| Component nesting | Works | Virtual DOM is renderer-agnostic |
| Signal reactivity | Works | Same reactive primitives |
| Conditional rendering | Works | `if`/`match` in RSX |
| List rendering | Works | `for` loops in RSX |
| DOM manipulation APIs | Missing | No `scrollTo`, `getElementById`, `querySelector` |
| Checkbox `checked` | Broken | Dioxus issue #5282 — signal subscribers not updating |

---

## CSS feature support

### Supported
- Flexbox (Taffy)
- CSS Grid (Taffy, partial)
- Block layout
- Table layout (`table-layout: fixed`, `border-collapse`)
- Absolute/relative positioning (partial)
- CSS variables
- `calc()` values
- `z-index`
- Box shadows (partial — blocked on Vello upstream)
- `:hover` pseudo-class

### Missing
- `position: fixed` and `position: static` (incomplete)
- CSS Subgrid
- Intrinsic sizing (`min-content`, `max-content`, `fit-content()`)
- `writing-mode` and direction
- `clip-path`, `mask`
- `text-shadow`
- Filters and blurs
- `@media(hover: hover)` — breaks Tailwind hover utilities
- Backgrounds on inline elements
- `<video>`, `<audio>`, `<picture>` elements

### Form controls
- **Working:** `<button>`, `<input type=text>`, `<textarea>`, `<input type=checkbox>`, `<input type=radio>`, `<input type=file>`, `<input type=submit>`
- **Missing:** `<select>`, `<input type=password>`, `<input type=email>`, `<input type=url>`, `<input type=color>`, `<meter>`, `<progress>`

---

## Platform stability

| Platform | Status | Issues |
|----------|--------|--------|
| Linux (X11/Wayland) | Segfault on close | Blitz #355, Dioxus #5128 |
| NixOS | Blank window | Dioxus #5133 — hard blocker |
| Windows | Instant crash | Dioxus #4901 — minimal apps crash |
| macOS | Best supported | Fewest reported issues |

---

## Dependency weight

Blitz pulls in a significant dependency tree:

- **wgpu 27** — GPU abstraction (Vulkan/Metal/DX12)
- **Vello 0.6** — GPU 2D renderer
- **Stylo** (from Servo/Firefox) — CSS engine
- **Taffy** (DioxusLabs fork) — layout engine
- **Parley** (Linebender, git dep) — text layout
- **winit** — cross-platform windowing
- **AccessKit** — accessibility

Compile time impact is substantial (Stylo alone is ~200 crates).

---

## Recommendation

**Do not adopt Blitz for Aletheia desktop today.** Use the wry webview renderer (`--features webview`) for the initial desktop release. Re-evaluate Blitz when:

1. Scrolling is implemented (Blitz #339, #353)
2. NixOS blank-window bug is fixed (Dioxus #5133)
3. Linux segfault-on-close is fixed (Blitz #355)
4. `@media(hover: hover)` is supported (Blitz #252) for Tailwind
5. The project reaches beta status with a stability commitment
