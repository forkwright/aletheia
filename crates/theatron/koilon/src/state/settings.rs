use std::collections::HashMap;

#[derive(Debug)]
pub(crate) struct SettingsOverlay {
    pub(crate) sections: Vec<SettingsSection>,
    pub(crate) cursor: usize,
    pub(crate) editing: Option<EditState>,
    pub(crate) pending_changes: HashMap<String, serde_json::Value>,
    pub(crate) save_status: SaveStatus,
    pub(crate) scroll_offset: usize,
}

#[derive(Debug)]
pub(crate) struct SettingsSection {
    pub(crate) name: String,
    pub(crate) fields: Vec<SettingsField>,
}

#[derive(Debug, Clone)]
pub(crate) struct SettingsField {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) value: serde_json::Value,
    pub(crate) original_value: serde_json::Value,
    pub(crate) field_type: FieldType,
    pub(crate) editable: bool,
    pub(crate) requires_restart: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum FieldType {
    Bool,
    Integer,
    #[expect(
        dead_code,
        reason = "used when text editing fields are added to settings"
    )]
    Text,
    ReadOnly,
}

#[derive(Debug)]
pub(crate) struct EditState {
    pub(crate) buffer: String,
    pub(crate) cursor: usize,
}

#[derive(Debug)]
#[non_exhaustive]
pub(crate) enum SaveStatus {
    Idle,
    Saving,
    #[expect(
        dead_code,
        reason = "matched in view/settings.rs; no constructor path yet"
    )]
    Success,
    Error(String),
}

impl SettingsOverlay {
    pub(crate) fn from_config(config: &serde_json::Value) -> Self {
        let sections = build_sections(config);
        Self {
            sections,
            cursor: 0,
            editing: None,
            pending_changes: HashMap::new(),
            save_status: SaveStatus::Idle,
            scroll_offset: 0,
        }
    }

    pub(crate) fn total_fields(&self) -> usize {
        self.sections.iter().map(|s| s.fields.len()).sum()
    }

    pub(crate) fn current_field(&self) -> Option<&SettingsField> {
        let mut idx = 0;
        for section in &self.sections {
            for field in &section.fields {
                if idx == self.cursor {
                    return Some(field);
                }
                idx += 1;
            }
        }
        None
    }

    pub(crate) fn current_field_mut(&mut self) -> Option<&mut SettingsField> {
        let mut idx = 0;
        for section in &mut self.sections {
            for field in &mut section.fields {
                if idx == self.cursor {
                    return Some(field);
                }
                idx += 1;
            }
        }
        None
    }

    pub(crate) fn has_changes(&self) -> bool {
        self.sections
            .iter()
            .any(|s| s.fields.iter().any(|f| f.value != f.original_value))
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "parts.len() == 2 guard ensures indices 0 and 1 are valid before accessing them"
    )]
    pub(crate) fn changed_sections(&self) -> HashMap<String, serde_json::Value> {
        let mut result: HashMap<String, serde_json::Value> = HashMap::new();
        for section in &self.sections {
            for field in &section.fields {
                if field.value != field.original_value {
                    let parts: Vec<&str> = field.key.splitn(2, '.').collect();
                    if parts.len() == 2 {
                        let section_key = parts[0];
                        let remainder = parts[1];
                        let section_val =
                            result.entry(section_key.to_owned()).or_insert_with(|| {
                                serde_json::Value::Object(serde_json::Map::default())
                            });
                        set_nested(section_val, remainder, field.value.clone());
                    }
                }
            }
        }
        result
    }

    pub(crate) fn reset(&mut self) {
        for section in &mut self.sections {
            for field in &mut section.fields {
                field.value = field.original_value.clone();
            }
        }
        self.pending_changes.clear();
        self.save_status = SaveStatus::Idle;
    }
}

/// Set a value at a dotted path within a JSON object, creating intermediate objects as needed.
fn set_nested(root: &mut serde_json::Value, dotted_path: &str, value: serde_json::Value) {
    let parts: Vec<&str> = dotted_path.split('.').collect();
    let mut current = root;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let serde_json::Value::Object(map) = current {
                map.insert((*part).to_owned(), value);
            }
            return;
        }
        if let serde_json::Value::Object(map) = current {
            current = map
                .entry((*part).to_owned())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::default()));
        } else {
            return;
        }
    }
}

fn build_sections(config: &serde_json::Value) -> Vec<SettingsSection> {
    let mut sections = Vec::new();

    if let Some(agents) = config.get("agents").and_then(|a| a.get("defaults")) {
        sections.push(SettingsSection {
            name: "Pipeline".to_owned(),
            fields: vec![
                field(
                    "agents.defaults.maxToolIterations",
                    "Max Tool Iterations",
                    agents.get("maxToolIterations"),
                    FieldType::Integer,
                    true,
                    false,
                ),
                field(
                    "agents.defaults.thinkingEnabled",
                    "Thinking Enabled",
                    agents.get("thinkingEnabled"),
                    FieldType::Bool,
                    true,
                    false,
                ),
                field(
                    "agents.defaults.thinkingBudget",
                    "Thinking Budget",
                    agents.get("thinkingBudget"),
                    FieldType::Integer,
                    true,
                    false,
                ),
                field(
                    "agents.defaults.contextTokens",
                    "Context Window",
                    agents.get("contextTokens"),
                    FieldType::Integer,
                    true,
                    false,
                ),
                field(
                    "agents.defaults.maxOutputTokens",
                    "Max Output Tokens",
                    agents.get("maxOutputTokens"),
                    FieldType::Integer,
                    true,
                    false,
                ),
            ],
        });

        if let Some(timeouts) = agents.get("toolTimeouts") {
            sections.push(SettingsSection {
                name: "Tool Timeouts".to_owned(),
                fields: vec![field(
                    "agents.defaults.toolTimeouts.defaultMs",
                    "Default (ms)",
                    timeouts.get("defaultMs"),
                    FieldType::Integer,
                    true,
                    false,
                )],
            });
        }
    }

    if let Some(gw) = config.get("gateway") {
        sections.push(SettingsSection {
            name: "Gateway".to_owned(),
            fields: vec![
                field(
                    "gateway.port",
                    "Port",
                    gw.get("port"),
                    FieldType::ReadOnly,
                    false,
                    true,
                ),
                field(
                    "gateway.bind",
                    "Bind",
                    gw.get("bind"),
                    FieldType::ReadOnly,
                    false,
                    true,
                ),
            ],
        });
    }

    if let Some(emb) = config.get("embedding") {
        sections.push(SettingsSection {
            name: "Embedding".to_owned(),
            fields: vec![
                field(
                    "embedding.provider",
                    "Provider",
                    emb.get("provider"),
                    FieldType::ReadOnly,
                    false,
                    false,
                ),
                field(
                    "embedding.dimension",
                    "Dimension",
                    emb.get("dimension"),
                    FieldType::ReadOnly,
                    false,
                    false,
                ),
            ],
        });
    }

    if let Some(data) = config.get("data").and_then(|d| d.get("retention")) {
        sections.push(SettingsSection {
            name: "Data Retention".to_owned(),
            fields: vec![
                field(
                    "data.retention.sessionMaxAgeDays",
                    "Session Max Age (days)",
                    data.get("sessionMaxAgeDays"),
                    FieldType::Integer,
                    true,
                    false,
                ),
                field(
                    "data.retention.orphanMessageMaxAgeDays",
                    "Orphan Msg Max Age (days)",
                    data.get("orphanMessageMaxAgeDays"),
                    FieldType::Integer,
                    true,
                    false,
                ),
                field(
                    "data.retention.archiveBeforeDelete",
                    "Archive Before Delete",
                    data.get("archiveBeforeDelete"),
                    FieldType::Bool,
                    true,
                    false,
                ),
            ],
        });
    }

    if let Some(maint) = config.get("maintenance") {
        let mut fields = Vec::new();
        if let Some(tr) = maint.get("traceRotation") {
            fields.push(field(
                "maintenance.traceRotation.enabled",
                "Trace Rotation",
                tr.get("enabled"),
                FieldType::Bool,
                true,
                false,
            ));
            fields.push(field(
                "maintenance.traceRotation.maxAgeDays",
                "Max Age (days)",
                tr.get("maxAgeDays"),
                FieldType::Integer,
                true,
                false,
            ));
        }
        if let Some(db) = maint.get("dbMonitoring") {
            fields.push(field(
                "maintenance.dbMonitoring.enabled",
                "DB Monitoring",
                db.get("enabled"),
                FieldType::Bool,
                true,
                false,
            ));
            fields.push(field(
                "maintenance.dbMonitoring.warnThresholdMb",
                "Warn (MB)",
                db.get("warnThresholdMb"),
                FieldType::Integer,
                true,
                false,
            ));
            fields.push(field(
                "maintenance.dbMonitoring.alertThresholdMb",
                "Alert (MB)",
                db.get("alertThresholdMb"),
                FieldType::Integer,
                true,
                false,
            ));
        }
        if let Some(pro) = maint.get("prosoche") {
            // NOTE: mode is read-only because the set of valid values is an enum
            // (daemon | external | both | disabled) and FieldType does not yet
            // support constrained choice editing.
            fields.push(field(
                "maintenance.prosoche.mode",
                "Prosoche mode",
                pro.get("mode"),
                FieldType::ReadOnly,
                false,
                true,
            ));
            if let Some(hb) = pro.get("heartbeat") {
                fields.push(field(
                    "maintenance.prosoche.heartbeat.enabled",
                    "Heartbeat enabled",
                    hb.get("enabled"),
                    FieldType::Bool,
                    true,
                    true,
                ));
                fields.push(field(
                    "maintenance.prosoche.heartbeat.intervalSecs",
                    "Heartbeat interval (s)",
                    hb.get("intervalSecs"),
                    FieldType::Integer,
                    true,
                    true,
                ));
                if let Some(aw) = hb.get("activeWindow") {
                    fields.push(field(
                        "maintenance.prosoche.heartbeat.activeWindow",
                        "Heartbeat active window",
                        Some(aw),
                        FieldType::ReadOnly,
                        false,
                        true,
                    ));
                }
            }
            if let Some(sa) = pro.get("selfAudit") {
                fields.push(field(
                    "maintenance.prosoche.selfAudit.enabled",
                    "Self-audit enabled",
                    sa.get("enabled"),
                    FieldType::Bool,
                    true,
                    true,
                ));
                fields.push(field(
                    "maintenance.prosoche.selfAudit.intervalSecs",
                    "Self-audit interval (s)",
                    sa.get("intervalSecs"),
                    FieldType::Integer,
                    true,
                    true,
                ));
                if let Some(aw) = sa.get("activeWindow") {
                    fields.push(field(
                        "maintenance.prosoche.selfAudit.activeWindow",
                        "Self-audit active window",
                        Some(aw),
                        FieldType::ReadOnly,
                        false,
                        true,
                    ));
                }
            }
            if let Some(et) = pro.get("externalTimer") {
                fields.push(field(
                    "maintenance.prosoche.externalTimer.enabled",
                    "External timer enabled",
                    et.get("enabled"),
                    FieldType::Bool,
                    true,
                    true,
                ));
                fields.push(field(
                    "maintenance.prosoche.externalTimer.taskId",
                    "External timer task id",
                    et.get("taskId"),
                    FieldType::ReadOnly,
                    false,
                    true,
                ));
                fields.push(field(
                    "maintenance.prosoche.externalTimer.intervalSecs",
                    "External timer interval (s)",
                    et.get("intervalSecs"),
                    FieldType::Integer,
                    true,
                    true,
                ));
            }
        }
        if !fields.is_empty() {
            sections.push(SettingsSection {
                name: "Maintenance".to_owned(),
                fields,
            });
        }
    }

    sections
}

fn field(
    key: &str,
    label: &str,
    value: Option<&serde_json::Value>,
    field_type: FieldType,
    editable: bool,
    requires_restart: bool,
) -> SettingsField {
    let val = value.cloned().unwrap_or(serde_json::Value::Null);
    SettingsField {
        key: key.to_owned(),
        label: label.to_owned(),
        value: val.clone(),
        original_value: val,
        field_type,
        editable,
        requires_restart,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    fn sample_config() -> serde_json::Value {
        serde_json::json!({
            "agents": {
                "defaults": {
                    "maxToolIterations": 10,
                    "thinkingEnabled": true,
                    "thinkingBudget": 2000,
                    "contextTokens": 8000,
                    "maxOutputTokens": 4000,
                    "toolTimeouts": {
                        "defaultMs": 5000
                    }
                }
            },
            "gateway": {
                "port": 18789,
                "bind": "0.0.0.0"
            },
            "embedding": {
                "provider": "openai",
                "dimension": 1536
            }
        })
    }

    #[test]
    fn from_config_creates_sections() {
        let overlay = SettingsOverlay::from_config(&sample_config());
        assert!(!overlay.sections.is_empty());
        assert_eq!(overlay.cursor, 0);
        assert!(overlay.editing.is_none());
    }

    #[test]
    fn from_config_pipeline_section_exists() {
        let overlay = SettingsOverlay::from_config(&sample_config());
        assert!(overlay.sections.iter().any(|s| s.name == "Pipeline"));
    }

    #[test]
    fn from_config_gateway_section_readonly() {
        let overlay = SettingsOverlay::from_config(&sample_config());
        let gw = overlay
            .sections
            .iter()
            .find(|s| s.name == "Gateway")
            .unwrap();
        for field in &gw.fields {
            assert!(!field.editable);
            assert_eq!(field.field_type, FieldType::ReadOnly);
        }
    }

    #[test]
    fn total_fields_sums_correctly() {
        let overlay = SettingsOverlay::from_config(&sample_config());
        let expected: usize = overlay.sections.iter().map(|s| s.fields.len()).sum();
        assert_eq!(overlay.total_fields(), expected);
        assert!(expected > 0);
    }

    #[test]
    fn current_field_at_zero() {
        let overlay = SettingsOverlay::from_config(&sample_config());
        let field = overlay.current_field();
        assert!(field.is_some());
    }

    #[test]
    fn current_field_beyond_range() {
        let mut overlay = SettingsOverlay::from_config(&sample_config());
        overlay.cursor = 999;
        assert!(overlay.current_field().is_none());
    }

    #[test]
    fn has_changes_false_initially() {
        let overlay = SettingsOverlay::from_config(&sample_config());
        assert!(!overlay.has_changes());
    }

    #[test]
    fn has_changes_true_after_modification() {
        let mut overlay = SettingsOverlay::from_config(&sample_config());
        if let Some(field) = overlay.current_field_mut() {
            field.value = serde_json::Value::Number(999.into());
        }
        assert!(overlay.has_changes());
    }

    #[test]
    fn reset_reverts_changes() {
        let mut overlay = SettingsOverlay::from_config(&sample_config());
        if let Some(field) = overlay.current_field_mut() {
            field.value = serde_json::Value::Number(999.into());
        }
        assert!(overlay.has_changes());
        overlay.reset();
        assert!(!overlay.has_changes());
    }

    #[test]
    fn changed_sections_returns_modified() {
        let mut overlay = SettingsOverlay::from_config(&sample_config());
        // Modify the first field (maxToolIterations)
        if let Some(field) = overlay.current_field_mut() {
            field.value = serde_json::Value::Number(20.into());
        }
        let changed = overlay.changed_sections();
        assert!(!changed.is_empty());
        assert!(changed.contains_key("agents"));
    }

    #[test]
    fn changed_sections_empty_when_no_changes() {
        let overlay = SettingsOverlay::from_config(&sample_config());
        let changed = overlay.changed_sections();
        assert!(changed.is_empty());
    }

    #[test]
    fn set_nested_single_level() {
        let mut root = serde_json::json!({});
        set_nested(&mut root, "key", serde_json::Value::Bool(true));
        assert_eq!(root.get("key").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn set_nested_multi_level() {
        let mut root = serde_json::json!({});
        set_nested(&mut root, "a.b.c", serde_json::Value::Number(42.into()));
        assert_eq!(
            root.get("a")
                .and_then(|v| v.get("b"))
                .and_then(|v| v.get("c"))
                .and_then(|v| v.as_u64()),
            Some(42)
        );
    }

    #[test]
    fn set_nested_non_object_root_noop() {
        let mut root = serde_json::Value::String("not an object".to_string());
        set_nested(&mut root, "key", serde_json::Value::Bool(true));
        assert!(root.is_string()); // unchanged
    }

    #[test]
    fn from_config_empty_json() {
        let overlay = SettingsOverlay::from_config(&serde_json::json!({}));
        assert!(overlay.sections.is_empty());
        assert_eq!(overlay.total_fields(), 0);
    }

    #[test]
    fn from_config_with_maintenance() {
        let config = serde_json::json!({
            "maintenance": {
                "traceRotation": {
                    "enabled": true,
                    "maxAgeDays": 30
                },
                "dbMonitoring": {
                    "enabled": false,
                    "warnThresholdMb": 500,
                    "alertThresholdMb": 1000
                }
            }
        });
        let overlay = SettingsOverlay::from_config(&config);
        assert!(overlay.sections.iter().any(|s| s.name == "Maintenance"));
        let maint = overlay
            .sections
            .iter()
            .find(|s| s.name == "Maintenance")
            .unwrap();
        assert_eq!(maint.fields.len(), 5);
    }

    #[test]
    fn from_config_with_prosoche() {
        let config = serde_json::json!({
            "maintenance": {
                "prosoche": {
                    "mode": "daemon",
                    "heartbeat": {
                        "enabled": true,
                        "intervalSecs": 2700,
                        "activeWindow": { "startHour": 8, "endHour": 23 }
                    },
                    "selfAudit": {
                        "enabled": true,
                        "intervalSecs": 21600,
                        "activeWindow": { "startHour": 8, "endHour": 23 }
                    },
                    "externalTimer": {
                        "enabled": false,
                        "taskId": "prosoche-self-audit",
                        "intervalSecs": 300
                    }
                }
            }
        });
        let overlay = SettingsOverlay::from_config(&config);
        assert!(overlay.sections.iter().any(|s| s.name == "Maintenance"));
        let maint = overlay
            .sections
            .iter()
            .find(|s| s.name == "Maintenance")
            .unwrap();

        let keys: Vec<&str> = maint.fields.iter().map(|f| f.key.as_str()).collect();
        assert!(keys.contains(&"maintenance.prosoche.mode"));
        assert!(keys.contains(&"maintenance.prosoche.heartbeat.enabled"));
        assert!(keys.contains(&"maintenance.prosoche.heartbeat.intervalSecs"));
        assert!(keys.contains(&"maintenance.prosoche.selfAudit.enabled"));
        assert!(keys.contains(&"maintenance.prosoche.externalTimer.taskId"));

        let mode = maint
            .fields
            .iter()
            .find(|f| f.key == "maintenance.prosoche.mode")
            .unwrap();
        assert_eq!(mode.field_type, FieldType::ReadOnly);
        assert!(!mode.editable);

        let hb = maint
            .fields
            .iter()
            .find(|f| f.key == "maintenance.prosoche.heartbeat.enabled")
            .unwrap();
        assert_eq!(hb.field_type, FieldType::Bool);
        assert!(hb.editable);

        let task_id = maint
            .fields
            .iter()
            .find(|f| f.key == "maintenance.prosoche.externalTimer.taskId")
            .unwrap();
        assert_eq!(task_id.field_type, FieldType::ReadOnly);
        assert_eq!(task_id.value, serde_json::json!("prosoche-self-audit"));
    }

    #[test]
    fn changed_sections_reconstructs_prosoche() {
        let config = serde_json::json!({
            "maintenance": {
                "prosoche": {
                    "heartbeat": { "enabled": true, "intervalSecs": 2700 }
                }
            }
        });
        let mut overlay = SettingsOverlay::from_config(&config);
        // Cursor order: mode (0), heartbeat.enabled (1), heartbeat.intervalSecs (2)
        overlay.cursor = 2;
        if let Some(f) = overlay.current_field_mut() {
            f.value = serde_json::json!(3600);
        }
        let changed = overlay.changed_sections();
        let interval = changed
            .get("maintenance")
            .and_then(|v| v.get("prosoche"))
            .and_then(|v| v.get("heartbeat"))
            .and_then(|v| v.get("intervalSecs"))
            .and_then(|v| v.as_u64());
        assert_eq!(interval, Some(3600));
    }

    #[test]
    fn from_config_with_data_retention() {
        let config = serde_json::json!({
            "data": {
                "retention": {
                    "sessionMaxAgeDays": 90,
                    "orphanMessageMaxAgeDays": 30,
                    "archiveBeforeDelete": true
                }
            }
        });
        let overlay = SettingsOverlay::from_config(&config);
        assert!(overlay.sections.iter().any(|s| s.name == "Data Retention"));
    }

    #[test]
    fn field_constructor_null_default() {
        let f = field("test.key", "Test", None, FieldType::Integer, true, false);
        assert!(f.value.is_null());
        assert!(f.original_value.is_null());
    }
}
