use crate::test_fixtures::test_store;
use crate::types::Role;

#[test]
fn notes_crud() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .add_note("ses-1", "syn", "task", "do something")
        .expect("add note");
    store
        .add_note("ses-1", "syn", "context", "background")
        .expect("add note");

    let notes = store.get_notes("ses-1").expect("get notes");
    assert_eq!(notes.len(), 2);

    let note_id = notes[0].id;
    let deleted = store.delete_note(note_id).expect("delete note");
    assert!(deleted);
    let notes_after = store.get_notes("ses-1").expect("get notes after delete");
    assert_eq!(notes_after.len(), 1);
}

#[test]
fn delete_session_removes_notes_via_session_gid_index() {
    // WHY: regression test for issue #5698 — deleting one session must not
    // require scanning the global `gid:` key space and must leave other
    // sessions' notes intact.
    let store = test_store();
    store
        .create_session("ses-a", "syn", "main", None, None)
        .expect("create a");
    store
        .create_session("ses-b", "syn", "secondary", None, None)
        .expect("create b");

    store
        .add_note("ses-a", "syn", "task", "note for a")
        .expect("add note to a");
    let id_b = store
        .add_note("ses-b", "syn", "task", "note for b")
        .expect("add note to b");

    let deleted = store.delete_session("ses-a").expect("delete a");
    assert!(deleted);

    assert!(
        store
            .find_session_by_id("ses-a")
            .expect("query a")
            .is_none(),
        "session a must be removed"
    );
    assert!(
        store.get_notes("ses-a").expect("notes a").is_empty(),
        "session a notes must be removed"
    );
    let remaining = store.get_notes("ses-b").expect("notes b");
    assert_eq!(remaining.len(), 1, "session b notes must survive");
    assert_eq!(remaining[0].id, id_b);

    // NOTE: deleting by global id must also clean the reverse index.
    assert!(store.delete_note(id_b).expect("delete b's note"));
}

#[test]
fn delete_session_removes_all_data() {
    let store = test_store();
    store
        .create_session("ses-1", "syn", "main", None, None)
        .expect("create");
    store
        .append_message("ses-1", Role::User, "hi", None, None, 10)
        .expect("append");

    let deleted = store.delete_session("ses-1").expect("delete");
    assert!(deleted);
    assert!(store.find_session_by_id("ses-1").expect("query").is_none());
    assert!(
        store
            .get_history("ses-1", None)
            .expect("history")
            .is_empty()
    );
}

#[test]
fn ping_succeeds() {
    let store = test_store();
    store.ping().expect("ping should succeed");
}
