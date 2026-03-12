//! Tool definitions for planning tools.

use indexmap::IndexMap;

use crate::types::{InputSchema, PropertyDef, PropertyType, ToolCategory, ToolDef};
use aletheia_koina::id::ToolName;

pub(super) fn plan_create_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_create").expect("valid tool name"),
        description: "Create a new planning project with phases and plans".to_owned(),
        extended_description: Some(
            "Creates a multi-phase planning project. Modes: 'full' (research through verification), \
             'quick' (time-boxed task with appetite_minutes), 'background' (autonomous processing)."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "name".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project name".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "description".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "What this project aims to accomplish".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "scope".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional scope constraint (e.g., 'crate X only')".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "mode".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Planning mode".to_owned(),
                        enum_values: Some(vec![
                            "full".to_owned(),
                            "quick".to_owned(),
                            "background".to_owned(),
                        ]),
                        default: Some(serde_json::json!("full")),
                    },
                ),
                (
                    "appetite_minutes".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Time budget in minutes (only for 'quick' mode)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["name".to_owned(), "description".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

pub(super) fn plan_research_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_research").expect("valid tool name"),
        description: "Advance project to research phase or skip research".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "skip".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Skip research and go directly to scoping".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["project_id".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

pub(super) fn plan_requirements_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_requirements").expect("valid tool name"),
        description: "Manage requirements scoping phase".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec!["start_scoping".to_owned(), "complete".to_owned()]),
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

pub(super) fn plan_roadmap_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_roadmap").expect("valid tool name"),
        description: "Manage project roadmap: add phases, start discussion or execution".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec![
                            "add_phase".to_owned(),
                            "start_discussion".to_owned(),
                            "start_execution".to_owned(),
                        ]),
                        default: None,
                    },
                ),
                (
                    "phase_name".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase name (required for add_phase)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "phase_goal".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase goal (required for add_phase)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

pub(super) fn plan_discuss_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_discuss").expect("valid tool name"),
        description: "Complete discussion phase and advance to execution".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec!["complete".to_owned()]),
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

pub(super) fn plan_execute_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_execute").expect("valid tool name"),
        description: "Manage plan execution: start, pause, resume, abandon, or verify".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec![
                            "start".to_owned(),
                            "pause".to_owned(),
                            "resume".to_owned(),
                            "abandon".to_owned(),
                            "start_verification".to_owned(),
                        ]),
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

pub(super) fn plan_verify_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_verify").expect("valid tool name"),
        description: "Complete verification or revert to an earlier phase".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec!["complete".to_owned(), "revert".to_owned()]),
                        default: None,
                    },
                ),
                (
                    "revert_to".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Target state for revert (required when action is 'revert')"
                            .to_owned(),
                        enum_values: Some(vec![
                            "scoping".to_owned(),
                            "planning".to_owned(),
                            "executing".to_owned(),
                        ]),
                        default: None,
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

pub(super) fn plan_status_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_status").expect("valid tool name"),
        description: "Get current project status including phases and completion".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([(
                "project_id".to_owned(),
                PropertyDef {
                    property_type: PropertyType::String,
                    description: "Project ID".to_owned(),
                    enum_values: None,
                    default: None,
                },
            )]),
            required: vec!["project_id".to_owned()],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

pub(super) fn plan_step_complete_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_step_complete").expect("valid tool name"),
        description: "Mark a plan step as successfully completed".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "phase_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase ID containing the plan".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "plan_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Plan ID to mark complete".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "achievement".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Description of what was accomplished".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "project_id".to_owned(),
                "phase_id".to_owned(),
                "plan_id".to_owned(),
            ],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}

pub(super) fn plan_step_fail_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("plan_step_fail").expect("valid tool name"),
        description: "Mark a plan step as failed with a reason".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "phase_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase ID containing the plan".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "plan_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Plan ID to mark failed".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "reason".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Why the plan failed".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "project_id".to_owned(),
                "phase_id".to_owned(),
                "plan_id".to_owned(),
                "reason".to_owned(),
            ],
        },
        category: ToolCategory::Planning,
        auto_activate: false,
    }
}
