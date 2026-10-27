//! The window manager driven end to end against an in-memory desktop: compositor events go in,
//! and what comes out is the geometry of the windows and the commands sent to the backend.
//!
//! Window ids are unique per test (10_001 and up), because tests in one binary share the process
//! and anything process-wide in it.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::Rect;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::sync::Arc;

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

fn activated(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::FocusChange(
        SystemNotification::KWin(KWinNotification::WindowActivated),
        Window::from(id),
    )
}

fn removed(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Destroy(
        SystemNotification::KWin(KWinNotification::WindowRemoved),
        Window::from(id),
    )
}

fn minimized(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Minimize(
        SystemNotification::KWin(KWinNotification::WindowMinimized),
        Window::from(id),
    )
}

fn restored(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Restore(
        SystemNotification::KWin(KWinNotification::WindowRestored),
        Window::from(id),
    )
}

/// A window manager over a fake desktop with one 1920x1080 monitor (1920x1050 without the panel)
fn desktop() -> (Arc<FakeBackend>, WindowManager) {
    desktop_on(FakeBackend::with_one_monitor())
}

fn desktop_on(fake: FakeBackend) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(fake);

    let wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new().unwrap();
        wm.init().unwrap();
        wm
    });

    (fake, wm)
}

fn open(fake: &FakeBackend, id: u32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
}

fn work_area() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1050,
    }
}

fn inside(rect: &Rect, area: &Rect) -> bool {
    rect.left >= area.left
        && rect.top >= area.top
        && rect.left + rect.right <= area.left + area.right
        && rect.top + rect.bottom <= area.top + area.bottom
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.left < b.left + b.right
        && b.left < a.left + a.right
        && a.top < b.top + b.bottom
        && b.top < a.top + a.bottom
}

#[test]
fn a_new_window_is_tiled_into_the_work_area() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_001);

    with_backend(fake.clone(), || wm.process_event(added(10_001))).unwrap();

    let rect = fake.rect_of(10_001).unwrap();
    assert!(
        inside(&rect, &work_area()),
        "{rect:?} is outside the work area"
    );
    assert!(
        rect.right > 1800 && rect.bottom > 1000,
        "{rect:?} does not fill the work area"
    );
    assert!(wm.focused_workspace().unwrap().contains_window(10_001));
}

#[test]
fn two_windows_split_the_work_area_without_overlapping() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_011);
    open(&fake, 10_012);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_011))?;
        wm.process_event(added(10_012))
    })
    .unwrap();

    let (a, b) = (fake.rect_of(10_011).unwrap(), fake.rect_of(10_012).unwrap());
    assert!(inside(&a, &work_area()) && inside(&b, &work_area()));
    assert!(!overlap(&a, &b), "{a:?} overlaps {b:?}");
    assert!(
        a.right < 1000 && b.right < 1000,
        "neither should be full width: {a:?} {b:?}"
    );
}

#[test]
fn closing_a_window_gives_its_space_to_the_other() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_021);
    open(&fake, 10_022);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_021))?;
        wm.process_event(added(10_022))?;
        fake.close_window_now(10_022);
        wm.process_event(removed(10_022))
    })
    .unwrap();

    assert!(!wm.focused_workspace().unwrap().contains_window(10_022));
    let rect = fake.rect_of(10_021).unwrap();
    assert!(
        rect.right > 1800,
        "the survivor should fill the work area: {rect:?}"
    );
}

#[test]
fn dialogs_and_other_non_normal_windows_are_left_alone() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_041);
    fake.set_normal(10_041, false);
    fake.clear_commands();

    with_backend(fake.clone(), || wm.process_event(added(10_041))).unwrap();

    assert!(!wm.focused_workspace().unwrap().contains_window(10_041));
    assert!(
        fake.commands().is_empty(),
        "nothing should have been asked of it"
    );
}

#[test]
fn a_minimize_by_the_user_unmanages_the_window_until_it_is_restored() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_071);
    open(&fake, 10_072);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_071))?;
        wm.process_event(added(10_072))?;
        wm.process_event(minimized(10_072))
    })
    .unwrap();

    assert!(!wm.focused_workspace().unwrap().contains_window(10_072));
    assert!(wm.minimized_windows.contains_key(&10_072));
    assert!(fake.rect_of(10_071).unwrap().right > 1800);

    with_backend(fake.clone(), || wm.process_event(restored(10_072))).unwrap();

    assert!(wm.focused_workspace().unwrap().contains_window(10_072));
    assert!(!wm.minimized_windows.contains_key(&10_072));
}

#[test]
fn the_window_given_the_focus_is_in_the_focused_container() {
    let (fake, mut wm) = desktop();
    open(&fake, 10_601);
    open(&fake, 10_602);

    with_backend(fake.clone(), || {
        wm.process_event(added(10_601))?;
        wm.process_event(added(10_602))?;
        // the user clicks the first window
        fake.set_foreground(Some(10_601));
        wm.process_event(activated(10_601))
    })
    .unwrap();

    assert_eq!(wm.focused_container_idx().unwrap(), 0);
    assert_eq!(wm.focused_window().unwrap().id, 10_601);
}

#[test]
fn a_window_that_was_open_before_komorebi_started_is_removed_when_it_closes() {
    // the first event komorebi sees may be the close of a window it picked up at startup
    let fake = FakeBackend::with_one_monitor();
    fake.open_window(10_301, "foot", "foot", "terminal 10301");
    fake.open_window(10_302, "foot", "foot", "terminal 10302");
    let (fake, mut wm) = desktop_on(fake);

    with_backend(fake.clone(), || {
        fake.close_window_now(10_301);
        wm.process_event(removed(10_301))
    })
    .unwrap();

    let remaining: Vec<u32> = wm
        .focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .flat_map(|c| c.windows().iter().map(|w| w.id))
        .collect();
    assert_eq!(
        remaining,
        [10_302],
        "a closed window is still in a container"
    );
}
