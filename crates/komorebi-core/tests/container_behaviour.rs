//! Characterization of `Container<W>`: the ring of windows, its focus index and the order of the
//! hide and restore calls it makes on its windows.

mod common;

use common::*;
use komorebi_core::Container;
use komorebi_core::Lockable;

fn ids(container: &Container<StubWindow>) -> Vec<u32> {
    container.windows().iter().map(|w| w.id).collect()
}

#[test]
fn a_new_container_is_empty_unlocked_and_has_an_id_of_its_own() {
    fresh();
    let a = Container::<StubWindow>::default();
    let b = Container::<StubWindow>::default();

    assert!(a.windows().is_empty());
    assert!(!a.locked);
    assert_eq!(a.focused_window_idx(), 0);
    assert!(a.focused_window().is_none());
    assert_ne!(a.id, b.id);
    assert!(!a.is_preselect());
}

#[test]
fn a_preselect_container_is_empty_and_named_preselect() {
    let container = Container::<StubWindow>::preselect();

    assert_eq!(container.id, "PRESELECT");
    assert!(container.windows().is_empty());
    assert!(container.is_preselect());
}

#[test]
fn adding_windows_appends_and_focuses_the_newest() {
    fresh();
    let container = container_of(&[1, 2, 3]);

    assert_eq!(ids(&container), [1, 2, 3]);
    assert_eq!(container.focused_window_idx(), 2);
    assert_eq!(container.focused_window().map(|w| w.id), Some(3));
}

#[test]
fn adding_a_window_hides_every_other_window_at_the_spot_in_stack_order() {
    fresh();
    let mut container = container_of(&[1, 2]);
    log();

    container.add_window(StubWindow { id: 3 }, 5).unwrap();

    assert_eq!(log(), ["hide 1 @5", "hide 2 @5"]);
}

#[test]
fn adding_the_first_window_hides_nothing() {
    fresh();
    let mut container = Container::default();

    container.add_window(StubWindow { id: 1 }, 5).unwrap();

    assert!(log().is_empty());
}

#[test]
fn a_window_that_cannot_be_hidden_makes_add_window_fail_but_it_is_already_in() {
    fresh();
    let mut container = container_of(&[1, 2]);
    fail("hide 1");
    log();

    let result = container.add_window(StubWindow { id: 3 }, 5);

    assert!(result.is_err());
    // the window was pushed and focused before the hiding began, and the hiding stopped at the
    // first failure, so window 2 was not hidden
    assert_eq!(ids(&container), [1, 2, 3]);
    assert_eq!(container.focused_window_idx(), 2);
    assert_eq!(log(), ["hide 1 @5"]);
}

#[test]
fn removing_a_window_focuses_the_one_before_its_index_whatever_was_focused() {
    fresh();
    // (removed index, the focus index afterwards) for a container of four windows with the last
    // one focused
    for (removed, focused_after) in [(0, 0), (1, 0), (2, 1), (3, 2)] {
        let mut container = container_of(&[10, 11, 12, 13]);

        let window = container.remove_window_by_idx(removed).unwrap();

        assert_eq!(window.id, 10 + removed as u32);
        assert_eq!(container.windows().len(), 3);
        assert_eq!(
            container.focused_window_idx(),
            focused_after,
            "after removing index {removed}"
        );
    }
}

#[test]
fn removing_a_window_does_not_touch_the_windows_that_are_left() {
    fresh();
    let mut container = container_of(&[1, 2, 3]);
    log();

    container.remove_window_by_idx(1);

    assert!(log().is_empty());
    assert_eq!(ids(&container), [1, 3]);
}

#[test]
fn removing_the_last_window_leaves_an_empty_container_focused_at_zero() {
    fresh();
    let mut container = container_of(&[1]);

    let window = container.remove_focused_window();

    assert_eq!(window.map(|w| w.id), Some(1));
    assert!(container.windows().is_empty());
    assert_eq!(container.focused_window_idx(), 0);
    assert!(container.focused_window().is_none());
}

#[test]
fn removing_from_an_empty_container_gives_nothing_and_keeps_focus_zero() {
    fresh();
    let mut container = Container::<StubWindow>::default();

    assert!(container.remove_focused_window().is_none());
    assert!(container.remove_window_by_idx(0).is_none());
    assert_eq!(container.focused_window_idx(), 0);
}

#[test]
fn removing_past_the_end_removes_nothing_and_leaves_the_focus_dangling() {
    fresh();
    let mut container = container_of(&[1, 2]);

    assert!(container.remove_window_by_idx(5).is_none());

    // the focus moved to index 4 although there are two windows: the ring does not clamp it
    assert_eq!(container.windows().len(), 2);
    assert_eq!(container.focused_window_idx(), 4);
    assert!(container.focused_window().is_none());
}

#[test]
fn removing_the_focused_window_of_a_stack_focuses_the_one_before_it() {
    fresh();
    let mut container = container_of(&[1, 2, 3]);
    container.focus_window(1);

    container.remove_focused_window();

    assert_eq!(ids(&container), [1, 3]);
    assert_eq!(container.focused_window_idx(), 0);
}

#[test]
fn focusing_a_window_only_moves_the_index_and_makes_no_window_call() {
    fresh();
    let mut container = container_of(&[1, 2, 3]);
    log();

    container.focus_window(0);

    assert_eq!(container.focused_window_idx(), 0);
    assert!(log().is_empty());
}

#[test]
fn windows_are_found_by_id_and_by_executable() {
    fresh();
    let container = container_of(&[4, 5, 6]);

    assert_eq!(container.idx_for_window(6), Some(2));
    assert!(container.contains_window(4));
    assert!(!container.contains_window(7));
    assert_eq!(container.idx_from_exe("exe5"), Some(1));
    assert_eq!(container.idx_from_exe("exe9"), None);
}

#[test]
fn loading_the_focused_window_walks_the_windows_in_order_and_stops_at_a_failure() {
    fresh();
    let mut container = container_of(&[1, 2, 3]);
    container.focus_window(1);
    fail("hide_stacked 1");
    log();

    let result = container.load_focused_window(3);

    assert!(result.is_err());
    // window 1 failed first, so neither the focused window 2 nor window 3 was reached
    assert_eq!(log(), ["hide_stacked 1 @3"]);
}

#[test]
fn loading_the_focused_window_of_a_single_window_container_restores_it_the_stacked_way() {
    fresh();
    let mut container = container_of(&[1]);

    container.load_focused_window(3).unwrap();

    assert_eq!(log(), ["restore_stacked 1"]);
}

#[test]
fn loading_an_empty_container_calls_nothing() {
    fresh();
    let mut container = Container::<StubWindow>::default();

    container.load_focused_window(3).unwrap();

    assert!(log().is_empty());
}

#[test]
fn hiding_goes_from_the_back_and_carries_on_after_a_window_that_fails() {
    fresh();
    let container = container_of(&[1, 2, 3]);
    fail("hide 3");
    log();

    let result = container.hide(None, 9);

    assert!(result.is_ok());
    assert_eq!(log(), ["hide 3 @9", "hide 2 @9", "hide 1 @9"]);
}

#[test]
fn hiding_with_an_omitted_window_spares_only_that_window() {
    fresh();
    let container = container_of(&[1, 2, 3]);
    log();

    container.hide(Some(2), 9).unwrap();

    assert_eq!(log(), ["hide 3 @9", "hide 1 @9"]);
}

#[test]
fn hiding_with_an_omitted_window_that_is_not_in_the_container_hides_them_all() {
    fresh();
    let container = container_of(&[1, 2]);
    log();

    container.hide(Some(42), 9).unwrap();

    assert_eq!(log(), ["hide 2 @9", "hide 1 @9"]);
}

#[test]
fn restoring_shows_only_the_focused_window_and_reports_a_failure() {
    fresh();
    let mut container = container_of(&[1, 2]);
    container.focus_window(0);
    log();

    container.restore().unwrap();
    assert_eq!(log(), ["restore 1"]);

    fail("restore 1");
    assert!(container.restore().is_err());
}

#[test]
fn restoring_an_empty_container_is_not_an_error() {
    fresh();
    let container = Container::<StubWindow>::default();

    assert!(container.restore().is_ok());
    assert!(log().is_empty());
}

#[test]
fn a_container_can_be_locked_and_unlocked() {
    let mut container = Container::<StubWindow>::default();

    container.set_locked(true);
    assert!(container.locked());
    container.set_locked(false);
    assert!(!container.locked());
}

#[test]
fn a_container_with_windows_is_written_as_a_ring_with_its_focus_index() {
    fresh();
    let mut container = container_of(&[1, 2]);
    container.id = "c".to_string();
    container.focus_window(0);

    assert_eq!(
        serde_json::to_value(&container).unwrap(),
        serde_json::json!({
            "id": "c",
            "locked": false,
            "windows": { "elements": [{ "id": 1 }, { "id": 2 }], "focused": 0 }
        })
    );
}
