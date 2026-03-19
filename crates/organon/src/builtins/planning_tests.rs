#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::as_conversions,
    reason = "test: coercion to Box<dyn Error> trait object"
)]
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};

use snafu::IntoError;

use aletheia_koina::id::{NousId, SessionId, ToolName};

use crate::error::{PlanningAdapterError, SaveProjectSnafu};
use crate::registry::ToolRegistry;
use crate::types::{
    PlanningService, ServerToolConfig, ToolCategory, ToolContext, ToolInput, ToolServices,
};

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn test_ctx() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

fn test_ctx_with_planning(planning: Arc<dyn PlanningService>) -> ToolContext {
    install_crypto_provider();
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: Some(Arc::new(ToolServices {
            cross_nous: None,
            messenger: None,
            note_store: None,
            blackboard_store: None,
            spawn: None,
            planning: Some(planning),
            knowledge: None,
            http_client: reqwest::Client::new(),
            lazy_tool_catalog: vec![],
            server_tool_config: ServerToolConfig::default(),
        })),
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

#[derive(Default)]
struct MockPlanning {
    create_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    load_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    transition_calls: Mutex<Vec<(String, String)>>,
    transition_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    add_phase_calls: Mutex<Vec<(String, String, String)>>,
    add_phase_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    complete_plan_calls: Mutex<Vec<(String, String, String)>>,
    complete_plan_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    fail_plan_calls: Mutex<Vec<(String, String, String, String)>>,
    fail_plan_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
}

impl PlanningService for MockPlanning {
    fn create_project(
        &self,
        _name: &str,
        _description: &str,
        _scope: Option<&str>,
        _mode: &str,
        _appetite_minutes: Option<u32>,
        _owner: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let result = self.create_result.lock().unwrap().take().unwrap_or(Ok(
            r#"{"id":"01J0000000000000000000000","name":"test","state":"Created"}"#.to_owned(),
        ));
        Box::pin(async move { result })
    }

    fn load_project(
        &self,
        _project_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let result = self.load_result.lock().unwrap().take().unwrap_or(Ok(
            r#"{"id":"01J0000000000000000000000","state":"Created"}"#.to_owned(),
        ));
        Box::pin(async move { result })
    }

    fn transition_project(
        &self,
        project_id: &str,
        transition: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        self.transition_calls
            .lock()
            .unwrap()
            .push((project_id.to_owned(), transition.to_owned()));
        let result = self.transition_result.lock().unwrap().take().unwrap_or(Ok(
            r#"{"id":"01J0000000000000000000000","state":"Researching"}"#.to_owned(),
        ));
        Box::pin(async move { result })
    }

    fn add_phase(
        &self,
        project_id: &str,
        name: &str,
        goal: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        self.add_phase_calls.lock().unwrap().push((
            project_id.to_owned(),
            name.to_owned(),
            goal.to_owned(),
        ));
        let result = self.add_phase_result.lock().unwrap().take().unwrap_or(Ok(
            r#"{"id":"01J0000000000000000000000","phases":[{"name":"Phase 1"}]}"#.to_owned(),
        ));
        Box::pin(async move { result })
    }

    fn complete_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        _achievement: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        self.complete_plan_calls.lock().unwrap().push((
            project_id.to_owned(),
            phase_id.to_owned(),
            plan_id.to_owned(),
        ));
        let result = self
            .complete_plan_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or(Ok(r#"{"status":"plan completed"}"#.to_owned()));
        Box::pin(async move { result })
    }

    fn fail_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        reason: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        self.fail_plan_calls.lock().unwrap().push((
            project_id.to_owned(),
            phase_id.to_owned(),
            plan_id.to_owned(),
            reason.to_owned(),
        ));
        let result = self
            .fail_plan_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or(Ok(r#"{"status":"plan failed"}"#.to_owned()));
        Box::pin(async move { result })
    }

    fn list_projects(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        Box::pin(async { Ok("[]".to_owned()) })
    }
}

#[tokio::test]
async fn register_planning_tools() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let planning_tools = reg.definitions_for_category(ToolCategory::Planning);
    assert_eq!(
        planning_tools.len(),
        10,
        "expected 10 planning tools to be registered"
    );
}

#[tokio::test]
async fn all_tools_are_lazy() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    for def in reg.definitions_for_category(ToolCategory::Planning) {
        assert!(!def.auto_activate, "{} should be lazy", def.name.as_str());
    }
}

#[tokio::test]
async fn plan_create_missing_service_returns_error() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_create").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"name": "test", "description": "test project"}),
    };
    let result = reg.execute(&input, &test_ctx()).await.expect("execute");
    assert!(
        result.is_error,
        "plan_create without a planning service should return an error"
    );
    assert!(
        result.content.text_summary().contains("not configured"),
        "error message should indicate service is not configured"
    );
}

#[tokio::test]
async fn plan_create_success() {
    let mock = Arc::new(MockPlanning::default());
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_create").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"name": "my project", "description": "build a thing"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_create should succeed when service is available"
    );
    assert!(
        result.content.text_summary().contains("Created"),
        "response should include the Created state"
    );
}

#[tokio::test]
async fn plan_create_error_propagates() {
    let mock = Arc::new(MockPlanning::default());
    *mock.create_result.lock().unwrap() = Some(Err(SaveProjectSnafu.into_error(Box::new(
        std::io::Error::new(std::io::ErrorKind::AlreadyExists, "project already exists"),
    )
        as Box<dyn std::error::Error + Send + Sync>)));
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_create").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"name": "test", "description": "test"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        result.is_error,
        "plan_create should return an error when the service returns a failure"
    );
    assert!(
        result.content.text_summary().contains("already exists"),
        "error message should propagate the underlying cause"
    );
}

#[tokio::test]
async fn plan_research_skip_dispatches_correctly() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_research").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "01J0000000000000000000000", "skip": true}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_research with skip=true should succeed"
    );

    let calls = mock_ref.transition_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one transition call");
    assert_eq!(
        calls[0].1, "skip_research",
        "skip=true should dispatch the skip_research transition"
    );
}

#[tokio::test]
async fn plan_research_no_skip_dispatches_start() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_research").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "01J0000000000000000000000"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_research without skip should succeed"
    );

    let calls = mock_ref.transition_calls.lock().unwrap();
    assert_eq!(
        calls[0].1, "start_research",
        "omitting skip should dispatch the start_research transition"
    );
}

#[tokio::test]
async fn plan_roadmap_add_phase_calls_add_phase() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_roadmap").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({
            "project_id": "01J0000000000000000000000",
            "action": "add_phase",
            "phase_name": "Foundation",
            "phase_goal": "Set up core infrastructure"
        }),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(!result.is_error, "plan_roadmap add_phase should succeed");

    let calls = mock_ref.add_phase_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one add_phase call");
    assert_eq!(
        calls[0].1, "Foundation",
        "phase name should be passed through to the service"
    );
    assert_eq!(
        calls[0].2, "Set up core infrastructure",
        "phase goal should be passed through to the service"
    );

    let t_calls = mock_ref.transition_calls.lock().unwrap();
    assert!(
        t_calls.is_empty(),
        "add_phase should not trigger any state transition"
    );
}

#[tokio::test]
async fn plan_status_returns_project_json() {
    let mock = Arc::new(MockPlanning::default());
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_status").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "01J0000000000000000000000"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_status should succeed and return project data"
    );
    assert!(
        result.content.text_summary().contains("Created"),
        "response should include the project state"
    );
}

#[tokio::test]
async fn plan_step_complete_dispatches() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_step_complete").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({
            "project_id": "proj1",
            "phase_id": "phase1",
            "plan_id": "plan1",
            "achievement": "implemented the feature"
        }),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(!result.is_error, "plan_step_complete should succeed");

    let calls = mock_ref.complete_plan_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one complete_plan call");
    assert_eq!(
        calls[0],
        ("proj1".to_owned(), "phase1".to_owned(), "plan1".to_owned()),
        "project_id, phase_id, and plan_id should be forwarded correctly to the service"
    );
}

#[tokio::test]
async fn plan_step_fail_dispatches() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_step_fail").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({
            "project_id": "proj1",
            "phase_id": "phase1",
            "plan_id": "plan1",
            "reason": "compilation error"
        }),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(!result.is_error, "plan_step_fail should succeed");

    let calls = mock_ref.fail_plan_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one fail_plan call");
    assert_eq!(
        calls[0].3, "compilation error",
        "failure reason should be forwarded to the service"
    );
}

#[tokio::test]
async fn plan_verify_revert_dispatches_correctly() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_verify").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({
            "project_id": "proj1",
            "action": "revert",
            "revert_to": "planning"
        }),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(!result.is_error, "plan_verify revert action should succeed");

    let calls = mock_ref.transition_calls.lock().unwrap();
    assert_eq!(
        calls[0].1, "revert_to_planning",
        "revert action should dispatch the revert_to_planning transition"
    );
}

#[tokio::test]
async fn plan_execute_maps_actions() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");

    for (action, expected_transition) in [
        ("start", "start_execution"),
        ("pause", "pause"),
        ("resume", "resume"),
        ("abandon", "abandon"),
        ("start_verification", "start_verification"),
    ] {
        *mock_ref.transition_result.lock().unwrap() = Some(Ok(r#"{"state":"ok"}"#.to_owned()));

        let input = ToolInput {
            name: ToolName::new("plan_execute").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({
                "project_id": "proj1",
                "action": action,
            }),
        };
        reg.execute(&input, &ctx).await.expect("execute");

        let calls = mock_ref.transition_calls.lock().unwrap();
        let last = calls.last().expect("should have a call");
        assert_eq!(
            last.1, expected_transition,
            "action '{action}' should map to '{expected_transition}'"
        );
    }
}

#[tokio::test]
async fn unknown_action_returns_error() {
    let mock = Arc::new(MockPlanning::default());
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_requirements").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "p1", "action": "invalid_action"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        result.is_error,
        "plan_requirements with an invalid action should return an error"
    );
    assert!(
        result.content.text_summary().contains("unknown action"),
        "error message should indicate the action was not recognized"
    );
}

#[tokio::test]
async fn plan_requirements_start_scoping_dispatches() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_requirements").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "proj1", "action": "start_scoping"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_requirements start_scoping should succeed"
    );

    let calls = mock_ref.transition_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one transition call");
    assert_eq!(
        calls[0].1, "start_scoping",
        "start_scoping action should dispatch the start_scoping transition"
    );
}

#[tokio::test]
async fn plan_requirements_complete_dispatches_start_planning() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_requirements").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "proj1", "action": "complete"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_requirements complete action should succeed"
    );

    let calls = mock_ref.transition_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one transition call");
    assert_eq!(
        calls[0].1, "start_planning",
        "complete action should advance to the start_planning transition"
    );
}

#[tokio::test]
async fn plan_discuss_complete_dispatches_start_execution() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_discuss").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "proj1", "action": "complete"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_discuss complete action should succeed"
    );

    let calls = mock_ref.transition_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one transition call");
    assert_eq!(
        calls[0].1, "start_execution",
        "complete action on plan_discuss should dispatch the start_execution transition"
    );
}

#[tokio::test]
async fn plan_discuss_unknown_action_returns_error() {
    let mock = Arc::new(MockPlanning::default());
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_discuss").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "proj1", "action": "invalid"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        result.is_error,
        "plan_discuss with an invalid action should return an error"
    );
    assert!(
        result.content.text_summary().contains("unknown action"),
        "error message should indicate the action was not recognized"
    );
}

#[tokio::test]
async fn plan_roadmap_start_discussion_dispatches() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_roadmap").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "proj1", "action": "start_discussion"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_roadmap start_discussion action should succeed"
    );

    let calls = mock_ref.transition_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one transition call");
    assert_eq!(
        calls[0].1, "start_discussion",
        "start_discussion action should dispatch the start_discussion transition"
    );
}

#[tokio::test]
async fn plan_roadmap_start_execution_dispatches() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_roadmap").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "proj1", "action": "start_execution"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_roadmap start_execution action should succeed"
    );

    let calls = mock_ref.transition_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one transition call");
    assert_eq!(
        calls[0].1, "start_execution",
        "start_execution action should dispatch the start_execution transition"
    );
}

#[tokio::test]
async fn plan_verify_complete_dispatches() {
    let mock = Arc::new(MockPlanning::default());
    let mock_ref = Arc::clone(&mock);
    let ctx = test_ctx_with_planning(mock);
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("plan_verify").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"project_id": "proj1", "action": "complete"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !result.is_error,
        "plan_verify complete action should succeed"
    );

    let calls = mock_ref.transition_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one transition call");
    assert_eq!(
        calls[0].1, "complete",
        "complete action on plan_verify should dispatch the complete transition"
    );
}

#[tokio::test]
async fn plan_missing_service_returns_error_for_all_tools() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let ctx = test_ctx();

    for tool_name in [
        "plan_research",
        "plan_requirements",
        "plan_roadmap",
        "plan_discuss",
        "plan_execute",
        "plan_verify",
        "plan_status",
        "plan_step_complete",
        "plan_step_fail",
    ] {
        let args = serde_json::json!({"project_id": "p1", "action": "start"});
        let input = ToolInput {
            name: ToolName::new(tool_name).expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: args,
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(
            result.is_error,
            "{tool_name} should return error when service not configured"
        );
        assert!(
            result.content.text_summary().contains("not configured"),
            "{tool_name}: expected 'not configured' in error"
        );
    }
}
