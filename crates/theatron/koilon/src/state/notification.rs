use std::time::{Duration, Instant};

use crate::id::NousId;
use crate::msg::NotificationKind;

/// Auto-dismissing notification toast.
#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub kind: NotificationKind,
    pub duration_secs: u64,
    pub created_at: Instant,
}

impl Toast {
    pub(crate) fn new(message: String, kind: NotificationKind) -> Self {
        Self::with_duration(message, kind, 5)
    }

    pub(crate) fn with_duration(
        message: String,
        kind: NotificationKind,
        duration_secs: u64,
    ) -> Self {
        Self {
            message,
            kind,
            duration_secs,
            created_at: Instant::now(),
        }
    }

    pub(crate) fn is_expired(&self) -> bool {
        self.created_at.elapsed() > Duration::from_secs(self.duration_secs)
    }
}

/// Persistent top-of-viewport alert, dismissed explicitly.
#[derive(Debug, Clone)]
pub struct ErrorBanner {
    pub message: String,
}

/// Single entry in the cross-agent notification log.
#[derive(Debug, Clone)]
pub struct Notification {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "used for deduplication and future API serialization"
        )
    )]
    pub id: u64,
    pub nous_id: Option<NousId>,
    pub message: String,
    pub kind: NotificationKind,
    pub read: bool,
    #[expect(dead_code, reason = "used for future timestamp rendering")]
    pub created_at: Instant,
}

/// Maximum number of notifications retained in the store.
/// Oldest notifications are evicted when this cap is reached.
const MAX_NOTIFICATIONS: usize = 10_000;

/// Maximum number of toasts retained in the viewport queue.
/// Defense-in-depth: prevents unbounded growth if dismiss logic stalls.
pub(crate) const MAX_TOASTS: usize = 100;

/// Bounded log of notifications with read/unread tracking.
///
/// WHY: Without a cap, every notification appended over a long-running session
/// grows memory without bound. When the store reaches `MAX_NOTIFICATIONS`, the
/// oldest entries are drained to make room.
#[derive(Debug, Default)]
pub struct NotificationStore {
    pub items: Vec<Notification>,
    next_id: u64,
}

impl NotificationStore {
    pub(crate) fn push(
        &mut self,
        nous_id: Option<NousId>,
        message: String,
        kind: NotificationKind,
    ) {
        // WHY: drain from the front to evict oldest entries when at capacity.
        if self.items.len() >= MAX_NOTIFICATIONS {
            let drain_count = self.items.len() - MAX_NOTIFICATIONS + 1;
            self.items.drain(..drain_count);
        }
        self.items.push(Notification {
            id: self.next_id,
            nous_id,
            message,
            kind,
            read: false,
            created_at: Instant::now(),
        });
        self.next_id += 1;
    }

    pub(crate) fn unread_count(&self) -> usize {
        self.items.iter().filter(|n| !n.read).count()
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "per-agent unread badge; used in future sidebar rendering"
        )
    )]
    pub(crate) fn unread_count_for(&self, nous_id: &NousId) -> usize {
        self.items
            .iter()
            .filter(|n| n.nous_id.as_ref() == Some(nous_id) && !n.read)
            .count()
    }

    pub(crate) fn mark_all_read(&mut self) {
        for n in &mut self.items {
            n.read = true;
        }
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "per-agent mark-read; used in future overlay interactions"
        )
    )]
    pub(crate) fn mark_read_for(&mut self, nous_id: &NousId) {
        for n in self.items.iter_mut() {
            if n.nous_id.as_ref() == Some(nous_id) {
                n.read = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_not_expired_immediately() {
        let t = Toast::new("hello".to_string(), NotificationKind::Info);
        assert!(!t.is_expired());
    }

    #[test]
    fn toast_custom_duration_stores() {
        let t = Toast::with_duration("msg".to_string(), NotificationKind::Error, 10);
        assert_eq!(t.duration_secs, 10);
        assert_eq!(t.message, "msg");
    }

    #[test]
    fn notification_store_push_and_unread() {
        let mut store = NotificationStore::default();
        store.push(None, "a".to_string(), NotificationKind::Info);
        store.push(None, "b".to_string(), NotificationKind::Warning);
        assert_eq!(store.unread_count(), 2);
    }

    #[test]
    fn notification_store_mark_all_read() {
        let mut store = NotificationStore::default();
        store.push(None, "a".to_string(), NotificationKind::Info);
        store.mark_all_read();
        assert_eq!(store.unread_count(), 0);
    }

    #[test]
    fn notification_store_unread_count_for_agent() {
        let mut store = NotificationStore::default();
        let id: NousId = "syn".into();
        store.push(Some(id.clone()), "msg".to_string(), NotificationKind::Error);
        store.push(None, "global".to_string(), NotificationKind::Info);
        assert_eq!(store.unread_count_for(&id), 1);
        store.mark_read_for(&id);
        assert_eq!(store.unread_count_for(&id), 0);
        assert_eq!(store.unread_count(), 1);
    }

    #[test]
    fn notification_ids_monotonically_increase() {
        let mut store = NotificationStore::default();
        store.push(None, "a".to_string(), NotificationKind::Info);
        store.push(None, "b".to_string(), NotificationKind::Info);
        assert_eq!(store.items[0].id, 0);
        assert_eq!(store.items[1].id, 1);
    }

    #[test]
    fn error_banner_stores_message() {
        let b = ErrorBanner {
            message: "oops".to_string(),
        };
        assert_eq!(b.message, "oops");
    }

    #[test]
    fn notification_store_evicts_oldest_at_capacity() {
        let mut store = NotificationStore::default();
        // Fill to capacity
        for i in 0..MAX_NOTIFICATIONS {
            store.push(None, format!("msg-{i}"), NotificationKind::Info);
        }
        assert_eq!(store.items.len(), MAX_NOTIFICATIONS);

        // One more should evict the oldest
        store.push(None, "overflow".to_string(), NotificationKind::Info);
        assert_eq!(store.items.len(), MAX_NOTIFICATIONS);
        // First item should now be "msg-1" (msg-0 was evicted)
        assert_eq!(store.items[0].message, "msg-1");
        // Last item is the new one
        assert_eq!(store.items[MAX_NOTIFICATIONS - 1].message, "overflow");
    }

    #[test]
    fn notification_store_preserves_unread_count_after_eviction() {
        let mut store = NotificationStore::default();
        for i in 0..MAX_NOTIFICATIONS {
            store.push(None, format!("msg-{i}"), NotificationKind::Info);
        }
        // Mark all read, then add one unread
        store.mark_all_read();
        store.push(None, "new".to_string(), NotificationKind::Info);
        assert_eq!(store.unread_count(), 1);
    }
}
