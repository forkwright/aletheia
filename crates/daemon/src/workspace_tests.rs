#![expect(clippy::unwrap_used, reason = "test assertions")]

use super::*;

#[test]
fn materialize_writes_template_when_absent() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("nous").join("alice");
    std::fs::create_dir_all(&workspace).unwrap();

    let outcome = materialize_prosoche_md(&workspace).unwrap();

    assert_eq!(
        outcome,
        ProsocheMaterialization::Materialized,
        "absent file must be materialized"
    );
    let written = std::fs::read_to_string(workspace.join("PROSOCHE.md")).unwrap();
    assert_eq!(
        written, PROSOCHE_TEMPLATE,
        "materialized content must be the embedded template"
    );
    // WHY: the daemon heartbeat prompt runs "per PROSOCHE.md"; the template's
    // bounded-heartbeat contract (also asserted for the on-disk templates in
    // aletheia/src/init/scaffold.rs) is what keeps the tick cheap.
    assert!(
        written.contains("60 seconds"),
        "template must state the heartbeat time budget"
    );
    assert!(
        written.contains("5 tool calls"),
        "template must state the heartbeat tool-call budget"
    );
}

#[test]
fn materialize_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path();

    let first = materialize_prosoche_md(workspace).unwrap();
    let second = materialize_prosoche_md(workspace).unwrap();

    assert_eq!(first, ProsocheMaterialization::Materialized);
    assert_eq!(
        second,
        ProsocheMaterialization::AlreadyPresent,
        "second call must not rewrite the file"
    );
}

#[test]
fn materialize_preserves_operator_edits() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path();
    let edited = "# PROSOCHE\n\nOperator-customized checklist.\n";
    #[expect(
        clippy::disallowed_methods,
        reason = "test setup runs outside the async runtime and requires synchronous filesystem access"
    )]
    std::fs::write(workspace.join("PROSOCHE.md"), edited).unwrap();

    let outcome = materialize_prosoche_md(workspace).unwrap();

    assert_eq!(
        outcome,
        ProsocheMaterialization::AlreadyPresent,
        "existing file must be reported as already present"
    );
    let on_disk = std::fs::read_to_string(workspace.join("PROSOCHE.md")).unwrap();
    assert_eq!(
        on_disk, edited,
        "operator-edited PROSOCHE.md must be left untouched"
    );
}

#[test]
fn materialize_errors_when_workspace_dir_missing() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-workspace");

    let result = materialize_prosoche_md(&missing);

    assert!(
        result.is_err(),
        "a missing workspace directory must surface an error, not a silent skip"
    );
}
