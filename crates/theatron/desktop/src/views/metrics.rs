//! Metrics view placeholder.

use dioxus::prelude::*;

#[component]
pub(crate) fn Metrics() -> Element {
    rsx! {
        div {
            h1 { style: "font-size: 24px; margin-bottom: 16px;", "Metrics" }
            p { style: "color: #888;", "Metrics dashboard" }
        }
    }
}
