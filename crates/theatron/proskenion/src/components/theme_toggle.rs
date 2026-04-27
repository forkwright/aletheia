//! Theme toggle wrapper — wires proskenion's settings persistence into
//! the canonical [`theatron_core::ThemeToggle`] component.
//!
//! The visual + interaction logic lives in theatron-core (W2 extraction).
//! This wrapper exists only to inject `on_change` with proskenion's
//! save-state plumbing so the user's theme preference survives restart.

use dioxus::prelude::*;
use theatron_core::{ThemeMode, ThemeToggle as CoreThemeToggle};

use crate::state::settings::{AppearanceSettings, KeybindingStore, ServerConfigStore};

/// Persistence-aware theme toggle for proskenion.
#[component]
pub(crate) fn ThemeToggle() -> Element {
    let mut appearance = use_context::<Signal<AppearanceSettings>>();
    let server_store = use_context::<Signal<ServerConfigStore>>();
    let keybindings = use_context::<Signal<KeybindingStore>>();

    rsx! {
        CoreThemeToggle {
            on_change: move |next: ThemeMode| {
                appearance.write().theme = next.label().to_lowercase();
                crate::services::settings_config::save_state(
                    &server_store.read(),
                    &appearance.read(),
                    &keybindings.read(),
                );
            },
        }
    }
}
