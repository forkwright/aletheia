#![expect(clippy::expect_used, reason = "test assertions")]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use oikonomos::maintenance::{maintenance_task_by_id, manual_maintenance_task_ids};
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

use super::build_config;

static CWD_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

// WHY: the cwd-resolution test changes process cwd; restore it on drop so
// that change cannot leak into later tests.
struct CwdGuard(PathBuf);

impl CwdGuard {
    fn save() -> Self {
        Self(std::env::current_dir().expect("current dir"))
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("restore cwd");
    }
}

#[test]
fn all_expansion_comes_from_registry_manual_tasks() {
    let ids = manual_maintenance_task_ids();
    assert!(!ids.is_empty(), "manual task registry must not be empty");

    let unique: BTreeSet<_> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "manual task IDs must be unique");

    for id in ids {
        let Some(definition) = maintenance_task_by_id(id) else {
            panic!("manual id '{id}' resolves");
        };
        assert!(
            definition.manual_run().is_some(),
            "manual task '{id}' must carry a manual run handler"
        );
    }
}

#[test]
fn manual_registry_exposes_instance_backup_not_fjall_backup() {
    let ids = manual_maintenance_task_ids();
    assert!(
        ids.contains(&"instance-backup"),
        "manual registry must expose instance-backup"
    );
    assert!(
        !ids.contains(&"fjall-backup"),
        "manual registry must not expose fjall-backup"
    );

    let Some(definition) = maintenance_task_by_id("instance-backup") else {
        panic!("instance-backup must resolve");
    };
    assert!(
        definition.manual_run().is_some(),
        "instance-backup must be runnable manually"
    );

    let Some(legacy) = maintenance_task_by_id("fjall-backup") else {
        panic!("fjall-backup legacy alias must still resolve");
    };
    assert_eq!(
        legacy.id(),
        "instance-backup",
        "legacy alias must point to instance-backup"
    );
}

#[test]
fn build_config_resolves_example_root_sibling_even_from_unrelated_cwd() {
    let _cwd_lock = CWD_LOCK.lock().expect("lock cwd mutation");
    let _guard = CwdGuard::save();
    let tmp = tempfile::tempdir().expect("tempdir");
    let instance_root = tmp.path().join("instance");
    let sibling_example = tmp.path().join("instance.example");
    std::fs::create_dir_all(&instance_root).expect("mkdir instance");
    std::fs::create_dir_all(&sibling_example).expect("mkdir sibling example");

    let unrelated = tmp.path().join("unrelated");
    std::fs::create_dir_all(&unrelated).expect("mkdir unrelated");
    std::env::set_current_dir(&unrelated).expect("set cwd");

    let oikos = Oikos::from_root(&instance_root);
    let config = AletheiaConfig::default();
    let maint = build_config(&oikos, &config.maintenance, &config.prompt_audit);

    assert_eq!(
        maint.drift_detection.example_root, sibling_example,
        "drift template should resolve to sibling instance.example, not cwd"
    );
}
