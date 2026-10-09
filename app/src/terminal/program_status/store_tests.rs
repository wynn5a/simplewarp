use super::*;
use crate::terminal::program_status::protocol::parse;

fn apply(store: &mut ProgramStatusStore, body: &str) -> bool {
    store.apply(parse(body.as_bytes()).unwrap(), ReportSource::Osc7501)
}

fn bridged(store: &mut ProgramStatusStore, body: &str) -> bool {
    store.apply(parse(body.as_bytes()).unwrap(), ReportSource::Osc94)
}

fn state_of(store: &ProgramStatusStore) -> Option<ProgramState> {
    store.root().map(|record| record.state)
}

#[test]
fn a_report_replaces_its_record_without_merging() {
    let mut store = ProgramStatusStore::default();
    assert!(apply(&mut store, "state=working:app=cargo:progress=40"));
    assert!(apply(&mut store, "state=working"));
    let root = store.root().unwrap();
    assert_eq!(root.app, None);
    assert_eq!(root.progress, None);
}

#[test]
fn an_identical_report_changes_nothing() {
    let mut store = ProgramStatusStore::default();
    assert!(apply(&mut store, "state=working:progress=40"));
    assert!(!apply(&mut store, "state=working:progress=40"));
}

#[test]
fn clear_removes_the_addressed_subtree_only() {
    let mut store = ProgramStatusStore::default();
    for id in ["build", "build/test", "build/test/unit", "buildx"] {
        apply(&mut store, &format!("state=working:id={id}"));
    }
    assert!(apply(&mut store, "state=clear:id=build"));
    assert_eq!(store.records.len(), 1);
    assert!(
        store
            .records
            .contains_key(&RecordPath::parse("buildx").unwrap())
    );
    assert!(!apply(&mut store, "state=clear:id=build"));
}

#[test]
fn clear_without_an_id_removes_everything() {
    let mut store = ProgramStatusStore::default();
    apply(&mut store, "state=working");
    apply(&mut store, "state=working:id=a/b");
    assert!(apply(&mut store, "state=clear"));
    assert!(store.is_empty());
}

#[test]
fn the_record_cap_rejects_new_records_but_not_updates() {
    let mut store = ProgramStatusStore::default();
    for i in 0..MAX_RECORDS {
        assert!(apply(&mut store, &format!("state=working:id=r{i}")));
    }
    assert!(!apply(&mut store, "state=working:id=overflow"));
    assert_eq!(store.records.len(), MAX_RECORDS);
    assert!(apply(&mut store, "state=done:id=r0"));
}

#[test]
fn drop_running_keeps_finished_and_idle_records() {
    let mut store = ProgramStatusStore::default();
    for (id, state) in [
        ("w", "working"),
        ("b", "blocked"),
        ("d", "done"),
        ("e", "error"),
        ("i", "idle"),
    ] {
        apply(&mut store, &format!("state={state}:id={id}"));
    }
    assert!(store.drop_running());
    let mut left: Vec<_> = store.records.values().map(|r| r.state).collect();
    left.sort_by_key(|state| format!("{state:?}"));
    assert_eq!(
        left,
        [ProgramState::Done, ProgramState::Error, ProgramState::Idle]
    );
    assert!(!store.drop_running());
}

#[test]
fn drop_finished_keeps_everything_else() {
    let mut store = ProgramStatusStore::default();
    for (id, state) in [
        ("w", "working"),
        ("d", "done"),
        ("e", "error"),
        ("i", "idle"),
    ] {
        apply(&mut store, &format!("state={state}:id={id}"));
    }
    assert!(store.drop_finished());
    assert_eq!(store.records.len(), 2);
    assert!(!store.drop_finished());
}

#[test]
fn reset_clears_records_and_the_bridge_latch() {
    let mut store = ProgramStatusStore::default();
    apply(&mut store, "state=done");
    assert!(store.reset());
    assert!(store.is_empty());
    assert!(bridged(&mut store, "state=working"));
}

#[test]
fn the_bridge_works_until_a_real_report_arrives() {
    let mut store = ProgramStatusStore::default();
    assert!(bridged(&mut store, "state=working:progress=10"));
    assert_eq!(state_of(&store), Some(ProgramState::Working));
    assert!(apply(&mut store, "state=done"));
    assert!(!bridged(&mut store, "state=working:progress=20"));
    assert_eq!(state_of(&store), Some(ProgramState::Done));
}

#[test]
fn the_bridge_latch_survives_clearing_the_last_record() {
    let mut store = ProgramStatusStore::default();
    apply(&mut store, "state=working");
    apply(&mut store, "state=clear");
    assert!(store.is_empty());
    assert!(!bridged(&mut store, "state=working"));
}

#[test]
fn debug_output_omits_untrusted_text() {
    let mut store = ProgramStatusStore::default();
    apply(&mut store, "state=idle:msg=c2VjcmV0");
    assert!(store.root().unwrap().message.is_some());
    assert!(!format!("{store:?}").contains("secret"));
}
