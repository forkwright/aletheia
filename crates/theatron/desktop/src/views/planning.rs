//! Planning view placeholder.

use dioxus::prelude::*;

#[component]
pub(crate) fn Planning() -> Element {
    rsx! {
        div {
            h1 { style: "font-size: 24px; margin-bottom: 16px;", "Planning" }
            p { style: "color: #888;", "Planning dashboard" }
        }
    }
}
