//! Route builders for first-party Aletheia API clients.

/// Planning API routes that currently exist in pylon.
pub mod planning {
    /// Template for `GET` project verification.
    pub const PROJECT_VERIFICATION_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/verification";

    /// Template for `POST` project verification refresh.
    pub const PROJECT_VERIFICATION_REFRESH_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/verification/refresh";

    /// Build the path for `GET` project verification.
    #[must_use]
    pub fn project_verification_path(project_id: &str) -> String {
        format!("/api/v1/planning/projects/{project_id}/verification")
    }

    /// Build the absolute URL for `GET` project verification.
    #[must_use]
    pub fn project_verification_url(base_url: &str, project_id: &str) -> String {
        format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            project_verification_path(project_id)
        )
    }

    /// Build the path for `POST` project verification refresh.
    #[must_use]
    pub fn project_verification_refresh_path(project_id: &str) -> String {
        format!("/api/v1/planning/projects/{project_id}/verification/refresh")
    }

    /// Build the absolute URL for `POST` project verification refresh.
    #[must_use]
    pub fn project_verification_refresh_url(base_url: &str, project_id: &str) -> String {
        format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            project_verification_refresh_path(project_id)
        )
    }
}
