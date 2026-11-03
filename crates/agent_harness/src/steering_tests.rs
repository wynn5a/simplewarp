use super::*;

const STEER: QueuedRow = QueuedRow::Steer { editing: false };
const EDITED_STEER: QueuedRow = QueuedRow::Steer { editing: true };

#[test]
fn a_steer_goes_to_the_front_of_an_empty_or_plain_queue() {
    assert_eq!(placement(&[]), Placement::InsertAt(0));
    assert_eq!(
        placement(&[QueuedRow::Other, QueuedRow::Other]),
        Placement::InsertAt(0)
    );
}

#[test]
fn a_steer_merges_into_the_steer_ahead_of_it() {
    assert_eq!(
        placement(&[STEER, QueuedRow::Other]),
        Placement::MergeInto(0)
    );
    assert_eq!(
        placement(&[EDITED_STEER, STEER, QueuedRow::Other]),
        Placement::MergeInto(1)
    );
}

#[test]
fn a_steer_being_edited_is_not_merged_into() {
    assert_eq!(
        placement(&[EDITED_STEER, QueuedRow::Other]),
        Placement::InsertAt(1)
    );
}

#[test]
fn a_steer_goes_after_locked_rows_at_the_head() {
    assert_eq!(
        placement(&[QueuedRow::Locked, QueuedRow::Other]),
        Placement::InsertAt(1)
    );
    assert_eq!(
        placement(&[QueuedRow::Locked, STEER]),
        Placement::MergeInto(1)
    );
}

#[test]
fn merging_keeps_the_order_and_drops_blank_parts() {
    assert_eq!(merge("a ", " b"), "a\n\nb");
    assert_eq!(merge("  ", " b "), "b");
    assert_eq!(merge("a", "\n"), "a");
}

#[test]
fn the_boundary_takes_every_steer_not_being_edited() {
    assert_eq!(
        steers_to_inject(&[STEER, QueuedRow::Other, EDITED_STEER, STEER], true),
        vec![0, 3]
    );
    assert!(steers_to_inject(&[QueuedRow::Other, QueuedRow::Locked], true).is_empty());
}

#[test]
fn a_boundary_with_subagent_results_takes_nothing() {
    assert!(steers_to_inject(&[STEER, STEER], false).is_empty());
}
