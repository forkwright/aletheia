//! Chat view placeholder.

use dioxus::prelude::*;

#[component]
pub(crate) fn Chat() -> Element {
    rsx! {
        div {
            h1 { style: "font-size: 24px; margin-bottom: 16px;", "Chat" }
            p { style: "color: #888;", "Chat interface" }
        }
    }
}
