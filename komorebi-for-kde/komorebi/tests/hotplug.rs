//! Monitors that are unplugged and plugged back, against an in-memory desktop.
//!
//! The compositor takes the windows of an output that is unplugged to the ones that are left, and
//! komorebi has to take them over there: it is the only thing that can tile them. Plugged back, the
//! monitor comes back with what komorebi kept of it; a monitor komorebi never saw gets workspaces of
//! its own.
//!
//! Window ids are in the 50_000s; the monitor cache is process-wide, so the tests that unplug a
//! monitor use names of their own.

use komorebi::backend::fake::FakeBackend;
use komorebi::backend::with_backend;
use komorebi::core::DefaultLayout;
use komorebi::core::Layout;
use komorebi::core::Rect;
use komorebi::core::SocketMessage;
use komorebi::monitor_reconciliator::reconcile_display_change;
use komorebi::static_config::Wallpaper;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use std::path::PathBuf;
use std::sync::Arc;

fn area(left: i32) -> Rect {
    Rect {
        left,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

fn work_area(left: i32) -> Rect {
    Rect {
        left,
        top: 0,
        right: 1920,
        bottom: 1050,
    }
}

fn desktop_of(names: &[&str]) -> (Arc<FakeBackend>, WindowManager) {
    let fake = Arc::new(FakeBackend::new());
    for (i, name) in names.iter().enumerate() {
        fake.add_monitor(
            50 + i as u32,
            name,
            area(1920 * i as i32),
            work_area(1920 * i as i32),
        );
    }

    let (_sender, receiver) = crossbeam_channel::bounded(1);
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let socket = std::path::PathBuf::from(format!(
        "/tmp/kmh-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));

    let wm = with_backend(fake.clone(), || {
        let mut wm = WindowManager::new(receiver, Some(socket)).unwrap();
        wm.init().unwrap();
        wm
    });

    (fake, wm)
}

fn added(id: u32) -> WindowManagerEvent {
    WindowManagerEvent::Show(
        SystemNotification::KWin(KWinNotification::WindowAdded),
        Window::from(id),
    )
}

/// where a window opens: somewhere in the area that starts at `left`
fn somewhere_in(left: i32) -> Rect {
    Rect {
        left: left + 200,
        top: 200,
        right: 800,
        bottom: 600,
    }
}

fn open_on(fake: &Arc<FakeBackend>, wm: &mut WindowManager, id: u32, left: i32) {
    fake.open_window(id, "foot", "foot", &format!("terminal {id}"));
    fake.move_window_by_user(id, somewhere_in(left));
    with_backend(fake.clone(), || wm.process_event(added(id))).unwrap();
}

fn containers_on(wm: &WindowManager, monitor: usize) -> usize {
    wm.monitors()[monitor]
        .focused_workspace()
        .map_or(0, |w| w.containers().len())
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

fn assert_tiled_in(fake: &FakeBackend, ids: &[u32], area: &Rect) {
    let rects: Vec<Rect> = ids.iter().map(|id| fake.rect_of(*id).unwrap()).collect();
    for (i, a) in rects.iter().enumerate() {
        assert!(inside(a, area), "{a:?} is not inside {area:?}");
        for b in &rects[i + 1..] {
            assert!(!overlap(a, b), "{a:?} and {b:?} overlap");
        }
    }
}

#[test]
fn a_monitor_that_was_never_there_gets_windows_of_its_own() {
    let (fake, mut wm) = desktop_of(&["Test-C"]);
    open_on(&fake, &mut wm, 50_101, 0);

    fake.add_monitor(61, "Test-D", area(1920), work_area(1920));
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();
    assert_eq!(wm.monitors().len(), 2);

    open_on(&fake, &mut wm, 50_102, 1920);
    open_on(&fake, &mut wm, 50_103, 1920);

    assert_eq!(containers_on(&wm, 0), 1);
    assert_eq!(containers_on(&wm, 1), 2);
    assert_tiled_in(&fake, &[50_102, 50_103], &work_area(1920));
}

#[test]
fn a_stack_goes_with_the_monitor_as_a_stack() {
    let (fake, mut wm) = desktop_of(&["Test-G", "Test-H"]);
    open_on(&fake, &mut wm, 50_301, 0);
    for id in [50_302, 50_303] {
        open_on(&fake, &mut wm, id, 1920);
    }
    // 50_303 onto 50_302's container on the second monitor
    wm.focus_monitor(1).unwrap();
    with_backend(fake.clone(), || Window::from(50_303).focus(false)).unwrap();
    with_backend(fake.clone(), || {
        wm.process_command(
            SocketMessage::StackWindow(komorebi::core::OperationDirection::Left),
            Vec::new(),
        )
    })
    .unwrap();
    assert_eq!(containers_on(&wm, 1), 1, "the two windows are one stack");

    fake.remove_monitor(51);
    for id in [50_302, 50_303] {
        fake.move_window_by_user(id, somewhere_in(100));
    }
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    assert_eq!(wm.monitors().len(), 1);
    let stacked = wm.monitors()[0]
        .focused_workspace()
        .unwrap()
        .containers()
        .iter()
        .any(|c| c.windows().len() == 2);
    assert!(stacked, "the stack was taken apart");
}

#[test]
fn the_windows_of_a_monitor_that_goes_are_tiled_on_the_one_that_is_left() {
    let (fake, mut wm) = desktop_of(&["Test-J", "Test-K"]);
    for id in [50_501, 50_502] {
        open_on(&fake, &mut wm, id, 0);
    }
    for id in [50_503, 50_504] {
        open_on(&fake, &mut wm, id, 1920);
    }
    assert_eq!(containers_on(&wm, 0), 2);
    assert_eq!(containers_on(&wm, 1), 2);

    // the second monitor is unplugged, and the compositor takes its windows to the first
    fake.remove_monitor(51);
    for id in [50_503, 50_504] {
        fake.move_window_by_user(id, somewhere_in(100));
    }
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    assert_eq!(
        wm.monitors().len(),
        1,
        "the monitor that went is still there"
    );
    assert_eq!(containers_on(&wm, 0), 4, "its windows were not taken over");
    for id in 50_501..=50_504 {
        assert_eq!(
            wm.known_window_ids.get(&id).map(|(m, _)| *m),
            Some(0),
            "window {id} is not managed on the monitor that is left"
        );
    }
    assert_tiled_in(&fake, &[50_501, 50_502, 50_503, 50_504], &work_area(0));
}

#[test]
fn a_monitor_that_comes_back_gets_what_was_kept_of_it_and_none_of_the_windows_it_had() {
    let (fake, mut wm) = desktop_of(&["Test-L", "Test-M"]);
    open_on(&fake, &mut wm, 50_601, 0);
    open_on(&fake, &mut wm, 50_602, 1920);
    wm.monitors_mut()[1].workspace_padding = Some(40);
    wm.monitors_mut()[1].workspaces_mut()[0].layout = Layout::Default(DefaultLayout::Columns);
    wm.monitors_mut()[1].wallpaper = Some(Wallpaper {
        path: PathBuf::from("/tmp/kmf-hotplug-wallpaper.png"),
        generate_theme: Some(false),
        theme_options: None,
    });

    fake.remove_monitor(51);
    fake.move_window_by_user(50_602, somewhere_in(100));
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();
    assert_eq!(wm.monitors().len(), 1);
    assert_eq!(containers_on(&wm, 0), 2);

    fake.add_monitor(51, "Test-M", area(1920), work_area(1920));
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    assert_eq!(wm.monitors().len(), 2, "the monitor did not come back");
    let monitor = &wm.monitors()[1];
    assert_eq!(monitor.workspace_padding, Some(40));
    assert_eq!(
        monitor.workspaces()[0].layout,
        Layout::Default(DefaultLayout::Columns)
    );
    assert_eq!(
        monitor
            .wallpaper
            .as_ref()
            .map(|wallpaper| wallpaper.path.clone()),
        Some(PathBuf::from("/tmp/kmf-hotplug-wallpaper.png")),
        "it came back without its wallpaper"
    );
    assert_eq!(
        containers_on(&wm, 1),
        0,
        "it came back with a window that is tiled elsewhere"
    );
    assert_eq!(containers_on(&wm, 0), 2);
}

#[test]
fn a_desktop_that_reports_no_monitor_is_left_as_it_was() {
    let (fake, mut wm) = desktop_of(&["Test-N"]);
    open_on(&fake, &mut wm, 50_701, 0);

    fake.remove_monitor(50);
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    assert_eq!(wm.monitors().len(), 1);
    assert_eq!(containers_on(&wm, 0), 1);
}

#[test]
fn a_monocle_of_a_monitor_that_goes_comes_back_as_a_container_with_the_windows_it_hid() {
    let (fake, mut wm) = desktop_of(&["Test-O", "Test-P"]);
    open_on(&fake, &mut wm, 50_801, 0);
    for id in [50_802, 50_803] {
        open_on(&fake, &mut wm, id, 1920);
    }
    wm.focus_monitor(1).unwrap();
    with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::ToggleMonocle, Vec::new())
    })
    .unwrap();
    let monocle = wm.monitors()[1].focused_workspace().unwrap();
    assert!(monocle.monocle_container.is_some(), "there is no monocle");
    assert!(fake.is_hidden(50_802) || fake.is_hidden(50_803));

    fake.remove_monitor(51);
    for id in [50_802, 50_803] {
        fake.move_window_by_user(id, somewhere_in(100));
    }
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    assert_eq!(wm.monitors().len(), 1);
    let workspace = wm.monitors()[0].focused_workspace().unwrap();
    assert!(workspace.monocle_container.is_none());
    assert_eq!(
        containers_on(&wm, 0),
        3,
        "a window was lost with the monocle"
    );
    for id in 50_801..=50_803 {
        assert!(!fake.is_hidden(id), "window {id} is still hidden");
    }
    assert_tiled_in(&fake, &[50_801, 50_802, 50_803], &work_area(0));
}

#[test]
fn a_maximized_window_of_a_monitor_that_goes_is_not_lost() {
    let (fake, mut wm) = desktop_of(&["Test-Q", "Test-R"]);
    open_on(&fake, &mut wm, 50_901, 0);
    for id in [50_902, 50_903] {
        open_on(&fake, &mut wm, id, 1920);
    }
    wm.focus_monitor(1).unwrap();
    with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::ToggleMaximize, Vec::new())
    })
    .unwrap();
    let maximized = wm.monitors()[1]
        .focused_workspace()
        .unwrap()
        .maximized_window
        .map(|window| window.id)
        .expect("there is no maximized window");

    fake.remove_monitor(51);
    for id in [50_902, 50_903] {
        fake.move_window_by_user(id, somewhere_in(100));
    }
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    assert_eq!(wm.monitors().len(), 1);
    assert!(
        wm.monitors()[0]
            .focused_workspace()
            .unwrap()
            .contains_window(maximized),
        "the maximized window was lost with its monitor"
    );
    for id in 50_901..=50_903 {
        assert_eq!(
            wm.known_window_ids.get(&id).map(|(m, _)| *m),
            Some(0),
            "window {id} is not managed on the monitor that is left"
        );
    }
}

#[test]
fn a_maximized_window_of_a_monitor_that_goes_stays_maximized() {
    let (fake, mut wm) = desktop_of(&["Test-S", "Test-T"]);
    open_on(&fake, &mut wm, 50_951, 0);
    for id in [50_952, 50_953] {
        open_on(&fake, &mut wm, id, 1920);
    }
    wm.focus_monitor(1).unwrap();
    with_backend(fake.clone(), || {
        wm.process_command(SocketMessage::ToggleMaximize, Vec::new())
    })
    .unwrap();
    let maximized = wm.monitors()[1]
        .focused_workspace()
        .unwrap()
        .maximized_window
        .map(|window| window.id)
        .expect("there is no maximized window");

    fake.remove_monitor(51);
    for id in [50_952, 50_953] {
        fake.move_window_by_user(id, somewhere_in(100));
    }
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    assert_eq!(wm.monitors().len(), 1);
    let workspace = wm.monitors()[0].focused_workspace().unwrap();
    assert_eq!(workspace.maximized_window.map(|w| w.id), Some(maximized));
    assert!(fake.is_maximized(maximized), "it was restored on the way");
    assert_eq!(
        containers_on(&wm, 0),
        2,
        "the windows it was not maximized with are tiles, and the maximized window is none"
    );
}

#[test]
fn a_maximized_window_that_comes_to_a_workspace_with_one_takes_its_place() {
    let (fake, mut wm) = desktop_of(&["Test-U", "Test-V"]);
    open_on(&fake, &mut wm, 50_961, 0);
    open_on(&fake, &mut wm, 50_962, 1920);
    for monitor in [0, 1] {
        wm.focus_monitor(monitor).unwrap();
        with_backend(fake.clone(), || {
            wm.process_command(SocketMessage::ToggleMaximize, Vec::new())
        })
        .unwrap();
    }
    assert!(fake.is_maximized(50_961) && fake.is_maximized(50_962));

    fake.remove_monitor(51);
    fake.move_window_by_user(50_962, somewhere_in(100));
    with_backend(fake.clone(), || reconcile_display_change(&mut wm)).unwrap();

    let workspace = wm.monitors()[0].focused_workspace().unwrap();
    assert_eq!(
        workspace.maximized_window.map(|w| w.id),
        Some(50_962),
        "the window that came is the maximized one"
    );
    assert!(fake.is_maximized(50_962));
    assert!(
        !fake.is_maximized(50_961),
        "the window that was maximized is restored"
    );
    assert_eq!(containers_on(&wm, 0), 1, "it is a tile again");
    assert!(workspace.container_idx_for_window(50_961).is_some());
}
