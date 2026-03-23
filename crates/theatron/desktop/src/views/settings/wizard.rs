//! First-run setup wizard.
//!
//! Five-step flow: Server → Auth → Discovery → Appearance → Ready.
//! Writes settings to disk and dismisses itself on completion.

use dioxus::prelude::*;

use crate::services::settings_config;
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

    let mut step = use_signal(WizardStep::default);
    let wizard_data = use_signal(WizardData::default);

    let step_count = WizardStep::total();
    let current_idx = step().index();

    rsx! {
        div {
            style: "min-height: 100vh; display: flex; align-items: center; justify-content: center; \
                    background: #0a0a14; padding: 24px;",

            div {
                style: "background: #111122; border: 1px solid #333; border-radius: 12px; \
                        width: 100%; max-width: 520px; padding: 32px;",

                // Title
                div {
                    style: "margin-bottom: 24px;",
                    h1 { style: "font-size: 22px; margin: 0 0 6px; color: #e0e0e0;", "Welcome to Aletheia" }
                    p { style: "font-size: 14px; color: #666; margin: 0;", "Let's get you set up in a few steps." }
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
                                on_next: move |_| { step.set(WizardStep::Auth); }
                            }
                        },
                        WizardStep::Auth => rsx! {
                            StepAuth {
                                wizard_data,
                                on_back: move |_| { step.set(WizardStep::Server); },
                                on_next: move |_| { step.set(WizardStep::Discovery); }
                            }
                        },
                        WizardStep::Discovery => rsx! {
                            StepDiscovery {
                                wizard_data,
                                on_back: move |_| { step.set(WizardStep::Auth); },
                                on_next: move |_| { step.set(WizardStep::Appearance); }
                            }
                        },
                        WizardStep::Appearance => rsx! {
                            StepAppearance {
                                wizard_data,
                                on_back: move |_| { step.set(WizardStep::Discovery); },
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
                                            data.server_name.clone(),
                                            data.server_url.clone(),
                                            token.clone(),
                                        );
                                        store.set_active(&id);
                                        id
                                    };
                                    drop(server_id);

                                    let selected_theme = data.selected_theme.clone();
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
                                    }

                                    // Persist.
                                    {
                                        let store = server_store.read();
                                        let app = appearance.read();
                                        let keys = keybindings.read();
                                        settings_config::save_state(&store, &app, &keys);
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
                    let bg = if filled { "#5b6af0" } else { "#2a2a3a" };
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
    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",
            h2 { style: "font-size: 16px; color: #e0e0e0; margin: 0;", "Server Connection" }
            p { style: "font-size: 13px; color: #888; margin: 0;",
                "Enter the URL of your Aletheia server instance."
            }

            div {
                style: "display: flex; flex-direction: column; gap: 4px;",
                label {
                    style: "font-size: 11px; color: #666; text-transform: uppercase; letter-spacing: 0.5px;",
                    "Server Name"
                }
                input {
                    style: "background: #0d0d1a; border: 1px solid #444; border-radius: 6px; \
                            padding: 8px 12px; color: #e0e0e0; font-size: 13px; width: 100%; box-sizing: border-box;",
                    placeholder: "My Aletheia",
                    value: "{wizard_data.read().server_name}",
                    oninput: move |e| { wizard_data.write().server_name = e.value(); },
                }
            }

            div {
                style: "display: flex; flex-direction: column; gap: 4px;",
                label {
                    style: "font-size: 11px; color: #666; text-transform: uppercase; letter-spacing: 0.5px;",
                    "Server URL"
                }
                input {
                    style: "background: #0d0d1a; border: 1px solid #444; border-radius: 6px; \
                            padding: 8px 12px; color: #e0e0e0; font-size: 13px; width: 100%; box-sizing: border-box;",
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
                    let name = wizard_data.read().server_name.clone();
                    if !url.is_empty() {
                        if name.is_empty() {
                            wizard_data.write().server_name = "My Aletheia".to_string();
                        }
                        on_next.call(());
                    }
                },
                next_label: "Next",
            }
        }
    }
}

// --- Step: Auth ---

#[component]
fn StepAuth(
    wizard_data: Signal<WizardData>,
    on_back: EventHandler<()>,
    on_next: EventHandler<()>,
) -> Element {
    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",
            h2 { style: "font-size: 16px; color: #e0e0e0; margin: 0;", "Authentication" }
            p { style: "font-size: 13px; color: #888; margin: 0;",
                "If your server requires an API token, enter it here."
            }

            div {
                style: "display: flex; flex-direction: column; gap: 4px;",
                label {
                    style: "font-size: 11px; color: #666; text-transform: uppercase; letter-spacing: 0.5px;",
                    "Auth Token (optional)"
                }
                input {
                    r#type: "password",
                    style: "background: #0d0d1a; border: 1px solid #444; border-radius: 6px; \
                            padding: 8px 12px; color: #e0e0e0; font-size: 13px; width: 100%; box-sizing: border-box;",
                    placeholder: "Leave blank if not required",
                    value: "{wizard_data.read().auth_token}",
                    oninput: move |e| { wizard_data.write().auth_token = e.value(); },
                }
            }

            div {
                style: "display: flex; align-items: center; gap: 8px; padding: 10px; \
                        background: #161626; border-radius: 6px; border: 1px solid #2a2a3a;",
                input {
                    r#type: "checkbox",
                    id: "skip_auth",
                    checked: wizard_data.read().skip_auth,
                    onchange: move |e| { wizard_data.write().skip_auth = e.checked(); },
                }
                label {
                    r#for: "skip_auth",
                    style: "font-size: 13px; color: #aaa; cursor: pointer;",
                    "My server does not require authentication"
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

// --- Step: Discovery ---

#[component]
fn StepDiscovery(
    wizard_data: Signal<WizardData>,
    on_back: EventHandler<()>,
    on_next: EventHandler<()>,
) -> Element {
    let mut discovering = use_signal(|| false);
    let mut discover_error: Signal<Option<String>> = use_signal(|| None);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 16px;",
            h2 { style: "font-size: 16px; color: #e0e0e0; margin: 0;", "Agent Discovery" }
            p { style: "font-size: 13px; color: #888; margin: 0;",
                "Connect to your server to discover available agents."
            }

            // Discovered agents list
            if !wizard_data.read().discovered_agents.is_empty() {
                div {
                    style: "background: #0d1a0d; border: 1px solid #1a3a1a; border-radius: 6px; padding: 12px;",
                    div {
                        style: "font-size: 11px; color: #4ade80; margin-bottom: 8px;",
                        "{wizard_data.read().discovered_agents.len()} agent(s) found"
                    }
                    for agent in wizard_data.read().discovered_agents.iter() {
                        div {
                            style: "font-size: 13px; color: #a0e0a0; padding: 2px 0;",
                            "• {agent}"
                        }
                    }
                }
            }

            if let Some(ref err) = discover_error.read().clone() {
                div {
                    style: "background: #1a0d0d; border: 1px solid #4a1a1a; border-radius: 6px; \
                            padding: 10px 12px; font-size: 12px; color: #f87171;",
                    "{err}"
                }
            }

            div {
                style: "display: flex; gap: 8px;",
                button {
                    style: "padding: 7px 16px; background: #1a2a3a; border: 1px solid #5b6af0; \
                            border-radius: 6px; color: #8899ff; font-size: 13px; cursor: pointer;",
                    disabled: discovering(),
                    onclick: move |_| {
                        let url = wizard_data.read().server_url.clone();
                        let token = wizard_data.read().auth_token.clone();
                        discovering.set(true);
                        discover_error.set(None);
                        spawn(async move {
                            match fetch_agents(&url, &token).await {
                                Ok(agents) => {
                                    wizard_data.write().discovered_agents = agents;
                                }
                                Err(e) => {
                                    discover_error.set(Some(e));
                                }
                            }
                            discovering.set(false);
                        });
                    },
                    if discovering() { "Connecting…" } else { "Discover agents" }
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
            h2 { style: "font-size: 16px; color: #e0e0e0; margin: 0;", "Appearance" }
            p { style: "font-size: 13px; color: #888; margin: 0;",
                "Choose your preferred theme and layout density."
            }

            // Theme
            div {
                style: "display: flex; flex-direction: column; gap: 8px;",
                div {
                    style: "font-size: 11px; color: #666; text-transform: uppercase; letter-spacing: 0.5px;",
                    "Theme"
                }
                div {
                    style: "display: flex; gap: 8px;",
                    for (mode, slug) in [("Dark", "dark"), ("Light", "light"), ("System", "system")] {
                        {
                            let is_active = wizard_data.read().selected_theme == slug;
                            let bg = if is_active { "#5b6af0" } else { "#2a2a4a" };
                            let border = if is_active { "1px solid #5b6af0" } else { "1px solid #444" };
                            let color = if is_active { "#fff" } else { "#aaa" };
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
                    style: "font-size: 11px; color: #666; text-transform: uppercase; letter-spacing: 0.5px;",
                    "Density"
                }
                div {
                    style: "display: flex; gap: 8px;",
                    for density in [UiDensity::Compact, UiDensity::Comfortable, UiDensity::Spacious] {
                        {
                            let is_active = wizard_data.read().selected_density == density;
                            let bg = if is_active { "#5b6af0" } else { "#2a2a4a" };
                            let border = if is_active { "1px solid #5b6af0" } else { "1px solid #444" };
                            let color = if is_active { "#fff" } else { "#aaa" };
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
                    style: "font-size: 11px; color: #666; text-transform: uppercase; letter-spacing: 0.5px;",
                    "Accent Color (preview)"
                }
                div {
                    style: "display: flex; gap: 10px; flex-wrap: wrap;",
                    for (label, hex) in ACCENT_PRESETS.iter() {
                        {
                            let hex_owned = hex.to_string();
                            let style = format!(
                                "width: 28px; height: 28px; border-radius: 50%; background: {hex_owned}; \
                                 border: 2px solid #333; cursor: pointer; outline: none;"
                            );
                            rsx! {
                                button {
                                    key: "{label}",
                                    title: "{label}",
                                    style: "{style}",
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
                "✓"
            }
            h2 { style: "font-size: 18px; color: #e0e0e0; margin: 0;", "You're all set!" }
            p { style: "font-size: 13px; color: #888; margin: 0;",
                "Aletheia will connect to {data.server_url}"
            }

            div {
                style: "background: #161626; border: 1px solid #333; border-radius: 8px; \
                        padding: 14px 20px; width: 100%; text-align: left;",
                SummaryRow { label: "Server", value: data.server_name.clone() }
                SummaryRow { label: "Theme", value: data.selected_theme.clone() }
                SummaryRow { label: "Density", value: data.selected_density.label().to_string() }
                SummaryRow {
                    label: "Auth",
                    value: if data.auth_token.is_empty() { "None".to_string() } else { "Configured".to_string() }
                }
                if !data.discovered_agents.is_empty() {
                    SummaryRow {
                        label: "Agents",
                        value: format!("{} discovered", data.discovered_agents.len()),
                    }
                }
            }

            button {
                style: "padding: 10px 32px; background: #5b6af0; border: none; border-radius: 8px; \
                        color: #fff; font-size: 15px; cursor: pointer; width: 100%;",
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
                    padding-top: 16px; border-top: 1px solid #222; margin-top: 8px;",
            if can_back {
                button {
                    style: "padding: 7px 18px; background: none; border: 1px solid #444; \
                            border-radius: 6px; color: #888; font-size: 13px; cursor: pointer;",
                    onclick: move |_| on_back.call(()),
                    "← Back"
                }
            } else {
                div {}
            }
            button {
                style: "padding: 7px 22px; background: #5b6af0; border: none; \
                        border-radius: 6px; color: #fff; font-size: 13px; cursor: pointer;",
                onclick: move |_| on_next.call(()),
                "{next_label} →"
            }
        }
    }
}

#[component]
fn SummaryRow(label: &'static str, value: String) -> Element {
    rsx! {
        div {
            style: "display: flex; justify-content: space-between; padding: 5px 0; \
                    border-bottom: 1px solid #1a1a2e; font-size: 13px;",
            span { style: "color: #666;", "{label}" }
            span { style: "color: #e0e0e0;", "{value}" }
        }
    }
}

// --- Async discovery ---

async fn fetch_agents(base_url: &str, token: &str) -> Result<Vec<String>, String> {
    use crate::services::connection::PylonClient;
    use crate::state::connection::ConnectionConfig;

    let config = ConnectionConfig {
        server_url: base_url.to_string(),
        auth_token: if token.is_empty() { None } else { Some(token.to_string()) },
        auto_reconnect: false,
    };
    let client = PylonClient::new(&config).map_err(|e| e.to_string())?;
    let url = format!("{}/api/v1/nous", client.base_url());

    let resp = client
        .raw_client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("server returned {}", resp.status()));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse response: {e}"))?;

    let agents: Vec<String> = body
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();

    Ok(agents)
}
