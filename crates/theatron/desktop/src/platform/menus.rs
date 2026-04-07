//! Native menu bar definition and event mapping.
//!
//! Defines the application menu structure with keyboard accelerators.
//! The actual menu rendering is handled by the Dioxus desktop runtime
//! (backed by the `muda` crate); this module provides the menu model
//! and maps menu item IDs to application actions.

#[cfg(test)]
mod tests {
    use theatron_core::api::types::Agent;
    use theatron_core::id::NousId;

    use crate::state::agents::AgentStore;

    fn make_store(names: &[&str]) -> AgentStore {
        let mut store = AgentStore::new();
        let agents: Vec<Agent> = names
            .iter()
            .map(|n| Agent {
                id: NousId::from(*n),
                name: Some(n.to_string()),
                model: None,
                emoji: None,
            })
            .collect();
        store.load_agents(agents);
        store
    }
}
