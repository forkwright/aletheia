//! First-run setup wizard.
//!
//! Three-step flow: Server -> Appearance -> Ready.
//! Writes settings to disk and dismisses itself on completion.

use dioxus::prelude::*;

use crate::services::{config, settings_config};
use crate::state::connection::ConnectionConfig;
use crate::state::settings::{
    ACCENT_PRESETS, AppearanceSettings, ServerConfigStore, UiDensity, WizardData, WizardStep,
};
use crate::theme::ThemeMode;

/// Setup wizard root.
#[component]
pub(crate) fn SetupWizard() -> Element {
    let mut is_first_run: Signal<bool> = use_context();
    let mut server_store: Signal<ServerConfigStore> = use_context();
    let mut appearance: Signal<AppearanceSettings> = use_context();
    let keybindings = use_context::<Signal<crate::state::settings::KeybindingStore>>();
    let mut theme_mode: Signal<ThemeMode> = use_context();
    let mut connection_config: Signal<ConnectionConfig> = use_context();

    let mut step = use_signal(WizardStep::default);
    let wizard_data = use_signal(WizardData::default);

    let step_count = WizardStep::total();
    let current_idx = step().index();

    rsx! {
        div {
            style: "min-height: 100vh; display: flex; align-items: center; justify-content: center; \
                    background: var(--bg-surface-dim); padding: 24px;",

            div {
                style: "background: var(--bg-surface); border: 1px solid var(--border); border-radius: 12px; \
                        width: 100%; max-width: 520px; padding: 32px;",

                // Title
                div {
                    style: "margin-bottom: 24px;",
                    h1 { style: "font-size: 22px; margin: 0 0 6px; color: var(--text-primary);", "Welcome to Aletheia" }
                    p { style: "font-size: 14px; color: var(--text-muted); margin: 0;", "Let's get you set up in a few steps." }
                }

                // Progress bar
                WizardProgress { current: current_idx, total: step_count }

                // Step content
                div {
                    style: "margin-top: 28px;",
                    { match step() {
                        WizardStep::Server => rsx! {
                            StepServer {
                                wizard_data,
                                on_next: move |_| { step.set(WizardStep::Appearance); }
                            }
                        },
                        WizardStep::Appearance => rsx! {
                            StepAppearance {
                                wizard_data,
                                on_back: move |_| { step.set(WizardStep::Server); },
                                on_next: move |_| { step.set(WizardStep::Ready); }
                            }
                        },
                        WizardStep::Ready => rsx! {
                            StepReady {
                                wizard_data,
                                on_finish: move |_| {
                                    // Apply wizard data to context signals.
                                    let data = wizard_data.read();
                                    let token = if data.auth_token.is_empty() { None } else { Some(data.auth_token.clone()) };

                                    let server_id = {
                                        let mut store = server_store.write();
                                        let id = store.add(
                                            "My Aletheia".to_string(),
                                            data.server_url.clone(),
                                            token.clone(),
                                        );
                                        store.set_active(&id);
                                        id
                                    };
                                    drop(server_id);

                                    let selected_theme = data.selected_theme.clone();
                                    let selected_accent = data.selected_accent.clone();
                                    let density = data.selected_density;
                                    drop(data);

                                    let new_mode = match selected_theme.as_str() {
                                        "dark" => ThemeMode::Dark,
                                        "light" => ThemeMode::Light,
                                        _ => ThemeMode::System,
                                    };
                                    theme_mode.set(new_mode);
                                    {
                                        let mut app = appearance.write();
                                        app.theme = selected_theme;
                                        app.density = density;
                                        if !selected_accent.is_empty() {
                                            app.accent_color = selected_accent;
                                        }
                                    }

                                    // Persist settings (appearance, keybindings, server list).
                                    {
                                        let store = server_store.read();
                                        let app = appearance.read();
                                        let keys = keybindings.read();
                                        settings_config::save_state(&store, &app, &keys);
                                    }

                                    // WHY: Also persist the URL to the connection config
                                    // (desktop.toml) so ConnectView picks it up instead
                                    // of reverting to the compiled default.
                                    {
                                        let data = wizard_data.read();
                                        let token = if data.auth_token.is_empty() {
                                            None
                                        } else {
                                            Some(data.auth_token.clone())
                                        };
                                        let new_config = ConnectionConfig {
                                            server_url: data.server_url.clone(),
                                            auth_token: token,
                                            auto_reconnect: true,
                                        };
                                        connection_config.set(new_config.clone());
                                        if let Err(e) = config::save(&new_config) {
                                            tracing::warn!("failed to save connection config from wizard: {e}");
                                        }
                                    }

                                    is_first_run.set(false);
                                }
                            }
                        },
                    } }
                }
            }
        }
    }
}

// --- Progress bar ---

#[component]
fn WizardProgress(current: usize, total: usize) -> Element {
    rsx! {
        div {
            style: "display: flex; gap: 6px; align-items: center;",
            for i in 0..total {
                {
                    let filled = i <= current;
                    let bg = if filled { "var(--accent)" } else { "var(--bg-surface-bright)" };
                    let style = format!(
                        "flex: 1; height: 3px; background: {bg}; border-radius: 2px; transition: background 0.2s;"
                    );
                    rsx! {
                        div {
                            key: "{i}",
                            style: "{style}",
                        }
                    }
                }
            }
        }
    }
}

// --- Step: Server ---

#[component]
fn StepServer(wizard_data: Signal<WizardData>, on_next: EventHandler<()>) -> Element {
    // WHY: Run auto-discovery once when the wizard's server step renders.
    // If the URL field is empty (first-run default), fill it with the
    // discovered server URL so the user can just click "Next".
    let _discovery = use_resource(move || {
        let current = wizard_data.read().server_url.clone();
        async move {
            if !current.is_empty() {
                return; // User already typed something
            }
            if let Some(discovered) = skene::discovery::discover_server().await {
                tracing::info!(url = %discovered, "auto-discovered server for wizard");
                wizard_data.write().server_url = discovered;
            }
        }
    });

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",
            h2 { style: "font-size: 16px; color: var(--text-primary); margin: 0;", "Server Connection" }
            p { style: "font-size: 13px; color: var(--text-secondary); margin: 0;",
                "Enter the URL of your Aletheia server instance, or leave blank for auto-discovery."
            }

            div {
                style: "display: flex; flex-direction: column; gap: 4px;",
                label {
                    style: "font-size: 11px; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px;",
                    "Server URL"
                }
                input {
                    style: "background: var(--input-bg); border: 1px solid var(--input-border); border-radius: 6px; \
                            padding: 8px 12px; color: var(--text-primary); font-size: 13px; width: 100%; box-sizing: border-box;",
                    placeholder: "http://localhost:3000",
                    value: "{wizard_data.read().server_url}",
                    oninput: move |e| { wizard_data.write().server_url = e.value(); },
                }
            }

            WizardNav {
                can_back: false,
                on_back: move |_| {},
                on_next: move |_| {
                    let url = wizard_data.read().server_url.clone();
                    if !url.is_empty() {
                        on_next.call(());
                    }
                },
                next_label: "Next",
            }
        }
    }
}

// --- Step: Appearance ---

#[component]
fn StepAppearance(
    wizard_data: Signal<WizardData>,
    on_back: EventHandler<()>,
    on_next: EventHandler<()>,
) -> Element {
    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 20px;",
            h2 { style: "font-size: 16px; color: var(--text-primary); margin: 0;", "Appearance" }
            p { style: "font-size: 13px; color: var(--text-secondary); margin: 0;",
                "Choose your preferred theme and layout density."
            }

            // Theme
            div {
                style: "display: flex; flex-direction: column; gap: 8px;",
                div {
                    style: "font-size: 11px; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px;",
                    "Theme"
                }
                div {
                    style: "display: flex; gap: 8px;",
                    for (mode, slug) in [("Dark", "dark"), ("Light", "light"), ("System", "system")] {
                        {
                            let is_active = wizard_data.read().selected_theme == slug;
                            let bg = if is_active { "var(--accent)" } else { "var(--bg-surface-bright)" };
                            let border = if is_active { "1px solid var(--accent)" } else { "1px solid var(--border)" };
                            let color = if is_active { "var(--text-inverse)" } else { "var(--text-secondary)" };
                            let style = format!(
                                "flex: 1; padding: 8px; background: {bg}; border: {border}; \
                                 border-radius: 6px; color: {color}; font-size: 13px; cursor: pointer;"
                            );
                            rsx! {
                                button {
                                    key: "{slug}",
                                    style: "{style}",
                                    onclick: move |_| { wizard_data.write().selected_theme = slug.to_string(); },
                                    "{mode}"
                                }
                            }
                        }
                    }
                }
            }

            // Density
            div {
                style: "display: flex; flex-direction: column; gap: 8px;",
                div {
                    style: "font-size: 11px; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px;",
                    "Density"
                }
                div {
                    style: "display: flex; gap: 8px;",
                    for density in [UiDensity::Compact, UiDensity::Comfortable, UiDensity::Spacious] {
                        {
                            let is_active = wizard_data.read().selected_density == density;
                            let bg = if is_active { "var(--accent)" } else { "var(--bg-surface-bright)" };
                            let border = if is_active { "1px solid var(--accent)" } else { "1px solid var(--border)" };
                            let color = if is_active { "var(--text-inverse)" } else { "var(--text-secondary)" };
                            let style = format!(
                                "flex: 1; padding: 8px; background: {bg}; border: {border}; \
                                 border-radius: 6px; color: {color}; font-size: 13px; cursor: pointer; text-align: center;"
                            );
                            rsx! {
                                button {
                                    key: "{density:?}",
                                    style: "{style}",
                                    onclick: move |_| { wizard_data.write().selected_density = density; },
                                    "{density.label()}"
                                }
                            }
                        }
                    }
                }
            }

            // Accent color
            div {
                style: "display: flex; flex-direction: column; gap: 8px;",
                div {
                    style: "font-size: 11px; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px;",
                    "Accent Color"
                }
                div {
                    style: "display: flex; gap: 10px; flex-wrap: wrap;",
                    for (label, hex) in ACCENT_PRESETS.iter() {
                        {
                            let hex_owned = hex.to_string();
                            let is_active = wizard_data.read().selected_accent == *hex;
                            let border = if is_active { "3px solid #fff" } else { "2px solid var(--border)" };
                            let style = format!(
                                "width: 28px; height: 28px; border-radius: 50%; background: {hex_owned}; \
                                 border: {border}; cursor: pointer; outline: none;"
                            );
                            rsx! {
                                button {
                                    key: "{label}",
                                    title: "{label}",
                                    style: "{style}",
                                    onclick: move |_| {
                                        wizard_data.write().selected_accent = hex_owned.clone();
                                    },
                                }
                            }
                        }
                    }
                }
            }

            WizardNav {
                can_back: true,
                on_back: move |_| on_back.call(()),
                on_next: move |_| on_next.call(()),
                next_label: "Next",
            }
        }
    }
}

// --- Step: Ready ---

#[component]
fn StepReady(wizard_data: Signal<WizardData>, on_finish: EventHandler<()>) -> Element {
    let data = wizard_data.read();
    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 20px; align-items: center; text-align: center;",
            div {
                style: "font-size: 48px;",
                "\u{2713}"
            }
            h2 { style: "font-size: 18px; color: var(--text-primary); margin: 0;", "You're all set!" }
            p { style: "font-size: 13px; color: var(--text-secondary); margin: 0;",
                "Aletheia will connect to {data.server_url}"
            }

            div {
                style: "background: var(--bg-surface-dim); border: 1px solid var(--border); border-radius: 8px; \
                        padding: 14px 20px; width: 100%; text-align: left;",
                SummaryRow { label: "Server", value: data.server_url.clone() }
                SummaryRow { label: "Theme", value: data.selected_theme.clone() }
                SummaryRow { label: "Density", value: data.selected_density.label().to_string() }
            }

            button {
                style: "padding: 10px 32px; background: var(--accent); border: none; border-radius: 8px; \
                        color: var(--text-inverse); font-size: 15px; cursor: pointer; width: 100%;",
                onclick: move |_| on_finish.call(()),
                "Launch Aletheia"
            }
        }
    }
}

// --- Shared wizard components ---

#[component]
fn WizardNav(
    can_back: bool,
    on_back: EventHandler<()>,
    on_next: EventHandler<()>,
    next_label: &'static str,
) -> Element {
    rsx! {
        div {
            style: "display: flex; justify-content: space-between; align-items: center; \
                    padding-top: 16px; border-top: 1px solid var(--border-separator); margin-top: 8px;",
            if can_back {
                button {
                    style: "padding: 7px 18px; background: none; border: 1px solid var(--border); \
                            border-radius: 6px; color: var(--text-secondary); font-size: 13px; cursor: pointer;",
                    onclick: move |_| on_back.call(()),
                    "\u{2190} Back"
                }
            } else {
                div {}
            }
            button {
                style: "padding: 7px 22px; background: var(--accent); border: none; \
                        border-radius: 6px; color: var(--text-inverse); font-size: 13px; cursor: pointer;",
                onclick: move |_| on_next.call(()),
                "{next_label} \u{2192}"
            }
        }
    }
}

#[component]
fn SummaryRow(label: &'static str, value: String) -> Element {
    rsx! {
        div {
            style: "display: flex; justify-content: space-between; padding: 5px 0; \
                    border-bottom: 1px solid var(--border-separator); font-size: 13px;",
            span { style: "color: var(--text-muted);", "{label}" }
            span { style: "color: var(--text-primary);", "{value}" }
        }
    }
}
