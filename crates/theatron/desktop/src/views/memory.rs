//! Memory view placeholder.

use dioxus::prelude::*;

#[component]
pub(crate) fn Memory() -> Element {
    rsx! {
        div {
            h1 { style: "font-size: 24px; margin-bottom: 16px;", "Memory" }
            p { style: "color: #888;", "Memory explorer" }
        }
    }
}
