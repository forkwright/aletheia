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
