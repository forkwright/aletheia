//! View state preservation for neurodivergent UX.
//!
//! WHY: Context switches cost ~23 minutes to recover from (research reference
//! in #2411). Preserving scroll position and input drafts across view switches
//! eliminates the UI-imposed context tax. Each view's ephemeral state is
//! captured before navigation and restored on return.

use std::collections::HashMap;

/// Key identifying a view for state preservation.
///
/// Maps 1:1 to the sidebar navigation routes. Parameterised routes
/// (e.g. `/planning/:project_id`) use the route string as key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ViewKey {
    Chat,
    Files,
    Memory,
    Sessions,
}

/// Preserved state for a single view.
#[derive(Debug, Clone, Default)]
pub(crate) struct PreservedViewState {
    /// Vertical scroll offset in pixels.
    pub scroll_top: f64,
    /// Draft input text (chat textarea, search bars, etc.).
    pub input_text: String,

}

/// Store holding preserved state for all views.
///
/// Provided as `Signal<ViewPreservationStore>` at the layout level so it
/// survives route changes. Views call `save()` before unmounting and
/// `restore()` after mounting.
#[derive(Debug, Clone, Default)]
pub(crate) struct ViewPreservationStore {
    states: HashMap<ViewKey, PreservedViewState>,
}

impl ViewPreservationStore {
    /// Create an empty store.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Save state for a view. Overwrites any previously saved state.
    pub(crate) fn save(&mut self, key: ViewKey, state: PreservedViewState) {
        self.states.insert(key, state);
    }

    /// Retrieve and remove saved state for a view.
    ///
    /// Returns `None` if no state was saved (first visit).
    #[must_use]
    pub(crate) fn restore(&mut self, key: &ViewKey) -> Option<PreservedViewState> {
        self.states.remove(key)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_restore_roundtrips() {
        let mut store = ViewPreservationStore::new();

        store.save(
            ViewKey::Chat,
            PreservedViewState {
                scroll_top: 42.0,
                input_text: "draft message".to_string(),
                secondary_scroll: 0.0,
            },
        );

        let restored = store.restore(&ViewKey::Chat);
        assert!(restored.is_some());
        let state = restored.unwrap();
        assert!((state.scroll_top - 42.0).abs() < f64::EPSILON);
        assert_eq!(state.input_text, "draft message");
    }

    #[test]
    fn restore_returns_none_on_first_visit() {
        let mut store = ViewPreservationStore::new();
        assert!(store.restore(&ViewKey::Memory).is_none());
    }

    #[test]
    fn save_overwrites_previous() {
        let mut store = ViewPreservationStore::new();
        store.save(
            ViewKey::Files,
            PreservedViewState {
                scroll_top: 100.0,
                ..Default::default()
            },
        );
        store.save(
            ViewKey::Files,
            PreservedViewState {
                scroll_top: 200.0,
                ..Default::default()
            },
        );

        let restored = store.restore(&ViewKey::Files).unwrap();
        assert!((restored.scroll_top - 200.0).abs() < f64::EPSILON);
    }
}
