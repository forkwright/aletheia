//! Scenario registry — all built-in behavioral scenarios.

mod auth;
mod conversation;
mod health;
mod nous;
mod session;

use crate::scenario::Scenario;

/// Return all built-in scenarios in execution order.
#[tracing::instrument(skip_all)]
pub fn all_scenarios() -> Vec<Box<dyn Scenario>> {
    let mut scenarios: Vec<Box<dyn Scenario>> = Vec::new();
    scenarios.extend(health::scenarios());
    scenarios.extend(auth::scenarios());
    scenarios.extend(nous::scenarios());
    scenarios.extend(session::scenarios());
    scenarios.extend(conversation::scenarios());
    scenarios
}
