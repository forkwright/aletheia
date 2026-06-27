//! Tool definitions for planning tools.

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolDef, ToolGroupId,
    ToolTag,
};

pub(super) fn plan_create_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_create"), // kanon:ignore RUST/expect
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
                        ..Default::default()
                    },
                ),
                (
                    "description".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "What this project aims to accomplish".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "scope".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional scope constraint (e.g., 'crate X only')".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
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
                        ..Default::default()
                    },
                ),
                (
                    "appetite_minutes".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Time budget in minutes (only for 'quick' mode)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["name".to_owned(), "description".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan],
    }
}

pub(super) fn plan_research_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_research"), // kanon:ignore RUST/expect
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
                        ..Default::default()
                    },
                ),
                (
                    "skip".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Skip research and go directly to scoping".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["project_id".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan, ToolTag::Recon],
    }
}

pub(super) fn plan_requirements_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_requirements"), // kanon:ignore RUST/expect
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
                        ..Default::default()
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec!["start_scoping".to_owned(), "complete".to_owned()]),
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan],
    }
}

pub(super) fn plan_roadmap_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_roadmap"), // kanon:ignore RUST/expect
        description: "Manage project roadmap: add phases, start discussion or execution".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: plan_roadmap_properties(),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan],
    }
}

fn plan_roadmap_properties() -> IndexMap<String, PropertyDef> {
    IndexMap::from([
        string_property("project_id", "Project ID"),
        (
            "action".to_owned(),
            PropertyDef {
                property_type: PropertyType::String,
                description: "Action to perform".to_owned(),
                enum_values: Some(vec![
                    "add_phase".to_owned(),
                    "add_plan".to_owned(),
                    "start_discussion".to_owned(),
                    "start_execution".to_owned(),
                ]),
                default: None,
                ..Default::default()
            },
        ),
        string_property("phase_name", "Phase name (required for add_phase)"),
        string_property("phase_goal", "Phase goal (required for add_phase)"),
        string_property("phase_id", "Phase ID (required for add_plan)"),
        string_property(
            "plan_title",
            "Executable plan title (required for add_plan)",
        ),
        string_property(
            "plan_description",
            "Concrete work description for the executable plan",
        ),
        integer_property(
            "wave",
            "Execution wave; plans in the same wave may run in parallel",
            Some(serde_json::json!(1)),
        ),
        (
            "depends_on".to_owned(),
            PropertyDef {
                property_type: PropertyType::Array,
                description: "Plan IDs that must complete before this plan can run".to_owned(),
                enum_values: None,
                default: Some(serde_json::json!([])),
                ..Default::default()
            },
        ),
        integer_property(
            "max_iterations",
            "Optional maximum execution iterations before stuck",
            None,
        ),
    ])
}

fn string_property(name: &str, description: &str) -> (String, PropertyDef) {
    (
        name.to_owned(),
        PropertyDef {
            property_type: PropertyType::String,
            description: description.to_owned(),
            enum_values: None,
            default: None,
            ..Default::default()
        },
    )
}

fn integer_property(
    name: &str,
    description: &str,
    default: Option<serde_json::Value>,
) -> (String, PropertyDef) {
    (
        name.to_owned(),
        PropertyDef {
            property_type: PropertyType::Integer,
            description: description.to_owned(),
            enum_values: None,
            default,
            ..Default::default()
        },
    )
}

pub(super) fn plan_discuss_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_discuss"), // kanon:ignore RUST/expect
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
                        ..Default::default()
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec!["complete".to_owned()]),
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan],
    }
}

pub(super) fn plan_execute_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_execute"), // kanon:ignore RUST/expect
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
                        ..Default::default()
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
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan, ToolTag::Execute],
    }
}

pub(super) fn plan_verify_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_verify"), // kanon:ignore RUST/expect
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
                        ..Default::default()
                    },
                ),
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action to perform".to_owned(),
                        enum_values: Some(vec!["complete".to_owned(), "revert".to_owned()]),
                        default: None,
                        ..Default::default()
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
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["project_id".to_owned(), "action".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan, ToolTag::Verify],
    }
}

pub(super) fn plan_status_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_status"), // kanon:ignore RUST/expect
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
                    ..Default::default()
                },
            )]),
            required: vec!["project_id".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan, ToolTag::Recon],
    }
}

pub(super) fn plan_step_complete_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_step_complete"), // kanon:ignore RUST/expect
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
                        ..Default::default()
                    },
                ),
                (
                    "phase_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase ID containing the plan".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "plan_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Plan ID to mark complete".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "achievement".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Description of what was accomplished".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
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
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan, ToolTag::Edit],
    }
}

pub(super) fn plan_verify_criteria_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_verify_criteria"), // kanon:ignore RUST/expect
        description: "Verify phase success criteria with evidence and goal-backward tracing"
            .to_owned(),
        extended_description: Some(
            "Submit criterion evaluations for a phase. Each criterion includes status \
             (met/partially-met/not-met), evidence (file paths, test results), and detail. \
             Returns structured verification result with overall status, per-criterion \
             results, gaps with proposed fixes, and goal-backward traces."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project ID".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "phase_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase ID to verify".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "criteria".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "JSON array of criterion evaluations. Each: \
                            {criterion: string, status: 'met'|'partially-met'|'not-met', \
                            evidence: [{kind: string, content: string}], detail: string, \
                            proposed_fix?: string}"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec![
                "project_id".to_owned(),
                "phase_id".to_owned(),
                "criteria".to_owned(),
            ],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan, ToolTag::Verify],
    }
}

pub(super) fn plan_step_fail_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("plan_step_fail"), // kanon:ignore RUST/expect
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
                        ..Default::default()
                    },
                ),
                (
                    "phase_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Phase ID containing the plan".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "plan_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Plan ID to mark failed".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "reason".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Why the plan failed".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
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
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Plan],
        tags: vec![ToolTag::Plan, ToolTag::Edit],
    }
}
