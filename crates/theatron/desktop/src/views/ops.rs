//! Ops view placeholder.

use dioxus::prelude::*;

#[component]
pub(crate) fn Ops() -> Element {
    rsx! {
        div {
            h1 { style: "font-size: 24px; margin-bottom: 16px;", "Ops" }
            p { style: "color: #888;", "Operations center" }
        }
    }
}
