# Tailwind integration validation

Assessment of Tailwind CSS with Dioxus 0.7 across both renderers.

---

## Setup

Dioxus 0.7 has zero-setup Tailwind support. The `dx` CLI auto-starts a TailwindCSS watcher when it detects a `tailwind.css` file.

### Steps

1. Create `input.css`:
   ```css
   @import "tailwindcss";
   @source "./src/**/*.{rs,html,css}";
   ```

2. The CLI generates compiled CSS in `assets/tailwind.css`.

3. Reference in components:
   ```rust
   use dioxus::prelude::*;

   fn app() -> Element {
       rsx! {
           document::Stylesheet { href: asset!("/assets/tailwind.css") }
           div { class: "max-w-3xl mx-auto p-6",
               h1 { class: "text-2xl font-bold text-gray-900", "Aletheia" }
           }
       }
   }
   ```

---

## Webview renderer: fully supported

Tailwind works without restrictions in the wry webview renderer. All utility classes, responsive breakpoints, hover states, animations, and dark mode function as expected.

**Verdict:** production-ready.

---

## Blitz native renderer: partial support with blockers

### What works
- Layout utilities: `flex`, `grid`, `p-*`, `m-*`, `w-*`, `h-*`, `max-w-*`
- Typography: `text-*`, `font-*`, `leading-*`
- Colors: `bg-*`, `text-*`, `border-*`
- Borders: `border`, `rounded-*`
- Spacing: `gap-*`, `space-*`
- Basic `:hover` pseudo-class styling

### Blocker: `@media(hover: hover)`

Tailwind wraps hover utilities like `hover:bg-amber-500` in `@media (hover: hover)` queries. Blitz does not support this media query (Blitz issue #252).

**Impact:** Hover utilities silently fail. The base styles apply but hover transitions do not activate.

**Workaround:** Write hover styles as raw CSS with plain `:hover` selectors instead of Tailwind hover utilities:

```rust
div {
    class: "bg-blue-500",
    // Tailwind hover: won't work in Blitz
    // Use inline style instead:
    style: "transition: background-color 0.2s;",
    onmouseenter: move |_| { /* set hover state */ },
    onmouseleave: move |_| { /* clear hover state */ },
}
```

### Missing features affecting Tailwind classes

| Tailwind utility | Blitz support | Notes |
|------------------|---------------|-------|
| `hover:*` | Broken | `@media(hover: hover)` not supported |
| `overflow-y-auto` | No effect | No scroll implementation |
| `backdrop-blur-*` | No effect | Filters not implemented |
| `shadow-*` | Partial | Box shadows partially implemented |
| `animate-*` | No effect | CSS animations not implemented |
| `transition-*` | No effect | CSS transitions not implemented |
| `dark:*` | Untested | Depends on `@media(prefers-color-scheme)` support |

---

## Recommendation

Use Tailwind with the **webview renderer** for production. If targeting Blitz, limit to layout and typography utilities and avoid hover, animation, and scroll-dependent classes.
