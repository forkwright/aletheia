use std::collections::HashMap;

#[derive(Debug)]
pub struct SettingsOverlay {
    pub sections: Vec<SettingsSection>,
    pub cursor: usize,
    pub editing: Option<EditState>,
    pub pending_changes: HashMap<String, serde_json::Value>,
    pub save_status: SaveStatus,
    pub scroll_offset: usize,
}

#[derive(Debug)]
pub struct SettingsSection {
    pub name: String,
    pub fields: Vec<SettingsField>,
}

#[derive(Debug, Clone)]
pub struct SettingsField {
    pub key: String,
    pub label: String,
    pub value: serde_json::Value,
    pub original_value: serde_json::Value,
    pub field_type: FieldType,
    pub editable: bool,
    pub requires_restart: bool,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
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
pub struct EditState {
    pub buffer: String,
    pub cursor: usize,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum SaveStatus {
    Idle,
    Saving,
    #[expect(dead_code, reason = "set after successful config save")]
    Success,
    Error(String),
}

impl SettingsOverlay {
    pub fn from_config(config: &serde_json::Value) -> Self {
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

    pub fn total_fields(&self) -> usize {
        self.sections.iter().map(|s| s.fields.len()).sum()
    }

    pub fn current_field(&self) -> Option<&SettingsField> {
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

    pub fn current_field_mut(&mut self) -> Option<&mut SettingsField> {
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

    pub fn has_changes(&self) -> bool {
        self.sections
            .iter()
            .any(|s| s.fields.iter().any(|f| f.value != f.original_value))
    }

    pub fn changed_sections(&self) -> HashMap<String, serde_json::Value> {
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

    pub fn reset(&mut self) {
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
                field(
                    "agents.defaults.timeoutSeconds",
                    "Turn Timeout (s)",
                    agents.get("timeoutSeconds"),
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
                    "timeoutSeconds": 60,
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
        set_nested(
            &mut root,
            "a.b.c",
            serde_json::Value::Number(42.into()),
        );
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
        assert!(
            overlay
                .sections
                .iter()
                .any(|s| s.name == "Data Retention")
        );
    }

    #[test]
    fn field_constructor_null_default() {
        let f = field("test.key", "Test", None, FieldType::Integer, true, false);
        assert!(f.value.is_null());
        assert!(f.original_value.is_null());
    }
}
