//! Planning views: project dashboard, requirements, roadmap, checkpoints, and verification.

pub(crate) mod category_proposal;
pub(crate) mod checkpoints;
pub(crate) mod dashboard;
pub(crate) mod discussion;
pub(crate) mod discussion_detail;
pub(crate) mod execution;
pub(crate) mod gap_analysis;
pub(crate) mod project_detail;
pub(crate) mod requirements;
pub(crate) mod roadmap;
pub(crate) mod verification;

pub(crate) use dashboard::Planning;
pub(crate) use project_detail::PlanningProject;
