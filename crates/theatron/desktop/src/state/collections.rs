//! Reactive collection patterns for file trees and entity lists.
//!
//! These types model dynamic, filterable, sortable collections. In Dioxus they
//! wrap in `Store<T>` to get field-level subscriptions. The `version` counter
//! provides a cheap change signal for components that only need "did it change?"
//! without reading the full vec.

use super::chat::NousId;

// ---------------------------------------------------------------------------
// Reactive list (generic pattern)
// ---------------------------------------------------------------------------

/// A versioned list that exposes a cheap change counter.
///
/// Components that render the full list subscribe to `items`.
/// Components that only show a count or badge subscribe to `version`.
#[derive(Debug, Clone)]
pub struct ReactiveList<T> {
    pub items: Vec<T>,
    /// Monotonically increasing counter, bumped on every mutation.
    pub version: u64,
}

impl<T> Default for ReactiveList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            version: 0,
        }
    }
}

impl<T> ReactiveList<T> {
    /// Append an item and bump the version.
    pub fn push(&mut self, item: T) {
        self.items.push(item);
        self.version += 1;
    }

    /// Replace the entire list. Useful after a full API fetch.
    pub fn replace(&mut self, items: Vec<T>) {
        self.items = items;
        self.version += 1;
    }

    /// Remove an item by index.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index < self.items.len() {
            let item = self.items.remove(index);
            self.version += 1;
            Some(item)
        } else {
            None
        }
    }

    /// Number of items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ---------------------------------------------------------------------------
// File tree
// ---------------------------------------------------------------------------

/// Kind of file system entry.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

/// A single entry in the flat file tree. Directories are expandable/collapsible.
/// The tree is stored as a flat sorted vec with depth levels, not recursive nodes.
/// This simplifies virtual scrolling (O(visible) rendering).
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub depth: u16,
    pub kind: FileKind,
    /// Whether a directory's children are visible.
    pub expanded: bool,
    pub selected: bool,
}

/// State for the workspace file tree panel.
#[derive(Debug, Clone, Default)]
pub struct FileTreeState {
    pub entries: ReactiveList<FileEntry>,
    pub filter: String,
}

// ---------------------------------------------------------------------------
// Entity list (knowledge graph)
// ---------------------------------------------------------------------------

/// A knowledge graph entity displayed in the memory inspector.
#[derive(Debug, Clone)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
}

/// Sort order for the entity list.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EntitySort {
    #[default]
    Name,
    Type,
    RecentlyUpdated,
}

/// State for the entity list in the memory inspector.
#[derive(Debug, Clone, Default)]
pub struct EntityListState {
    pub entities: ReactiveList<Entity>,
    pub sort: EntitySort,
    pub filter_text: String,
    pub selected: Option<usize>,
}

// ---------------------------------------------------------------------------
// Session list
// ---------------------------------------------------------------------------

/// A session entry in the session picker.
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub id: String,
    pub nous_id: NousId,
    pub display_name: Option<String>,
    pub message_count: u32,
    pub is_archived: bool,
    pub updated_at: Option<String>,
}

/// State for the session list (per-agent or global).
#[derive(Debug, Clone, Default)]
pub struct SessionListState {
    pub sessions: ReactiveList<SessionEntry>,
    pub show_archived: bool,
    pub selected: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reactive_list_default_empty() {
        let list: ReactiveList<i32> = ReactiveList::default();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.version, 0);
    }

    #[test]
    fn reactive_list_push_bumps_version() {
        let mut list = ReactiveList::default();
        list.push(1);
        assert_eq!(list.len(), 1);
        assert_eq!(list.version, 1);
        list.push(2);
        assert_eq!(list.version, 2);
    }

    #[test]
    fn reactive_list_replace_bumps_version() {
        let mut list = ReactiveList::default();
        list.push(1);
        list.push(2);
        let v_before = list.version;
        list.replace(vec![10, 20, 30]);
        assert_eq!(list.len(), 3);
        assert_eq!(list.version, v_before + 1);
        assert_eq!(list.items, vec![10, 20, 30]);
    }

    #[test]
    fn reactive_list_remove_bumps_version() {
        let mut list = ReactiveList::default();
        list.push("a");
        list.push("b");
        let v_before = list.version;
        let removed = list.remove(0);
        assert_eq!(removed, Some("a"));
        assert_eq!(list.len(), 1);
        assert_eq!(list.version, v_before + 1);
    }

    #[test]
    fn reactive_list_remove_out_of_range() {
        let mut list: ReactiveList<i32> = ReactiveList::default();
        list.push(1);
        let v_before = list.version;
        assert!(list.remove(5).is_none());
        assert_eq!(list.version, v_before); // no bump
    }

    #[test]
    fn file_tree_default() {
        let tree = FileTreeState::default();
        assert!(tree.entries.is_empty());
        assert!(tree.filter.is_empty());
    }

    #[test]
    fn entity_sort_default_is_name() {
        assert_eq!(EntitySort::default(), EntitySort::Name);
    }

    #[test]
    fn entity_list_default() {
        let list = EntityListState::default();
        assert!(list.entities.is_empty());
        assert!(list.filter_text.is_empty());
        assert!(list.selected.is_none());
    }

    #[test]
    fn session_list_default() {
        let list = SessionListState::default();
        assert!(list.sessions.is_empty());
        assert!(!list.show_archived);
        assert!(list.selected.is_none());
    }
}
