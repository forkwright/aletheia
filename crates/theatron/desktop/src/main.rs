//! Aletheia desktop shell — Dioxus 0.7 proof-of-concept.
//!
//! Validates two renderer backends:
//!
//! **Blitz native** (default feature `native`):
//! ```bash
//! cargo run --manifest-path crates/theatron/desktop/Cargo.toml
//! ```
//!
//! **Wry webview** (feature `webview`, production fallback):
//! ```bash
//! cargo run --manifest-path crates/theatron/desktop/Cargo.toml \
//!     --features webview --no-default-features
//! ```
//!
//! System tray and global hotkey require the `webview` feature — Blitz does
//! not expose windowing platform APIs (tray-icon, global-hotkey) as of 0.7.

use dioxus::prelude::*;

fn main() {
    dioxus::launch(app);
}

/// Root application component.
///
/// Validates:
/// - Text-heavy layout with dense paragraphs
/// - Nested reusable components
/// - Interactive signals (click counter)
/// - Inline CSS styling (Blitz supports inline styles)
fn app() -> Element {
    let mut click_count = use_signal(|| 0_usize);

    rsx! {
        div {
            style: "font-family: system-ui, sans-serif; max-width: 720px; margin: 0 auto; padding: 24px;",

            // Header
            header {
                style: "border-bottom: 1px solid #e2e8f0; padding-bottom: 16px; margin-bottom: 24px;",
                h1 {
                    style: "font-size: 24px; font-weight: 700; color: #1a202c; margin: 0;",
                    "Aletheia Desktop"
                }
                p {
                    style: "font-size: 14px; color: #718096; margin: 4px 0 0 0;",
                    "Dioxus 0.7 renderer validation"
                }
            }

            // Text-heavy layout: dense paragraph rendering
            text_panel {
                title: "Text rendering",
                body: "This panel validates that the renderer handles dense text correctly. \
                       Long paragraphs, inline emphasis, and nested containers must render \
                       without clipping or overlap. Blitz uses Vello for GPU-accelerated \
                       rendering and Parley for text shaping and line-breaking."
            }

            // Nested component reuse
            text_panel {
                title: "Nested components",
                body: "Dioxus components compose identically across renderers. This panel \
                       is a reusable component rendered twice to validate that nested virtual \
                       DOM nodes map correctly to the renderer's internal DOM representation."
            }

            // Streaming content placeholder
            text_panel {
                title: "Streaming content",
                body: "Aletheia streams LLM responses via SSE. The desktop app will use \
                       Dioxus signals + use_coroutine to drive incremental text updates. \
                       This works identically in both Blitz and webview renderers since \
                       the reactive layer is renderer-agnostic."
            }

            // Interactive element
            div {
                style: "margin-top: 24px; padding: 16px; background: #f7fafc; border-radius: 8px;",
                h2 {
                    style: "font-size: 18px; font-weight: 600; margin: 0 0 12px 0;",
                    "Interaction test"
                }
                button {
                    style: "padding: 8px 16px; background: #4299e1; color: white; \
                            border: none; border-radius: 4px; cursor: pointer; font-size: 14px;",
                    onclick: move |_| click_count += 1,
                    "Click count: {click_count}"
                }
            }

            // Footer
            footer {
                style: "margin-top: 32px; padding-top: 16px; border-top: 1px solid #e2e8f0; \
                        font-size: 12px; color: #a0aec0;",
                "theatron-desktop proof-of-concept · Dioxus 0.7"
            }
        }
    }
}

/// Reusable text panel for validating text-heavy layouts and component nesting.
#[component]
fn text_panel(title: String, body: String) -> Element {
    rsx! {
        div {
            style: "margin-bottom: 16px; padding: 16px; border: 1px solid #e2e8f0; border-radius: 8px;",
            h2 {
                style: "font-size: 18px; font-weight: 600; color: #2d3748; margin: 0 0 8px 0;",
                "{title}"
            }
            p {
                style: "font-size: 14px; line-height: 1.6; color: #4a5568; margin: 0;",
                "{body}"
            }
        }
    }
}
