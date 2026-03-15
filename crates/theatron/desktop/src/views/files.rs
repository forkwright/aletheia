//! Files view placeholder.

use dioxus::prelude::*;

#[component]
pub(crate) fn Files() -> Element {
    rsx! {
        div {
            h1 { style: "font-size: 24px; margin-bottom: 16px;", "Files" }
            p { style: "color: #888;", "File browser" }
        }
    }
}
