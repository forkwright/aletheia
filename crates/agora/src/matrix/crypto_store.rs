//! Custom `CryptoStore` backed by fjall.
//!
//! ## Why fjall (not the matrix-sdk default `SqliteCryptoStore`)
//!
//! The rest of Aletheia's storage is fjall (LSM, pure-Rust, zero C deps).
//! Adopting `SqliteCryptoStore` would re-introduce rusqlite for a single
//! subsystem. Thumos's `design-harmostes.md` and issue #3557 both require
//! "zero rusqlite in aletheia dep tree" as an acceptance criterion, so we
//! implement `CryptoStore` against fjall directly.
//!
//! ## Structure
//!
//! For Phase 2 we delegate all operations to an in-process `MemoryStore` and
//! persist **full snapshots** of the serializable crypto state to fjall on
//! every mutating call (`save_changes`, `save_pending_changes`, and the
//! targeted `save_inbound_group_sessions` path). Snapshots are codec'd with
//! `rmp-serde` for compactness.
//!
//! This is deliberately the simplest correct persistence scheme: it trades
//! per-operation bandwidth for code surface area, which we want to keep
//! bounded while Phase 3 wires up the sync loop. A future revision can split
//! the snapshot into per-record partitions (session, inbound_group_session,
//! device, identity, tracked_user, outbound_group_session) once the access
//! patterns have hardened.
//!
//! ## Keyspace layout
//!
//! ```text
//! crypto/{agent_id}/snapshot   -> rmp-serde blob (full MemoryStore dump)
//! ```
//!
//! `{agent_id}` isolates per-agent crypto state so a single conduwuit deploy
//! can host independent accounts without keyspace collisions.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

// WHY: matrix-sdk-crypto's `CryptoStore` trait is defined with #[async_trait]
// because it is dyn-safe + Send/Sync-bound across !wasm and wasm targets. We
// must use the same macro to implement the trait on our Fjall-backed store.
// Kanon normally bans `async_trait` in favour of native async traits, but
// honouring the external trait signature overrides that policy here.
use async_trait::async_trait; // kanon:ignore RUST/banned-crate:async_trait -- required by matrix_sdk_crypto::store::CryptoStore trait signature
use fjall::{KeyspaceCreateOptions, Readable as _, SingleWriterTxDatabase};
use matrix_sdk_base::cross_process_lock::CrossProcessLockGeneration;
use matrix_sdk_base::ruma::{
    DeviceId, OwnedDeviceId, RoomId, TransactionId, UserId, events::secret::request::SecretName,
};
use matrix_sdk_crypto::olm::OlmMessageHash;
use matrix_sdk_crypto::store::types::{
    BackupKeys, Changes, DehydratedDeviceKey, PendingChanges, RoomKeyCounts, RoomKeyWithheldEntry,
    RoomSettings, StoredRoomKeyBundleData, TrackedUser,
};
use matrix_sdk_crypto::store::{CryptoStore, CryptoStoreError, MemoryStore};
use matrix_sdk_crypto::{
    Account, DeviceData, GossipRequest, GossippedSecret, SecretInfo, UserIdentityData,
    olm::{
        InboundGroupSession, OutboundGroupSession, PrivateCrossSigningIdentity, SenderDataType,
        Session,
    },
    vodozemac::Curve25519PublicKey,
};
use tokio::sync::Mutex;
use tracing::debug;

use super::error::{CodecSnafu, Error, Result, StoreSnafu};

/// fjall keyspace (partition) holding the crypto snapshot blob.
const SNAPSHOT_PARTITION: &str = "crypto_snapshot";

/// Key inside `SNAPSHOT_PARTITION` at which the snapshot blob lives.
const SNAPSHOT_KEY: &str = "snapshot";

/// fjall-backed custom `CryptoStore`.
///
/// The in-memory `MemoryStore` is the authoritative cache for reads and
/// writes; fjall is the durable mirror, refreshed on every mutating
/// operation. A dedicated persist `Mutex` serialises snapshot writes so
/// concurrent `save_changes` calls do not race the fjall txn.
pub struct FjallCryptoStore {
    /// Authoritative in-memory store. All `CryptoStore` trait methods delegate here.
    inner: MemoryStore,
    /// fjall database handle (shared with the write-lock guard).
    db: Arc<SingleWriterTxDatabase>,
    /// Koina-style write-serialisation lock around fjall writes.
    ///
    /// WHY `std::sync::Mutex`: matches the convention in `koina::fjall::FjallDb`.
    /// Held only across synchronous fjall transactions, never across `.await`.
    fjall_write_lock: std::sync::Mutex<()>,
    /// Temp dir guard for ephemeral stores (test mode). `None` for persistent stores.
    ///
    /// Read only via `Debug` (to report persistent vs ephemeral); kept alive
    /// primarily for its `Drop` side-effect which deletes the tempdir.
    temp_dir_guard: Option<tempfile::TempDir>,
    /// Serialises async persist calls so repeated `save_changes` does not
    /// interleave their snapshot dumps (which would otherwise briefly write
    /// stale state over fresh state).
    persist_lock: Mutex<()>,
}

impl std::fmt::Debug for FjallCryptoStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FjallCryptoStore")
            .field("persistent", &self.temp_dir_guard.is_none())
            .finish_non_exhaustive()
    }
}

impl FjallCryptoStore {
    /// Open (or create) a persistent crypto store for `agent_id` at `path`.
    ///
    /// The fjall keyspace is created under `path/{agent_id}`; callers should
    /// pass a stable per-agent path (typically `oikos.data().join("matrix-crypto")`).
    ///
    /// Async because a snapshot restore replays saved `Changes` back through
    /// `MemoryStore::save_changes`, which is an async trait method.
    pub async fn open(path: &Path, agent_id: &str) -> Result<Self> {
        let agent_path = path.join(agent_id);
        let fdb = koina::fjall::FjallDb::open(&agent_path, &[SNAPSHOT_PARTITION]).map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        let store = Self {
            inner: MemoryStore::new(),
            db: Arc::new(fdb.db),
            fjall_write_lock: std::sync::Mutex::new(()),
            temp_dir_guard: fdb._temp_dir,
            persist_lock: Mutex::new(()),
        };
        store.load_snapshot_from_fjall().await?;
        Ok(store)
    }

    /// Open an ephemeral in-memory store whose fjall backing lives in a
    /// tempdir that is deleted on drop. Intended for tests.
    pub fn open_temp(agent_id: &str) -> Result<Self> {
        let dir = tempfile::TempDir::new().map_err(|source| {
            StoreSnafu {
                message: format!("temp dir: {source}"),
            }
            .build()
        })?;
        let agent_path = dir.path().join(agent_id);
        let fdb = koina::fjall::FjallDb::open(&agent_path, &[SNAPSHOT_PARTITION]).map_err(|e| {
            StoreSnafu {
                message: e.to_string(),
            }
            .build()
        })?;
        Ok(Self {
            inner: MemoryStore::new(),
            db: Arc::new(fdb.db),
            fjall_write_lock: std::sync::Mutex::new(()),
            temp_dir_guard: Some(dir),
            persist_lock: Mutex::new(()),
        })
    }

    fn partition(&self, name: &str) -> Result<fjall::SingleWriterTxKeyspace> {
        self.db
            .keyspace(name, KeyspaceCreateOptions::default)
            .map_err(|e| {
                StoreSnafu {
                    message: format!("keyspace {name}: {e}"),
                }
                .build()
            })
    }

    /// Pull the durable blob (if any) into the in-memory store. Called once
    /// during `open`; a failure here is fatal — callers need fresh state.
    async fn load_snapshot_from_fjall(&self) -> Result<()> {
        let partition = self.partition(SNAPSHOT_PARTITION)?;
        let snap = self.db.read_tx();
        let raw = snap.get(&partition, SNAPSHOT_KEY.as_bytes()).map_err(|e| {
            StoreSnafu {
                message: format!("read snapshot: {e}"),
            }
            .build()
        })?;
        let Some(bytes) = raw else {
            debug!("no crypto snapshot on disk yet");
            return Ok(());
        };
        let snapshot: Snapshot = rmp_serde::from_slice(&bytes).map_err(|e| {
            CodecSnafu {
                message: format!("decode snapshot: {e}"),
            }
            .build()
        })?;
        let device_count = snapshot.devices.len();
        let tracked_count = snapshot.tracked_users.len();
        snapshot.restore_into(&self.inner).await;
        debug!(
            devices = device_count,
            tracked_users = tracked_count,
            "loaded crypto snapshot"
        );
        Ok(())
    }

    /// Flush the full in-memory snapshot to fjall.
    ///
    /// Errors map to `CryptoStoreError::Backend` at the trait boundary.
    async fn persist(&self) -> Result<()> {
        let _persist_guard = self.persist_lock.lock().await;
        let snapshot = Snapshot::capture(&self.inner).await;
        let bytes = rmp_serde::to_vec(&snapshot).map_err(|e| {
            CodecSnafu {
                message: format!("encode: {e}"),
            }
            .build()
        })?;
        let partition = self.partition(SNAPSHOT_PARTITION)?;
        let _write_guard = self
            .fjall_write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut tx = self.db.write_tx();
        tx.insert(&partition, SNAPSHOT_KEY.as_bytes(), &bytes);
        tx.commit().map_err(|e| {
            StoreSnafu {
                message: format!("commit snapshot: {e}"),
            }
            .build()
        })?;
        Ok(())
    }
}

impl From<Error> for CryptoStoreError {
    fn from(e: Error) -> Self {
        CryptoStoreError::backend(e)
    }
}

// Snapshot type + capture/restore live in `snapshot.rs`.
use super::snapshot::Snapshot;

/// Convert a debug-formatted delegate error into a `StoreSnafu` Error.
///
/// WHY: every `CryptoStore` delegate method emits the same `.map_err` shape.
/// Consolidating into this helper shaves ~200 lines and makes the trait
/// impl block easy to eyeball for "are we persisting after this?" decisions.
fn store_err<E: std::fmt::Debug>(label: &'static str, e: E) -> Error {
    StoreSnafu {
        message: format!("{label}: {e:?}"),
    }
    .build()
}

// ── CryptoStore impl ────────────────────────────────────────────────────────
//
// The impl delegates every call to `self.inner` (MemoryStore). Mutating paths
// additionally `persist()` after the delegate succeeds, converting fjall
// errors into `CryptoStoreError::Backend`.
//
// Unused parameter reasons:
// - `_` naming inherited from the trait signature; each method preserves the
//   caller's parameter for future Phase 3 specialisation against typed
//   partitions.

#[async_trait]
impl CryptoStore for FjallCryptoStore {
    type Error = Error;

    async fn load_account(&self) -> Result<Option<Account>> {
        Ok(self.inner.load_account().await.unwrap_or(None))
    }

    async fn load_identity(&self) -> Result<Option<PrivateCrossSigningIdentity>> {
        Ok(self.inner.load_identity().await.unwrap_or(None))
    }

    async fn save_changes(&self, changes: Changes) -> Result<()> {
        self.inner
            .save_changes(changes)
            .await
            .map_err(|e| store_err("memory save_changes", e))?;
        self.persist().await
    }

    async fn save_pending_changes(&self, changes: PendingChanges) -> Result<()> {
        self.inner
            .save_pending_changes(changes)
            .await
            .map_err(|e| store_err("memory save_pending", e))?;
        self.persist().await
    }

    async fn save_inbound_group_sessions(
        &self,
        sessions: Vec<InboundGroupSession>,
        backed_up_to_version: Option<&str>,
    ) -> Result<()> {
        self.inner
            .save_inbound_group_sessions(sessions, backed_up_to_version)
            .await
            .map_err(|e| store_err("memory save_igs", e))?;
        self.persist().await
    }

    async fn get_sessions(&self, sender_key: &str) -> Result<Option<Vec<Session>>> {
        self.inner
            .get_sessions(sender_key)
            .await
            .map_err(|e| store_err("get_sessions", e))
    }

    async fn get_inbound_group_session(
        &self,
        room_id: &RoomId,
        session_id: &str,
    ) -> Result<Option<InboundGroupSession>> {
        self.inner
            .get_inbound_group_session(room_id, session_id)
            .await
            .map_err(|e| store_err("get_igs", e))
    }

    async fn get_withheld_info(
        &self,
        room_id: &RoomId,
        session_id: &str,
    ) -> Result<Option<RoomKeyWithheldEntry>> {
        self.inner
            .get_withheld_info(room_id, session_id)
            .await
            .map_err(|e| store_err("get_withheld_info", e))
    }

    async fn get_withheld_sessions_by_room_id(
        &self,
        room_id: &RoomId,
    ) -> Result<Vec<RoomKeyWithheldEntry>> {
        self.inner
            .get_withheld_sessions_by_room_id(room_id)
            .await
            .map_err(|e| store_err("get_withheld_by_room", e))
    }

    async fn get_inbound_group_sessions(&self) -> Result<Vec<InboundGroupSession>> {
        self.inner
            .get_inbound_group_sessions()
            .await
            .map_err(|e| store_err("get_igs_all", e))
    }

    async fn inbound_group_session_counts(
        &self,
        backup_version: Option<&str>,
    ) -> Result<RoomKeyCounts> {
        self.inner
            .inbound_group_session_counts(backup_version)
            .await
            .map_err(|e| store_err("igs_counts", e))
    }

    async fn get_inbound_group_sessions_by_room_id(
        &self,
        room_id: &RoomId,
    ) -> Result<Vec<InboundGroupSession>> {
        self.inner
            .get_inbound_group_sessions_by_room_id(room_id)
            .await
            .map_err(|e| store_err("igs_by_room", e))
    }

    async fn get_inbound_group_sessions_for_device_batch(
        &self,
        curve_key: Curve25519PublicKey,
        sender_data_type: SenderDataType,
        after_session_id: Option<String>,
        limit: usize,
    ) -> Result<Vec<InboundGroupSession>> {
        self.inner
            .get_inbound_group_sessions_for_device_batch(
                curve_key,
                sender_data_type,
                after_session_id,
                limit,
            )
            .await
            .map_err(|e| store_err("igs_for_device", e))
    }

    async fn inbound_group_sessions_for_backup(
        &self,
        backup_version: &str,
        limit: usize,
    ) -> Result<Vec<InboundGroupSession>> {
        self.inner
            .inbound_group_sessions_for_backup(backup_version, limit)
            .await
            .map_err(|e| store_err("igs_for_backup", e))
    }

    async fn mark_inbound_group_sessions_as_backed_up(
        &self,
        backup_version: &str,
        room_and_session_ids: &[(&RoomId, &str)],
    ) -> Result<()> {
        self.inner
            .mark_inbound_group_sessions_as_backed_up(backup_version, room_and_session_ids)
            .await
            .map_err(|e| store_err("mark_igs_backed_up", e))?;
        self.persist().await
    }

    async fn reset_backup_state(&self) -> Result<()> {
        self.inner
            .reset_backup_state()
            .await
            .map_err(|e| store_err("reset_backup", e))?;
        self.persist().await
    }

    async fn load_backup_keys(&self) -> Result<BackupKeys> {
        self.inner
            .load_backup_keys()
            .await
            .map_err(|e| store_err("load_backup_keys", e))
    }

    async fn load_dehydrated_device_pickle_key(&self) -> Result<Option<DehydratedDeviceKey>> {
        self.inner
            .load_dehydrated_device_pickle_key()
            .await
            .map_err(|e| store_err("load_dehydrated", e))
    }

    async fn delete_dehydrated_device_pickle_key(&self) -> Result<()> {
        self.inner
            .delete_dehydrated_device_pickle_key()
            .await
            .map_err(|e| store_err("delete_dehydrated", e))?;
        self.persist().await
    }

    async fn get_outbound_group_session(
        &self,
        room_id: &RoomId,
    ) -> Result<Option<OutboundGroupSession>> {
        self.inner
            .get_outbound_group_session(room_id)
            .await
            .map_err(|e| store_err("get_ogs", e))
    }

    async fn load_tracked_users(&self) -> Result<Vec<TrackedUser>> {
        self.inner
            .load_tracked_users()
            .await
            .map_err(|e| store_err("load_tracked", e))
    }

    async fn save_tracked_users(&self, users: &[(&UserId, bool)]) -> Result<()> {
        self.inner
            .save_tracked_users(users)
            .await
            .map_err(|e| store_err("save_tracked", e))?;
        self.persist().await
    }

    async fn get_device(
        &self,
        user_id: &UserId,
        device_id: &DeviceId,
    ) -> Result<Option<DeviceData>> {
        self.inner
            .get_device(user_id, device_id)
            .await
            .map_err(|e| store_err("get_device", e))
    }

    async fn get_user_devices(
        &self,
        user_id: &UserId,
    ) -> Result<HashMap<OwnedDeviceId, DeviceData>> {
        self.inner
            .get_user_devices(user_id)
            .await
            .map_err(|e| store_err("get_user_devices", e))
    }

    async fn get_own_device(&self) -> Result<DeviceData> {
        self.inner
            .get_own_device()
            .await
            .map_err(|e| store_err("get_own_device", e))
    }

    async fn get_user_identity(&self, user_id: &UserId) -> Result<Option<UserIdentityData>> {
        self.inner
            .get_user_identity(user_id)
            .await
            .map_err(|e| store_err("get_user_identity", e))
    }

    async fn is_message_known(&self, message_hash: &OlmMessageHash) -> Result<bool> {
        self.inner
            .is_message_known(message_hash)
            .await
            .map_err(|e| store_err("is_message_known", e))
    }

    async fn get_outgoing_secret_requests(
        &self,
        request_id: &TransactionId,
    ) -> Result<Option<GossipRequest>> {
        self.inner
            .get_outgoing_secret_requests(request_id)
            .await
            .map_err(|e| store_err("get_outgoing_secret", e))
    }

    async fn get_secret_request_by_info(
        &self,
        secret_info: &SecretInfo,
    ) -> Result<Option<GossipRequest>> {
        self.inner
            .get_secret_request_by_info(secret_info)
            .await
            .map_err(|e| store_err("get_secret_by_info", e))
    }

    async fn get_unsent_secret_requests(&self) -> Result<Vec<GossipRequest>> {
        self.inner
            .get_unsent_secret_requests()
            .await
            .map_err(|e| store_err("get_unsent_secrets", e))
    }

    async fn delete_outgoing_secret_requests(&self, request_id: &TransactionId) -> Result<()> {
        self.inner
            .delete_outgoing_secret_requests(request_id)
            .await
            .map_err(|e| store_err("delete_outgoing_secret", e))?;
        self.persist().await
    }

    async fn get_secrets_from_inbox(
        &self,
        secret_name: &SecretName,
    ) -> Result<Vec<GossippedSecret>> {
        self.inner
            .get_secrets_from_inbox(secret_name)
            .await
            .map_err(|e| store_err("get_inbox_secrets", e))
    }

    async fn delete_secrets_from_inbox(&self, secret_name: &SecretName) -> Result<()> {
        self.inner
            .delete_secrets_from_inbox(secret_name)
            .await
            .map_err(|e| store_err("delete_inbox_secrets", e))?;
        self.persist().await
    }

    async fn get_room_settings(&self, room_id: &RoomId) -> Result<Option<RoomSettings>> {
        self.inner
            .get_room_settings(room_id)
            .await
            .map_err(|e| store_err("get_room_settings", e))
    }

    async fn get_received_room_key_bundle_data(
        &self,
        room_id: &RoomId,
        user_id: &UserId,
    ) -> Result<Option<StoredRoomKeyBundleData>> {
        self.inner
            .get_received_room_key_bundle_data(room_id, user_id)
            .await
            .map_err(|e| store_err("get_room_key_bundle", e))
    }

    async fn get_custom_value(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.inner
            .get_custom_value(key)
            .await
            .map_err(|e| store_err("get_custom", e))
    }

    async fn set_custom_value(&self, key: &str, value: Vec<u8>) -> Result<()> {
        self.inner
            .set_custom_value(key, value)
            .await
            .map_err(|e| store_err("set_custom", e))?;
        self.persist().await
    }

    async fn remove_custom_value(&self, key: &str) -> Result<()> {
        self.inner
            .remove_custom_value(key)
            .await
            .map_err(|e| store_err("remove_custom", e))?;
        self.persist().await
    }

    async fn try_take_leased_lock(
        &self,
        lease_duration_ms: u32,
        key: &str,
        holder: &str,
    ) -> Result<Option<CrossProcessLockGeneration>> {
        self.inner
            .try_take_leased_lock(lease_duration_ms, key, holder)
            .await
            .map_err(|e| store_err("try_take_leased_lock", e))
    }

    async fn next_batch_token(&self) -> Result<Option<String>> {
        self.inner
            .next_batch_token()
            .await
            .map_err(|e| store_err("next_batch_token", e))
    }

    async fn get_size(&self) -> Result<Option<usize>> {
        self.inner
            .get_size()
            .await
            .map_err(|e| store_err("get_size", e))
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use matrix_sdk_base::ruma::{device_id, user_id};

    use super::*;

    fn make_device() -> DeviceData {
        // Canonical path: freshly created Account -> DeviceData. This is what
        // matrix-sdk-crypto's own tests do.
        let user = user_id!("@alice:example.org");
        let device = device_id!("ABCDEFGHIJ");
        let account = Account::with_device_id(user, device);
        DeviceData::from_account(&account)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn device_roundtrip() {
        let store = FjallCryptoStore::open_temp("alice").expect("open temp");

        let device = make_device();
        let user_id_owned = device.user_id().to_owned();
        let device_id_owned = device.device_id().to_owned();

        let mut changes = Changes::default();
        changes.devices.new.push(device.clone());
        store.save_changes(changes).await.expect("save_changes");

        let loaded = store
            .get_device(&user_id_owned, &device_id_owned)
            .await
            .expect("get_device")
            .expect("device present");
        assert_eq!(loaded.user_id(), device.user_id());
        assert_eq!(loaded.device_id(), device.device_id());
    }

    #[test]
    fn open_temp_creates_and_drops() {
        let store = FjallCryptoStore::open_temp("alice").expect("open temp");
        // Dropping the store should remove the tempdir without panicking.
        drop(store);
    }

    #[tokio::test]
    async fn tracked_user_roundtrip() {
        let store = FjallCryptoStore::open_temp("alice").expect("open temp");
        let uid = user_id!("@alice:example.org");
        store
            .save_tracked_users(&[(uid, true)])
            .await
            .expect("save tracked");
        let tracked = store.load_tracked_users().await.expect("load tracked");
        assert_eq!(tracked.len(), 1);
        let entry = tracked.first().expect("entry present");
        assert_eq!(entry.user_id, uid);
        assert!(entry.dirty);
    }

    // Compile-time check that the device_id!/user_id! macros from
    // matrix_sdk_base::ruma remain accessible on future SDK version
    // bumps — if they are moved or renamed, this test fails to build.
    #[test]
    fn reexport_smoke() {
        let _d = device_id!("ABCDEFGHIJ");
        let _u = user_id!("@alice:example.org");
    }
}
