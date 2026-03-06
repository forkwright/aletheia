use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::Overlay;
use crate::state::settings::{EditState, FieldType, SaveStatus, SettingsOverlay};

pub async fn handle_open(app: &mut App) {
    match app.client.config().await {
        Ok(config) => {
            app.overlay = Some(Overlay::Settings(SettingsOverlay::from_config(&config)));
        }
        Err(e) => {
            app.error_toast = Some(ErrorToast::new(format!("Failed to load config: {e}")));
        }
    }
}

pub fn handle_loaded(app: &mut App, config: serde_json::Value) {
    app.overlay = Some(Overlay::Settings(SettingsOverlay::from_config(&config)));
}

pub fn handle_up(app: &mut App) {
    if let Some(Overlay::Settings(s)) = &mut app.overlay
        && s.editing.is_none()
        && s.cursor > 0
    {
        s.cursor -= 1;
    }
}

pub fn handle_down(app: &mut App) {
    if let Some(Overlay::Settings(s)) = &mut app.overlay
        && s.editing.is_none()
        && s.cursor + 1 < s.total_fields()
    {
        s.cursor += 1;
    }
}

pub fn handle_enter(app: &mut App) {
    if let Some(Overlay::Settings(ref mut s)) = app.overlay {
        if s.editing.is_some() {
            confirm_edit(s);
            return;
        }

        let field = match s.current_field() {
            Some(f) => f.clone(),
            None => return,
        };

        if !field.editable {
            return;
        }

        match field.field_type {
            FieldType::Bool => {
                if let Some(f) = s.current_field_mut() {
                    let new_val = !f.value.as_bool().unwrap_or(false);
                    f.value = serde_json::Value::Bool(new_val);
                }
            }
            FieldType::Integer | FieldType::Text => {
                let buf = match &field.value {
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::String(sv) => sv.clone(),
                    _ => String::new(),
                };
                let cursor = buf.len();
                s.editing = Some(EditState {
                    buffer: buf,
                    cursor,
                });
            }
            FieldType::ReadOnly => {}
        }
    }
}

fn confirm_edit(s: &mut SettingsOverlay) {
    if let Some(edit) = s.editing.take()
        && let Some(field) = s.current_field_mut()
    {
        match field.field_type {
            FieldType::Integer => {
                if let Ok(n) = edit.buffer.parse::<u64>() {
                    field.value = serde_json::Value::Number(n.into());
                }
            }
            FieldType::Text => {
                field.value = serde_json::Value::String(edit.buffer);
            }
            _ => {}
        }
    }
}

pub fn handle_edit_char(app: &mut App, c: char) {
    if let Some(Overlay::Settings(s)) = &mut app.overlay
        && let Some(edit) = &mut s.editing
    {
        edit.buffer.insert(edit.cursor, c);
        edit.cursor += 1;
    }
}

pub fn handle_edit_backspace(app: &mut App) {
    if let Some(Overlay::Settings(s)) = &mut app.overlay
        && let Some(edit) = &mut s.editing
        && edit.cursor > 0
    {
        edit.cursor -= 1;
        edit.buffer.remove(edit.cursor);
    }
}

pub fn handle_edit_escape(app: &mut App) {
    if let Some(Overlay::Settings(ref mut s)) = app.overlay {
        s.editing = None;
    }
}

pub async fn handle_save(app: &mut App) {
    let changed = {
        let settings = match &mut app.overlay {
            Some(Overlay::Settings(s)) => s,
            _ => return,
        };

        if !settings.has_changes() {
            app.overlay = None;
            return;
        }

        settings.save_status = SaveStatus::Saving;
        settings.changed_sections()
    };

    let mut errors = Vec::new();
    for (section, data) in &changed {
        if let Err(e) = app.client.update_config_section(section, data).await {
            errors.push(format!("{section}: {e}"));
        }
    }

    if errors.is_empty() {
        app.error_toast = Some(ErrorToast::new("Config saved and reloaded".to_owned()));
        app.overlay = None;
    } else if let Some(Overlay::Settings(s)) = &mut app.overlay {
        s.save_status = SaveStatus::Error(errors.join("; "));
    }
}

pub fn handle_saved(app: &mut App) {
    app.error_toast = Some(ErrorToast::new("Config saved and reloaded".to_owned()));
    app.overlay = None;
}

pub fn handle_save_error(app: &mut App, msg: String) {
    if let Some(Overlay::Settings(s)) = &mut app.overlay {
        s.save_status = SaveStatus::Error(msg);
    }
}

pub fn handle_reset(app: &mut App) {
    if let Some(Overlay::Settings(s)) = &mut app.overlay {
        s.reset();
    }
}

pub fn is_editing(app: &App) -> bool {
    matches!(
        &app.overlay,
        Some(Overlay::Settings(s)) if s.editing.is_some()
    )
}
