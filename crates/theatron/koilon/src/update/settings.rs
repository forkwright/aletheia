use crate::app::App;
use crate::msg::ErrorToast;
use crate::sanitize::sanitize_for_display;
use crate::state::Overlay;
use crate::state::settings::{EditState, FieldType, SaveStatus, SettingsOverlay};

// SAFETY: sanitized at ingestion: config from API is sanitized via sanitize_config_json.
pub(crate) async fn handle_open(app: &mut App) {
    match app.client.config().await {
        Ok(config) => {
            let clean_config = sanitize_config_json(config);
            app.layout.overlay = Some(Overlay::Settings(SettingsOverlay::from_config(
                &clean_config,
            )));
        }
        Err(e) => {
            app.viewport.error_toast = Some(ErrorToast::new(format!("Failed to load config: {e}")));
        }
    }
}

// SAFETY: sanitized at ingestion: config values from API are sanitized in SettingsOverlay.
// Config values are mostly numbers/bools; string values are sanitized by sanitize_config_json.
pub(crate) fn handle_loaded(app: &mut App, config: serde_json::Value) {
    let clean_config = sanitize_config_json(config);
    app.layout.overlay = Some(Overlay::Settings(SettingsOverlay::from_config(
        &clean_config,
    )));
}

pub(crate) fn handle_up(app: &mut App) {
    if let Some(Overlay::Settings(s)) = &mut app.layout.overlay
        && s.editing.is_none()
        && s.cursor > 0
    {
        s.cursor -= 1;
    }
}

pub(crate) fn handle_down(app: &mut App) {
    if let Some(Overlay::Settings(s)) = &mut app.layout.overlay
        && s.editing.is_none()
        && s.cursor + 1 < s.total_fields()
    {
        s.cursor += 1;
    }
}

pub(crate) fn handle_enter(app: &mut App) {
    if let Some(Overlay::Settings(ref mut s)) = app.layout.overlay {
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
            // NOTE: read-only fields cannot be edited
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
            _ => {
                // NOTE: ReadOnly and Boolean fields don't need text confirmation
            }
        }
    }
}

pub(crate) fn handle_edit_char(app: &mut App, c: char) {
    if let Some(Overlay::Settings(s)) = &mut app.layout.overlay
        && let Some(edit) = &mut s.editing
    {
        edit.buffer.insert(edit.cursor, c);
        edit.cursor += c.len_utf8();
    }
}

pub(crate) fn handle_edit_backspace(app: &mut App) {
    if let Some(Overlay::Settings(s)) = &mut app.layout.overlay
        && let Some(edit) = &mut s.editing
        && edit.cursor > 0
    {
        let mut prev = edit.cursor - 1;
        while prev > 0 && !edit.buffer.is_char_boundary(prev) {
            prev -= 1;
        }
        edit.buffer.remove(prev);
        edit.cursor = prev;
    }
}

pub(crate) fn handle_edit_escape(app: &mut App) {
    if let Some(Overlay::Settings(ref mut s)) = app.layout.overlay {
        s.editing = None;
    }
}

pub(crate) async fn handle_save(app: &mut App) {
    let changed = {
        let settings = match &mut app.layout.overlay {
            Some(Overlay::Settings(s)) => s,
            _ => return,
        };

        if !settings.has_changes() {
            app.layout.overlay = None;
            return;
        }

        settings.save_status = SaveStatus::Saving;
        settings.changed_sections()
    };

    let mut errors = Vec::new();
    let mut restart_required = Vec::new();
    for (section, data) in &changed {
        match app.client.update_config_section(section, data).await {
            Ok(response) => restart_required.extend(restart_paths(&response)),
            Err(e) => errors.push(format!("{section}: {e}")),
        }
    }

    if errors.is_empty() {
        finish_save(app, &restart_required);
    } else if let Some(Overlay::Settings(s)) = &mut app.layout.overlay {
        s.save_status = SaveStatus::Error(errors.join("; "));
    }
}

/// Extract the field paths a config update deferred to restart.
///
/// WHY the response is read rather than assumed: pylon preserves cold values in
/// the live config and reports them in `restart_required`, so a save can succeed
/// while leaving the running system unchanged.
// SAFETY: sanitized at ingestion: paths come from the API response.
fn restart_paths(response: &serde_json::Value) -> Vec<String> {
    let Some(paths) = response
        .get("restart_required")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    paths
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(|path| sanitize_for_display(path).into_owned())
        .collect()
}

/// Build the operator-facing result of a config save.
///
/// WHY the restart case names the paths: "saved and reloaded" on a cold change
/// tells the operator the running system changed when it did not, and the paths
/// are what they need to decide whether the restart is worth taking now.
fn save_outcome_message(restart_required: &[String]) -> String {
    if restart_required.is_empty() {
        return "Config saved and reloaded".to_owned();
    }
    format!(
        "Config saved — restart required for: {}",
        restart_required.join(", ")
    )
}

/// Complete a successful save: report the outcome and refresh dependent state.
///
/// WHY one function rather than a copy per call site: the toast text and the
/// render invalidation below belong to the same event, and when they were split
/// across two call sites only one of them ran.
fn finish_save(app: &mut App, restart_required: &[String]) {
    app.viewport.error_toast = Some(ErrorToast::new(save_outcome_message(restart_required)));
    app.layout.overlay = None;
    // Config changes can affect display (e.g. syntax highlighting theme). Invalidate
    // the cached markdown lines and rebuild virtual scroll heights so the next render
    // picks up fresh layout.
    app.viewport.render.markdown_cache.clear();
    app.rebuild_virtual_scroll();
}

pub(crate) fn handle_saved(app: &mut App) {
    finish_save(app, &[]);
}

// SAFETY: sanitized at ingestion: error messages may contain external data.
pub(crate) fn handle_save_error(app: &mut App, msg: String) {
    if let Some(Overlay::Settings(s)) = &mut app.layout.overlay {
        s.save_status = SaveStatus::Error(sanitize_for_display(&msg).into_owned());
    }
}

/// Recursively sanitize string values in a JSON config tree.
fn sanitize_config_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            serde_json::Value::String(sanitize_for_display(&s).into_owned())
        }
        serde_json::Value::Object(map) => {
            let cleaned: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .map(|(k, v)| {
                    (
                        sanitize_for_display(&k).into_owned(),
                        sanitize_config_json(v),
                    )
                })
                .collect();
            serde_json::Value::Object(cleaned)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sanitize_config_json).collect())
        }
        other => other,
    }
}

pub(crate) fn handle_reset(app: &mut App) {
    if let Some(Overlay::Settings(s)) = &mut app.layout.overlay {
        s.reset();
    }
}

pub(crate) fn is_editing(app: &App) -> bool {
    matches!(
        &app.layout.overlay,
        Some(Overlay::Settings(s)) if s.editing.is_some()
    )
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;
    use crate::state::settings::FieldType;

    fn config_json() -> serde_json::Value {
        serde_json::json!({
            "agents": {
                "defaults": {
                    "maxToolIterations": 10,
                    "thinkingEnabled": true,
                    "thinkingBudget": 2000,
                    "contextTokens": 8000,
                    "maxOutputTokens": 4000,
                    "timeoutSeconds": 60,
                    "toolTimeouts": {
                        "defaultMs": 5000
                    }
                }
            },
            "gateway": {
                "port": 18789,
                "bind": "0.0.0.0"
            }
        })
    }

    fn app_with_settings() -> App {
        let mut app = test_app();
        let settings = SettingsOverlay::from_config(&config_json());
        app.layout.overlay = Some(Overlay::Settings(settings));
        app
    }

    #[test]
    fn handle_loaded_sets_overlay() {
        let mut app = test_app();
        handle_loaded(&mut app, config_json());
        assert!(matches!(app.layout.overlay, Some(Overlay::Settings(_))));
    }

    #[test]
    fn handle_up_decrements_cursor() {
        let mut app = app_with_settings();
        if let Some(Overlay::Settings(s)) = &mut app.layout.overlay {
            s.cursor = 2;
        }
        handle_up(&mut app);
        if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            assert_eq!(s.cursor, 1);
        }
    }

    #[test]
    fn handle_up_saturates_at_zero() {
        let mut app = app_with_settings();
        if let Some(Overlay::Settings(s)) = &mut app.layout.overlay {
            s.cursor = 0;
        }
        handle_up(&mut app);
        if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            assert_eq!(s.cursor, 0);
        }
    }

    #[test]
    fn handle_down_increments_cursor() {
        let mut app = app_with_settings();
        handle_down(&mut app);
        if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            assert_eq!(s.cursor, 1);
        }
    }

    #[test]
    fn handle_down_clamps_at_max() {
        let mut app = app_with_settings();
        if let Some(Overlay::Settings(s)) = &mut app.layout.overlay {
            s.cursor = s.total_fields().saturating_sub(1);
        }
        let prev = if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            s.cursor
        } else {
            0
        };
        handle_down(&mut app);
        if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            assert_eq!(s.cursor, prev);
        }
    }

    #[test]
    fn handle_enter_toggles_bool() {
        let mut app = app_with_settings();
        // Find a bool field
        if let Some(Overlay::Settings(s)) = &mut app.layout.overlay {
            for (i, section) in s.sections.iter().enumerate() {
                for (j, field) in section.fields.iter().enumerate() {
                    if field.field_type == FieldType::Bool && field.editable {
                        // Calculate flat index
                        let flat: usize = s.sections[..i]
                            .iter()
                            .map(|sec| sec.fields.len())
                            .sum::<usize>()
                            + j;
                        s.cursor = flat;
                        break;
                    }
                }
            }
        }
        let before = if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            s.current_field()
                .and_then(|f| f.value.as_bool())
                .unwrap_or(false)
        } else {
            false
        };
        handle_enter(&mut app);
        let after = if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            s.current_field()
                .and_then(|f| f.value.as_bool())
                .unwrap_or(false)
        } else {
            false
        };
        assert_ne!(before, after);
    }

    #[test]
    fn handle_enter_opens_editor_for_integer() {
        let mut app = app_with_settings();
        // cursor 0 should be maxToolIterations (Integer)
        handle_enter(&mut app);
        assert!(is_editing(&app));
    }

    #[test]
    fn handle_edit_char_appends() {
        let mut app = app_with_settings();
        handle_enter(&mut app); // start editing
        handle_edit_char(&mut app, '5');
        if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            let edit = s.editing.as_ref().unwrap();
            assert!(edit.buffer.ends_with('5'));
        }
    }

    #[test]
    fn handle_edit_backspace_removes() {
        let mut app = app_with_settings();
        handle_enter(&mut app);
        handle_edit_char(&mut app, '5');
        handle_edit_backspace(&mut app);
        if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            let edit = s.editing.as_ref().unwrap();
            assert!(!edit.buffer.ends_with('5'));
        }
    }

    #[test]
    fn handle_edit_escape_cancels_edit() {
        let mut app = app_with_settings();
        handle_enter(&mut app);
        assert!(is_editing(&app));
        handle_edit_escape(&mut app);
        assert!(!is_editing(&app));
    }

    #[test]
    fn handle_reset_restores_original() {
        let mut app = app_with_settings();
        // Modify a field
        if let Some(Overlay::Settings(s)) = &mut app.layout.overlay
            && let Some(field) = s.current_field_mut()
        {
            field.value = serde_json::Value::Number(999.into());
        }
        handle_reset(&mut app);
        if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            assert!(!s.has_changes());
        }
    }

    #[test]
    fn handle_saved_closes_overlay() {
        let mut app = app_with_settings();
        handle_saved(&mut app);
        assert!(app.layout.overlay.is_none());
        assert!(app.viewport.error_toast.is_some());
    }

    #[test]
    fn hot_update_reports_saved_and_reloaded() {
        assert_eq!(save_outcome_message(&[]), "Config saved and reloaded");
    }

    #[test]
    fn cold_update_names_the_restart_paths() {
        let message = save_outcome_message(&["gateway.port".to_owned()]);
        assert!(
            !message.contains("reloaded"),
            "a cold change must not claim the running system reloaded: {message}"
        );
        assert!(message.contains("gateway.port"), "{message}");
    }

    #[test]
    fn mixed_update_names_every_restart_path() {
        let message = save_outcome_message(&["gateway.port".to_owned(), "gateway.bind".to_owned()]);
        assert!(message.contains("gateway.port"), "{message}");
        assert!(message.contains("gateway.bind"), "{message}");
    }

    #[test]
    fn restart_paths_reads_the_response_field() {
        let response = serde_json::json!({
            "section": "gateway",
            "config": {},
            "restart_required": ["gateway.port", "gateway.bind"],
        });
        assert_eq!(
            restart_paths(&response),
            vec!["gateway.port".to_owned(), "gateway.bind".to_owned()]
        );
    }

    #[test]
    fn restart_paths_absent_or_empty_means_no_restart() {
        assert!(restart_paths(&serde_json::json!({"section": "gateway"})).is_empty());
        assert!(restart_paths(&serde_json::json!({"restart_required": []})).is_empty());
    }

    #[test]
    fn finish_save_invalidates_the_render_cache() {
        // WHY: config changes can alter the syntax-highlighting theme, so a
        // completed save has to drop cached markdown. This ran only on the
        // message-driven path before, which nothing emits.
        let mut app = app_with_settings();
        app.viewport.render.markdown_cache.text = "cached".to_owned();
        app.viewport.render.markdown_cache.width = 80;
        finish_save(&mut app, &[]);
        assert!(
            app.viewport.render.markdown_cache.text.is_empty()
                && app.viewport.render.markdown_cache.width == 0,
            "a completed save must invalidate cached markdown"
        );
    }

    #[test]
    fn handle_save_error_sets_status() {
        let mut app = app_with_settings();
        handle_save_error(&mut app, "network error".to_string());
        if let Some(Overlay::Settings(s)) = &app.layout.overlay {
            assert!(matches!(s.save_status, SaveStatus::Error(_)));
        }
    }

    #[test]
    fn is_editing_false_by_default() {
        let app = app_with_settings();
        assert!(!is_editing(&app));
    }
}
