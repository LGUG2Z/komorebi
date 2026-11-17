use super::*;
use crate::IdWithIdentifier;
use crate::MatchingRule;
use crate::MatchingStrategy;
use crate::container::Container;
use crate::core::ApplicationIdentifier;
use crate::monitor;
use std::path::PathBuf;
use uuid::Uuid;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_APPWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::core::PCWSTR;
use windows::core::w;

struct MonocleTest {
    wm: WindowManager,
    socket: PathBuf,
    rule: MatchingRule,
    windows: Vec<Window>,
    previous_foreground: Option<isize>,
}

impl MonocleTest {
    fn new() -> Self {
        let title = format!("komorebi-monocle-test-{}", Uuid::new_v4());
        let rule = MatchingRule::Simple(IdWithIdentifier {
            kind: ApplicationIdentifier::Title,
            id: title.clone(),
            matching_strategy: Some(MatchingStrategy::Equals),
        });
        crate::MANAGE_IDENTIFIERS.lock().push(rule.clone());

        let socket = PathBuf::from(format!("komorebi-test-{}.sock", Uuid::new_v4()));
        let (_, receiver) = crossbeam_channel::bounded(1);
        let mut wm = WindowManager::new(receiver, Some(socket.clone())).unwrap();
        wm.virtual_desktop_id = None;
        wm.mouse_follows_focus = false;

        let mut test = Self {
            wm,
            socket,
            rule,
            windows: Vec::new(),
            previous_foreground: WindowsApi::foreground_window().ok(),
        };
        let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        for _ in 0..2 {
            // No caption: a running daemon ignores these windows. Only this test's
            // process has the matching manage rule. They are initially invisible.
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_APPWINDOW,
                    w!("STATIC"),
                    PCWSTR(title.as_ptr()),
                    WS_POPUP,
                    100,
                    100,
                    400,
                    300,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap();
            test.windows.push(Window::from(hwnd.0 as isize));
        }

        let mut monitor = monitor::new(
            WindowsApi::monitor_from_window(test.windows[0].hwnd),
            Rect::default(),
            Rect::default(),
            "test".into(),
            "test".into(),
            "test".into(),
            None,
        );
        let workspace = monitor.focused_workspace_mut().unwrap();
        let mut monocle = Container::default();
        monocle.windows_mut().push_back(test.windows[0]);
        workspace.monocle_container = Some(monocle);
        workspace.monocle_container_restore_idx = Some(0);
        let mut other = Container::default();
        other.windows_mut().push_back(test.windows[1]);
        workspace.add_container_to_back(other);
        test.wm.monitors_mut().push_back(monitor);

        // The event must leave membership unchanged. Pre-populate this cache so
        // update_known_hwnds does not write to a running daemon's hwnd file.
        for window in &test.windows {
            test.wm.known_hwnds.insert(window.hwnd, (0, 0));
        }
        test
    }

    fn assert_monocle_preserved(&self) {
        let workspace = self.wm.focused_workspace().unwrap();
        assert_eq!(
            workspace
                .monocle_container
                .as_ref()
                .unwrap()
                .focused_window(),
            Some(&self.windows[0])
        );
        assert_eq!(workspace.containers().len(), 1);
        assert_eq!(workspace.containers()[0].windows().len(), 1);
        assert_eq!(
            workspace.containers()[0].focused_window(),
            Some(&self.windows[1])
        );
        assert_eq!(self.wm.known_hwnds.len(), 2);
    }
}

impl Drop for MonocleTest {
    fn drop(&mut self) {
        if self.windows.iter().any(|window| window.is_focused())
            && let Some(previous) = self.previous_foreground
        {
            WindowsApi::raise_and_focus_window(previous).unwrap();
        }
        crate::HIDDEN_HWNDS
            .lock()
            .retain(|hwnd| !self.windows.iter().any(|window| window.hwnd == *hwnd));
        for window in &self.windows {
            unsafe { DestroyWindow(window.hwnd()).unwrap() };
        }
        crate::MANAGE_IDENTIFIERS
            .lock()
            .retain(|rule| *rule != self.rule);
        std::fs::remove_file(&self.socket).unwrap();
    }
}

#[test]
fn a_managed_tiled_window_is_hidden_again_when_uncloaked_over_monocle() {
    let mut test = MonocleTest::new();
    let other = test.windows[1];
    test.assert_monocle_preserved();
    assert!(!crate::HIDDEN_HWNDS.lock().contains(&other.hwnd));
    let event = WindowManagerEvent::Uncloak(WinEvent::ObjectUncloaked, other);
    assert!(
        other
            .should_manage(Some(event), &mut RuleDebug::default())
            .unwrap()
    );

    test.wm.process_event(event).unwrap();

    assert!(crate::HIDDEN_HWNDS.lock().contains(&other.hwnd));
    test.assert_monocle_preserved();
}

#[test]
fn a_monocle_window_is_not_hidden_by_its_own_show_event() {
    let mut test = MonocleTest::new();
    let monocle = test.windows[0];
    assert!(!crate::HIDDEN_HWNDS.lock().contains(&monocle.hwnd));

    test.wm
        .process_event(WindowManagerEvent::Show(WinEvent::ObjectShow, monocle))
        .unwrap();

    assert!(!crate::HIDDEN_HWNDS.lock().contains(&monocle.hwnd));
    test.assert_monocle_preserved();
}

#[test]
fn a_floating_window_can_be_shown_over_monocle() {
    let mut test = MonocleTest::new();
    let floating = test.windows[1];
    let workspace = test.wm.focused_workspace_mut().unwrap();
    workspace.containers_mut().clear();
    workspace.floating_windows_mut().push_back(floating);
    assert_eq!(workspace.layer, WorkspaceLayer::Tiling);
    assert!(!crate::HIDDEN_HWNDS.lock().contains(&floating.hwnd));

    test.wm
        .process_event(WindowManagerEvent::Uncloak(
            WinEvent::ObjectUncloaked,
            floating,
        ))
        .unwrap();

    assert!(!crate::HIDDEN_HWNDS.lock().contains(&floating.hwnd));
    let workspace = test.wm.focused_workspace().unwrap();
    assert_eq!(
        workspace
            .monocle_container
            .as_ref()
            .unwrap()
            .focused_window(),
        Some(&test.windows[0])
    );
    assert_eq!(workspace.floating_windows().len(), 1);
    assert!(workspace.containers().is_empty());
}

#[test]
fn an_existing_tiled_window_can_be_shown_without_monocle() {
    let mut test = MonocleTest::new();
    let other = test.windows[1];
    test.wm.focused_workspace_mut().unwrap().monocle_container = None;
    test.wm.known_hwnds.remove(&test.windows[0].hwnd);
    assert!(!crate::HIDDEN_HWNDS.lock().contains(&other.hwnd));

    test.wm
        .process_event(WindowManagerEvent::Show(WinEvent::ObjectShow, other))
        .unwrap();

    assert!(!crate::HIDDEN_HWNDS.lock().contains(&other.hwnd));
    assert_eq!(test.wm.focused_workspace().unwrap().containers().len(), 1);
}

#[test]
#[ignore = "needs an interactive desktop for foreground focus and shell cloaking"]
fn an_external_activation_preserves_monocle_visibility_and_input_focus() {
    let mut test = MonocleTest::new();
    let monocle = test.windows[0];
    let other = test.windows[1];

    WindowsApi::restore_window(monocle.hwnd);
    for event in [
        WindowManagerEvent::Uncloak(WinEvent::ObjectUncloaked, other),
        WindowManagerEvent::Show(WinEvent::ObjectShow, other),
    ] {
        WindowsApi::restore_window(other.hwnd);
        other.restore();
        other.focus(false).unwrap();
        assert_eq!(WindowsApi::foreground_window().unwrap(), other.hwnd);
        assert!(!other.is_cloaked().unwrap());
        test.assert_monocle_preserved();

        test.wm.process_event(event).unwrap();

        assert!(other.is_cloaked().unwrap());
        assert_eq!(WindowsApi::foreground_window().unwrap(), monocle.hwnd);
        test.assert_monocle_preserved();
    }

    // A background uncloak must re-hide the tiled window without taking input
    // from the unmanaged foreground window.
    let unmanaged_hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            w!("STATIC"),
            w!("komorebi-monocle-test-background"),
            WS_POPUP,
            550,
            100,
            400,
            300,
            None,
            None,
            None,
            None,
        )
    }
    .unwrap();
    let unmanaged = Window::from(unmanaged_hwnd.0 as isize);
    test.windows.push(unmanaged);
    WindowsApi::restore_window(unmanaged.hwnd);
    unmanaged.focus(false).unwrap();
    other.restore();
    assert!(!other.is_cloaked().unwrap());
    assert_eq!(WindowsApi::foreground_window().unwrap(), unmanaged.hwnd);

    test.wm
        .process_event(WindowManagerEvent::Uncloak(
            WinEvent::ObjectUncloaked,
            other,
        ))
        .unwrap();

    assert!(other.is_cloaked().unwrap());
    assert_eq!(WindowsApi::foreground_window().unwrap(), unmanaged.hwnd);
    test.assert_monocle_preserved();
}
