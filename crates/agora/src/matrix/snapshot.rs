//! Serializable snapshot of the Matrix `CryptoStore` in-memory state.
//!
//! Phase 2 persistence strategy: we delegate `CryptoStore` trait operations
//! to an in-process [`MemoryStore`] and mirror the externally-observable
//! state into fjall as a single rmp-serde blob on every mutation.
//!
//! This module owns only the [`Snapshot`] type and its capture / restore
//! routines. The `CryptoStore` trait impl and fjall plumbing live in
//! [`super::crypto_store`]. Kept separate to respect the 800-line standard
//! cap and to make the serialised shape easy to evolve independently of
//! the trait delegation.
//!
//! Phase 3 replaces this with typed per-record partitions (session,
//! inbound_group_session, device, identity, tracked_user,
//! outbound_group_session); the Snapshot::capture/restore entry points
//! stay valid because they round-trip the same MemoryStore.

use std::collections::HashMap;

use matrix_sdk_base::ruma;
use matrix_sdk_base::ruma::UserId;
use matrix_sdk_crypto::DeviceData;
use matrix_sdk_crypto::store::types::Changes;
use matrix_sdk_crypto::store::{CryptoStore as _, MemoryStore};
use tracing::warn;

/// Serialisable view of the subset of `CryptoStore` state we mirror to fjall.
///
/// - `devices`: per-device `serde_json` blobs keyed by `{user_id}:{device_id}`.
/// - `tracked_users`: `user_id` → "dirty" (needs /keys/query refresh).
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub(super) struct Snapshot {
    /// Serialized (via `serde_json`) `DeviceData` per user/device. Keyed by
    /// `{user_id}:{device_id}` for deterministic fjall ordering.
    pub devices: HashMap<String, String>,
    /// Tracked users: whether the set is dirty (needs a `/keys/query` refresh).
    pub tracked_users: HashMap<String, bool>,
}

impl Snapshot {
    /// Build a snapshot from the live [`MemoryStore`] state.
    ///
    /// WHY we iterate tracked-users first: `MemoryStore` does not expose a
    /// bulk-device iterator. Per-user fan-out is cheap for the single-agent
    /// low-device-count deployments targeted by Phase 2.
    pub async fn capture(inner: &MemoryStore) -> Self {
        let tracked = inner.load_tracked_users().await.unwrap_or_default();
        let mut tracked_users = HashMap::with_capacity(tracked.len());
        let mut devices = HashMap::new();

        for user in &tracked {
            tracked_users.insert(user.user_id.to_string(), user.dirty);
            let devs = match inner.get_user_devices(&user.user_id).await {
                Ok(d) => d,
                Err(e) => {
                    warn!(
                        error = ?e,
                        user = %user.user_id,
                        "get_user_devices failed during snapshot capture"
                    );
                    continue;
                }
            };
            for (device_id, device) in devs {
                let key = format!("{}:{}", user.user_id, device_id);
                match serde_json::to_string(&device) {
                    Ok(encoded) => {
                        devices.insert(key, encoded);
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            user = %user.user_id,
                            device = %device_id,
                            "device serialise failed during snapshot capture"
                        );
                    }
                }
            }
        }

        Self {
            devices,
            tracked_users,
        }
    }

    /// Replay a previously-captured snapshot into a fresh [`MemoryStore`].
    ///
    /// Order matters: tracked users go in first so subsequent device writes
    /// index against the correct user set.
    pub async fn restore_into(self, inner: &MemoryStore) {
        let owned: Vec<(ruma::OwnedUserId, bool)> = self
            .tracked_users
            .iter()
            .filter_map(|(u, &dirty)| ruma::UserId::parse(u).ok().map(|uid| (uid, dirty)))
            .collect();
        let borrowed: Vec<(&UserId, bool)> = owned.iter().map(|(u, d)| (u.as_ref(), *d)).collect();
        if let Err(e) = inner.save_tracked_users(&borrowed).await {
            warn!(error = ?e, "save_tracked_users failed during snapshot restore");
        }

        for (_key, encoded) in self.devices {
            match serde_json::from_str::<DeviceData>(&encoded) {
                Ok(device) => {
                    // MemoryStore exposes no public bulk-insert for devices;
                    // the only public path is `save_changes`. Build a minimal
                    // `Changes` carrying just this device and replay it.
                    let mut changes = Changes::default();
                    changes.devices.new.push(device);
                    if let Err(e) = inner.save_changes(changes).await {
                        warn!(
                            error = ?e,
                            "save_changes failed during snapshot restore"
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "device decode failed during snapshot restore, skipping"
                    );
                }
            }
        }
    }
}
