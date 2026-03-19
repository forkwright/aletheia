//! Filesystem-backed adapter for the `PlanningService` trait.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use snafu::ResultExt;

use aletheia_dianoia::phase::Phase;
use aletheia_dianoia::plan::PlanState;
use aletheia_dianoia::project::{Project, ProjectMode};
use aletheia_dianoia::state::{ProjectState, Transition};
use aletheia_dianoia::workspace::ProjectWorkspace;
use aletheia_organon::error::{
    InvalidIdSnafu, InvalidModeSnafu, InvalidTransitionSnafu, IoSnafu, LoadProjectSnafu,
    NotFoundSnafu, PlanningAdapterError, SaveProjectSnafu, SerializeSnafu, TaskJoinSnafu,
    TransitionSnafu, WorkspaceSnafu,
};
use aletheia_organon::types::PlanningService;

pub(crate) struct FilesystemPlanningService {
    projects_root: PathBuf,
}

impl FilesystemPlanningService {
    pub(crate) fn new(projects_root: PathBuf) -> Self {
        Self { projects_root }
    }
}

impl PlanningService for FilesystemPlanningService {
    fn create_project(
        &self,
        name: &str,
        description: &str,
        scope: Option<&str>,
        mode: &str,
        appetite_minutes: Option<u32>,
        owner: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let root = self.projects_root.clone();
        let name = name.to_owned();
        let description = description.to_owned();
        let scope = scope.map(str::to_owned);
        let mode_str = mode.to_owned();
        let owner = owner.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mode = parse_mode(&mode_str, appetite_minutes)?;
                let mut project = Project::new(name, description, mode, owner);
                if let Some(s) = scope {
                    project.scope = Some(s);
                }
                let ws_path = root.join(project.id.to_string());
                let ws = ProjectWorkspace::create(&ws_path).map_err(|e| {
                    WorkspaceSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                ws.save_project(&project).map_err(|e| {
                    SaveProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                serde_json::to_string_pretty(&project).context(SerializeSnafu)
            })
            .await
            .context(TaskJoinSnafu)?
        })
    }

    fn load_project(
        &self,
        project_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let root = self.projects_root.clone();
        let project_id = project_id.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let ws_path = root.join(&project_id);
                let ws = ProjectWorkspace::open(&ws_path).map_err(|e| {
                    WorkspaceSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let project = ws.load_project().map_err(|e| {
                    LoadProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                serde_json::to_string_pretty(&project).context(SerializeSnafu)
            })
            .await
            .context(TaskJoinSnafu)?
        })
    }

    fn transition_project(
        &self,
        project_id: &str,
        transition: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let root = self.projects_root.clone();
        let project_id = project_id.to_owned();
        let transition_str = transition.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let ws_path = root.join(&project_id);
                let ws = ProjectWorkspace::open(&ws_path).map_err(|e| {
                    WorkspaceSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let mut project = ws.load_project().map_err(|e| {
                    LoadProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let transition = parse_transition(&transition_str).ok_or_else(|| {
                    InvalidTransitionSnafu {
                        name: transition_str,
                    }
                    .build()
                })?;
                project.advance(transition).map_err(|e| {
                    TransitionSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                ws.save_project(&project).map_err(|e| {
                    SaveProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                serde_json::to_string_pretty(&project).context(SerializeSnafu)
            })
            .await
            .context(TaskJoinSnafu)?
        })
    }

    fn add_phase(
        &self,
        project_id: &str,
        name: &str,
        goal: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let root = self.projects_root.clone();
        let project_id = project_id.to_owned();
        let name = name.to_owned();
        let goal = goal.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let ws_path = root.join(&project_id);
                let ws = ProjectWorkspace::open(&ws_path).map_err(|e| {
                    WorkspaceSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let mut project = ws.load_project().map_err(|e| {
                    LoadProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::as_conversions,
                    reason = "usize→u32: phase count fits in u32"
                )]
                let order = project.phases.len() as u32 + 1;
                let phase = Phase::new(name, goal, order);
                project.add_phase(phase);
                ws.save_project(&project).map_err(|e| {
                    SaveProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                serde_json::to_string_pretty(&project).context(SerializeSnafu)
            })
            .await
            .context(TaskJoinSnafu)?
        })
    }

    fn complete_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        achievement: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let root = self.projects_root.clone();
        let project_id = project_id.to_owned();
        let phase_id = phase_id.to_owned();
        let plan_id = plan_id.to_owned();
        let achievement = achievement.map(str::to_owned);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let ws_path = root.join(&project_id);
                let ws = ProjectWorkspace::open(&ws_path).map_err(|e| {
                    WorkspaceSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let mut project = ws.load_project().map_err(|e| {
                    LoadProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let plan = find_plan_mut(&mut project, &phase_id, &plan_id)?;
                plan.state = PlanState::Complete;
                if let Some(a) = achievement {
                    plan.achievements.push(a);
                }
                ws.save_project(&project).map_err(|e| {
                    SaveProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                serde_json::to_string_pretty(&project).context(SerializeSnafu)
            })
            .await
            .context(TaskJoinSnafu)?
        })
    }

    fn fail_plan(
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        reason: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let root = self.projects_root.clone();
        let project_id = project_id.to_owned();
        let phase_id = phase_id.to_owned();
        let plan_id = plan_id.to_owned();
        let reason = reason.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let ws_path = root.join(&project_id);
                let ws = ProjectWorkspace::open(&ws_path).map_err(|e| {
                    WorkspaceSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let mut project = ws.load_project().map_err(|e| {
                    LoadProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                let plan = find_plan_mut(&mut project, &phase_id, &plan_id)?;
                plan.state = PlanState::Failed;
                plan.blockers.push(aletheia_dianoia::plan::Blocker {
                    description: reason,
                    plan_id: plan.id,
                    detected_at: jiff::Timestamp::now(),
                });
                ws.save_project(&project).map_err(|e| {
                    SaveProjectSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
                serde_json::to_string_pretty(&project).context(SerializeSnafu)
            })
            .await
            .context(TaskJoinSnafu)?
        })
    }

    fn list_projects(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>> {
        let root = self.projects_root.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || list_projects_sync(&root))
                .await
                .context(TaskJoinSnafu)?
        })
    }
}

fn parse_mode(
    mode: &str,
    appetite_minutes: Option<u32>,
) -> Result<ProjectMode, PlanningAdapterError> {
    match mode {
        "full" => Ok(ProjectMode::Full),
        "quick" => {
            let minutes = appetite_minutes.unwrap_or(30);
            Ok(ProjectMode::Quick {
                appetite_minutes: minutes,
            })
        }
        "background" => Ok(ProjectMode::Background),
        other => InvalidModeSnafu {
            mode: other.to_owned(),
        }
        .fail(),
    }
}

fn parse_transition(s: &str) -> Option<Transition> {
    Some(match s {
        "start_questioning" => Transition::StartQuestioning,
        "start_research" => Transition::StartResearch,
        "skip_research" => Transition::SkipResearch,
        "skip_to_research" => Transition::SkipToResearch,
        "start_scoping" => Transition::StartScoping,
        "start_planning" => Transition::StartPlanning,
        "start_discussion" => Transition::StartDiscussion,
        "start_execution" => Transition::StartExecution,
        "start_verification" => Transition::StartVerification,
        "complete" => Transition::Complete,
        "abandon" => Transition::Abandon,
        "pause" => Transition::Pause,
        "resume" => Transition::Resume,
        "revert_to_scoping" => Transition::Revert {
            to: ProjectState::Scoping,
        },
        "revert_to_planning" => Transition::Revert {
            to: ProjectState::Planning,
        },
        "revert_to_executing" => Transition::Revert {
            to: ProjectState::Executing,
        },
        _ => return None,
    })
}

fn find_plan_mut<'a>(
    project: &'a mut Project,
    phase_id: &str,
    plan_id: &str,
) -> Result<&'a mut aletheia_dianoia::plan::Plan, PlanningAdapterError> {
    let phase_ulid: ulid::Ulid = phase_id.parse().map_err(|e: ulid::DecodeError| {
        InvalidIdSnafu {
            kind: "phase_id".to_owned(),
            message: e.to_string(),
        }
        .build()
    })?;
    let plan_ulid: ulid::Ulid = plan_id.parse().map_err(|e: ulid::DecodeError| {
        InvalidIdSnafu {
            kind: "plan_id".to_owned(),
            message: e.to_string(),
        }
        .build()
    })?;

    let phase = project
        .phases
        .iter_mut()
        .find(|p| p.id == phase_ulid)
        .ok_or_else(|| {
            NotFoundSnafu {
                kind: "phase",
                id: phase_id,
            }
            .build()
        })?;

    phase
        .plans
        .iter_mut()
        .find(|p| p.id == plan_ulid)
        .ok_or_else(|| {
            NotFoundSnafu {
                kind: "plan",
                id: plan_id,
            }
            .build()
        })
}

fn list_projects_sync(root: &Path) -> Result<String, PlanningAdapterError> {
    if !root.exists() {
        return Ok("[]".to_owned());
    }

    let entries = std::fs::read_dir(root).context(IoSnafu)?;
    let mut summaries = Vec::new();

    for entry in entries {
        let entry = entry.context(IoSnafu)?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let project_file = path.join("PROJECT.json");
        if !project_file.exists() {
            continue;
        }
        let contents = std::fs::read_to_string(&project_file).context(IoSnafu)?;
        let project: Project = match serde_json::from_str(&contents) {
            Ok(p) => p,
            Err(_) => continue,
        };
        summaries.push(serde_json::json!({
            "id": project.id.to_string(),
            "name": project.name,
            "state": format!("{:?}", project.state),
            "mode": format!("{:?}", project.mode),
            "owner": project.owner,
            "phase_count": project.phases.len(),
            "updated_at": project.updated_at.to_string(),
        }));
    }

    serde_json::to_string_pretty(&summaries).context(SerializeSnafu)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: serde_json indexing is safe (returns Null on missing key)"
)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let service = FilesystemPlanningService::new(dir.path().to_path_buf());

        let json = service
            .create_project("test-project", "a test", None, "full", None, "syn")
            .await
            .expect("create");

        let project: serde_json::Value = serde_json::from_str(&json).unwrap();
        let project_id = project["id"].as_str().unwrap();

        let loaded_json = service.load_project(project_id).await.expect("load");

        let loaded: serde_json::Value = serde_json::from_str(&loaded_json).unwrap();
        assert_eq!(loaded["name"], "test-project");
        assert_eq!(loaded["state"], "Created");
    }

    #[tokio::test]
    async fn transition_advances_state() {
        let dir = tempfile::tempdir().unwrap();
        let service = FilesystemPlanningService::new(dir.path().to_path_buf());

        let json = service
            .create_project("test", "desc", None, "full", None, "syn")
            .await
            .unwrap();
        let project: serde_json::Value = serde_json::from_str(&json).unwrap();
        let id = project["id"].as_str().unwrap();

        let json = service
            .transition_project(id, "start_questioning")
            .await
            .unwrap();
        let project: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(project["state"], "Questioning");
    }

    #[tokio::test]
    async fn invalid_transition_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let service = FilesystemPlanningService::new(dir.path().to_path_buf());

        let json = service
            .create_project("test", "desc", None, "full", None, "syn")
            .await
            .unwrap();
        let project: serde_json::Value = serde_json::from_str(&json).unwrap();
        let id = project["id"].as_str().unwrap();

        let result = service.transition_project(id, "start_verification").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_phase_appends() {
        let dir = tempfile::tempdir().unwrap();
        let service = FilesystemPlanningService::new(dir.path().to_path_buf());

        let json = service
            .create_project("test", "desc", None, "full", None, "syn")
            .await
            .unwrap();
        let project: serde_json::Value = serde_json::from_str(&json).unwrap();
        let id = project["id"].as_str().unwrap();

        let json = service
            .add_phase(id, "Foundation", "Set up core")
            .await
            .unwrap();
        let project: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(project["phases"].as_array().unwrap().len(), 1);
        assert_eq!(project["phases"][0]["name"], "Foundation");
    }

    #[tokio::test]
    async fn list_projects_returns_summaries() {
        let dir = tempfile::tempdir().unwrap();
        let service = FilesystemPlanningService::new(dir.path().to_path_buf());

        service
            .create_project("proj1", "first", None, "full", None, "syn")
            .await
            .unwrap();
        service
            .create_project("proj2", "second", None, "quick", Some(15), "syn")
            .await
            .unwrap();

        let json = service.list_projects().await.unwrap();
        let list: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(list.len(), 2);
    }
}
