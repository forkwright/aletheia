// kanon:ignore STORAGE/no-migration-checksum -- theme toggle UI contains no storage migrations (#3988)
//! Theme toggle wrapper — wires proskenion's settings persistence into
//! the canonical [`themelion::ThemeToggle`] component.
//!
//! The visual + interaction logic lives in themelion (W2 extraction).
//! This wrapper exists only to inject `on_change` with proskenion's
//! save-state plumbing so the user's theme preference survives restart.

use dioxus::prelude::*;
use themelion::{ThemeMode, ThemeToggle as CoreThemeToggle};

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
                // WHY: slug() is the storage wire format ("dark"/"light"/
                // "system"); label() is display-only.
                appearance.write().theme = next.slug().to_string();
                crate::services::settings_config::save_state(
                    &server_store.read(),
                    &appearance.read(),
                    &keybindings.read(),
                );
            },
        }
    }
}
