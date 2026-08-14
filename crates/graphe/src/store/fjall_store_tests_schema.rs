#![expect(clippy::expect_used, reason = "test assertions")]

use tempfile::TempDir;

use super::super::{CURRENT_SCHEMA_VERSION, SCHEMA_MANIFEST_FILE, SchemaManifest, SessionStore};

fn manifest_path(store_path: &std::path::Path) -> std::path::PathBuf {
    store_path.join(SCHEMA_MANIFEST_FILE)
}

fn read_manifest(store_path: &std::path::Path) -> SchemaManifest {
    let bytes = std::fs::read(manifest_path(store_path)).expect("manifest file exists");
    serde_json::from_slice(&bytes).expect("manifest is valid JSON")
}

fn write_manifest_with_version(store_path: &std::path::Path, schema_version: u32) {
    let mut manifest = read_manifest(store_path);
    manifest.schema_version = schema_version;
    let data = serde_json::to_vec_pretty(&manifest).expect("manifest serializes");
    std::fs::write(manifest_path(store_path), data).expect("manifest overwrites");
}

#[test]
fn fresh_store_writes_schema_manifest() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");

    SessionStore::open(&path).expect("fresh store opens");

    let manifest = read_manifest(&path);
    assert_eq!(manifest.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(manifest.store_kind, "graphe-session-store");
    assert_eq!(
        manifest.created_at, manifest.updated_at,
        "a brand-new manifest's created_at and updated_at should match"
    );
}

#[test]
fn compatible_reopen_succeeds_and_preserves_created_at() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");

    {
        let store = SessionStore::open(&path).expect("first open");
        store
            .create_session("ses-1", "syn", "main", None, None)
            .expect("create session");
    }
    let first_manifest = read_manifest(&path);

    {
        let store = SessionStore::open(&path).expect("reopen");
        let found = store
            .find_session_by_id("ses-1")
            .expect("query succeeds")
            .expect("session survived reopen");
        assert_eq!(found.id, "ses-1");
    }
    let second_manifest = read_manifest(&path);

    assert_eq!(
        second_manifest.created_at, first_manifest.created_at,
        "reopen must not overwrite the original created_at"
    );
    assert_eq!(second_manifest.schema_version, CURRENT_SCHEMA_VERSION);
}

#[test]
fn missing_manifest_on_existing_store_refuses_and_preserves_data() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");

    {
        let store = SessionStore::open(&path).expect("first open");
        store
            .create_session("ses-legacy", "syn", "main", None, None)
            .expect("create session");
    }

    // WHY: simulate a store written before the schema-manifest feature
    // existed by deleting the manifest file only — the fjall data beneath it
    // is untouched.
    std::fs::remove_file(manifest_path(&path)).expect("manifest removed");

    let err = SessionStore::open(&path).expect_err("missing manifest on non-empty store refuses");
    let msg = err.to_string();
    assert!(
        msg.contains("no schema manifest"),
        "error should name the missing-manifest condition, got: {msg}"
    );
    assert!(
        msg.contains("aletheia session-store stamp"),
        "refusal should name the operator-invocable CLI command, not just the Rust \
         function, got: {msg}"
    );

    // The refusal must happen entirely via a plain filesystem read — fjall's
    // own keyspace open (and whatever internal recovery it performs) must
    // never have run. Prove the session data survived intact by stamping the
    // store as legacy-compatible (an explicit, human-attested step) and
    // reopening normally.
    SessionStore::stamp_legacy_schema_manifest(&path).expect("legacy stamp succeeds");
    let store = SessionStore::open(&path).expect("reopen after stamping succeeds");
    let restored = store
        .find_session_by_id("ses-legacy")
        .expect("query succeeds")
        .expect("session data survived the refusal untouched");
    assert_eq!(restored.id, "ses-legacy");
}

#[test]
fn older_schema_version_refuses_to_open() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");
    SessionStore::open(&path).expect("fresh store opens");
    write_manifest_with_version(&path, 0);

    let err = SessionStore::open(&path).expect_err("older schema version refuses");
    let msg = err.to_string();
    assert!(
        msg.contains("requires") && msg.contains('0'),
        "error should name both versions, got: {msg}"
    );
}

#[test]
fn newer_schema_version_refuses_to_open() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");
    SessionStore::open(&path).expect("fresh store opens");
    write_manifest_with_version(&path, 9999);

    let err = SessionStore::open(&path).expect_err("newer schema version refuses");
    let msg = err.to_string();
    assert!(
        msg.contains("only understands up to") && msg.contains("9999"),
        "error should name both versions, got: {msg}"
    );
}

#[test]
fn corrupt_manifest_refuses_to_open() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");
    SessionStore::open(&path).expect("fresh store opens");
    std::fs::write(manifest_path(&path), b"not json").expect("manifest overwritten with garbage");

    let err = SessionStore::open(&path).expect_err("corrupt manifest refuses");
    assert!(err.to_string().contains("is corrupt"), "got: {err}");
}

#[test]
fn verify_schema_manifest_is_read_only() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");
    SessionStore::open(&path).expect("fresh store opens");
    let before = read_manifest(&path);

    let verified = SessionStore::verify_schema_manifest(&path).expect("verify succeeds");
    assert_eq!(verified.schema_version, CURRENT_SCHEMA_VERSION);

    let after = read_manifest(&path);
    assert_eq!(
        before.updated_at, after.updated_at,
        "verify must not write anything"
    );
}

#[test]
fn stamp_legacy_schema_manifest_refuses_when_already_stamped() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");
    SessionStore::open(&path).expect("fresh store opens");

    let err = SessionStore::stamp_legacy_schema_manifest(&path)
        .expect_err("stamping an already-stamped store refuses");
    let msg = err.to_string();
    assert!(msg.contains("already has a schema manifest"), "got: {msg}");
    assert!(
        msg.contains("aletheia session-store verify"),
        "refusal should point at the CLI command an operator can run to inspect the \
         existing manifest, got: {msg}"
    );
}

#[test]
fn stamp_legacy_schema_manifest_refuses_on_fresh_path() {
    let dir = TempDir::new().expect("temp dir creates");
    let path = dir.path().join("sessions");

    let err = SessionStore::stamp_legacy_schema_manifest(&path)
        .expect_err("stamping a path with no existing store data refuses");
    assert!(
        err.to_string().contains("no existing store data"),
        "got: {err}"
    );
}
