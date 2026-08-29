#![expect(clippy::expect_used, reason = "test assertions")]
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::error::PlanningAdapterError;
use crate::types::{PlanningPlanInput, PlanningService, ToolContext, ToolServices};

pub(super) fn test_ctx() -> ToolContext {
    crate::testing::make_test_context_without_services()
}

pub(super) fn test_ctx_with_planning(planning: Arc<dyn PlanningService>) -> ToolContext {
    crate::testing::make_test_context_with(ToolServices {
        planning: Some(planning),
        ..Default::default()
    })
}

#[derive(Default)]
pub(super) struct MockPlanning {
    pub(super) create_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    pub(super) load_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    pub(super) transition_calls: Mutex<Vec<(String, String)>>,
    pub(super) transition_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    pub(super) add_phase_calls: Mutex<Vec<(String, String, String)>>,
    pub(super) add_phase_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    pub(super) add_plan_calls: Mutex<Vec<(String, String, String, String)>>,
    pub(super) add_plan_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    pub(super) complete_plan_calls: Mutex<Vec<(String, String, String)>>,
    pub(super) complete_plan_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
    pub(super) fail_plan_calls: Mutex<Vec<(String, String, String, String)>>,
    pub(super) fail_plan_result: Mutex<Option<Result<String, PlanningAdapterError>>>,
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
        let result = self
            .create_result
            .lock()
            .expect("lock not poisoned")
            .take()
            .unwrap_or(Ok(
                r#"{"id":"01J0000000000000000000000","name":"test","state":"Created"}"#.to_owned(),
            ));
        Box::pin(async move { result })
    }

    fn load_project(
        &self,
        _project_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let result = self
            .load_result
            .lock()
            .expect("lock not poisoned")
            .take()
            .unwrap_or(Ok(
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
            .expect("lock not poisoned")
            .push((project_id.to_owned(), transition.to_owned()));
        let result = self
            .transition_result
            .lock()
            .expect("lock not poisoned")
            .take()
            .unwrap_or(Ok(
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
        self.add_phase_calls
            .lock()
            .expect("lock not poisoned")
            .push((project_id.to_owned(), name.to_owned(), goal.to_owned()));
        let result = self
            .add_phase_result
            .lock()
            .expect("lock not poisoned")
            .take()
            .unwrap_or(Ok(
                r#"{"id":"01J0000000000000000000000","phases":[{"name":"Phase 1"}]}"#.to_owned(),
            ));
        Box::pin(async move { result })
    }

    fn add_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan: PlanningPlanInput<'_>,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        self.add_plan_calls
            .lock()
            .expect("lock not poisoned")
            .push((
                project_id.to_owned(),
                phase_id.to_owned(),
                plan.title.to_owned(),
                plan.description.to_owned(),
            ));
        let result = self
            .add_plan_result
            .lock()
            .expect("lock not poisoned")
            .take()
            .unwrap_or(Ok(r#"{"status":"plan added"}"#.to_owned()));
        Box::pin(async move { result })
    }

    fn complete_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        _achievement: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        self.complete_plan_calls
            .lock()
            .expect("lock not poisoned")
            .push((
                project_id.to_owned(),
                phase_id.to_owned(),
                plan_id.to_owned(),
            ));
        let result = self
            .complete_plan_result
            .lock()
            .expect("lock not poisoned")
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
        self.fail_plan_calls
            .lock()
            .expect("lock not poisoned")
            .push((
                project_id.to_owned(),
                phase_id.to_owned(),
                plan_id.to_owned(),
                reason.to_owned(),
            ));
        let result = self
            .fail_plan_result
            .lock()
            .expect("lock not poisoned")
            .take()
            .unwrap_or(Ok(r#"{"status":"plan failed"}"#.to_owned()));
        Box::pin(async move { result })
    }

    fn list_projects(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        Box::pin(async { Ok("[]".to_owned()) })
    }

    fn verify_criteria(
        &self,
        _project_id: &str,
        _phase_id: &str,
        _criteria_json: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        Box::pin(async {
            Ok(r#"{"verification":{"status":"Met","summary":"all criteria met"},"goal_traces":[]}"#
                .to_owned())
        })
    }
}
