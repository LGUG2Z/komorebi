//! Which windows there are, from komorebi for KDE's own state.
//!
//! A Wayland client cannot list other programs' windows, and KWin has no call that does. What
//! komorebi for KDE tells a client is its `State`: the windows it manages, with the rect each has
//! and the program that owns it. So the windows listed here are komorebi's, and a window that
//! komorebi ignores (a panel, a dialog, the recorder's own window) is not among them. A window is
//! shown when its workspace is the one showing on its monitor and it is the one in front of its
//! stack.
//!
//! The state is asked of komorebi at each sample, which is a message over a socket and not a
//! picture: komorebi not running, or not answering, is an empty list.

use crate::analysis::flavor::Flavor;
use crate::analysis::state::Container;
use crate::analysis::state::Snapshot;
use crate::analysis::state::Window;
use crate::komorebi_link;
use crate::sources::managed_apps::WindowOwner;
use crate::sources::windowtrack::WindowSample;
use std::collections::HashMap;

/// Returns no windows: the state komorebi for KDE gives has no process id to say who owns one.
#[must_use]
pub fn window_owners() -> HashMap<u32, WindowOwner> {
    HashMap::new()
}

/// One window of the state as a sample, shown or not.
fn sample(window: &Window, visible: bool) -> Option<WindowSample> {
    let rect = window.rect.as_ref()?;
    Some(WindowSample {
        id: u32::try_from(window.id).ok()?,
        owner: window.exe.clone(),
        title: Some(window.title.clone()).filter(|title| !title.is_empty()),
        rect: [
            i32::try_from(rect.left).ok()?,
            i32::try_from(rect.top).ok()?,
            i32::try_from(rect.right_edge()).ok()?,
            i32::try_from(rect.bottom_edge()).ok()?,
        ],
        visible,
        minimized: false,
        cloaked: None,
        border: false,
        border_of: None,
    })
}

fn container(found: &mut Vec<WindowSample>, container: &Container, workspace_showing: bool) {
    for (tab, window) in container.windows.iter().enumerate() {
        found.extend(sample(
            window,
            workspace_showing && tab == container.focused,
        ));
    }
}

/// Returns the windows of a komorebi `State`, as samples.
#[must_use]
pub fn windows_of(state: &serde_json::Value) -> Vec<WindowSample> {
    let snapshot = Snapshot::parse(Flavor::Kde, state);
    let mut found = Vec::new();
    for monitor in &snapshot.monitors {
        for (index, workspace) in monitor.workspaces.iter().enumerate() {
            let showing = index == monitor.focused;
            for tile in &workspace.containers {
                container(&mut found, tile, showing);
            }
            if let Some(monocle) = &workspace.monocle {
                container(&mut found, monocle, showing);
            }
            if let Some(maximized) = &workspace.maximized {
                found.extend(sample(maximized, showing));
            }
            for window in &workspace.floating {
                found.extend(sample(window, showing));
            }
        }
    }
    found
}

/// Returns the windows komorebi for KDE manages, or none if it cannot be asked.
#[must_use]
pub fn window_list() -> Vec<WindowSample> {
    let Ok(text) = komorebi_link::query_state() else {
        return Vec::new();
    };
    serde_json::from_str(&text)
        .map(|state| windows_of(&state))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn window(id: u64, exe: &str, rect: [i64; 4]) -> serde_json::Value {
        json!({
            "id": id,
            "rect": {"left": rect[0], "top": rect[1], "right": rect[2], "bottom": rect[3]},
            "details": {"title": format!("window {id}"), "exe": exe, "class": exe, "path": ""}
        })
    }

    fn ring(elements: &[serde_json::Value], focused: usize) -> serde_json::Value {
        json!({"elements": elements, "focused": focused})
    }

    fn state() -> serde_json::Value {
        let stack = json!({
            "id": "c1",
            "windows": ring(&[window(1, "konsole", [0, 0, 800, 600]), window(2, "dolphin", [0, 0, 800, 600])], 1)
        });
        let single =
            json!({"id": "c2", "windows": ring(&[window(3, "firefox", [800, 0, 800, 600])], 0)});
        let elsewhere =
            json!({"id": "c3", "windows": ring(&[window(4, "kate", [0, 0, 1600, 600])], 0)});
        let workspace = |containers: Vec<serde_json::Value>| {
            json!({
                "name": "",
                "containers": ring(&containers, 0),
                "monocle_container": null,
                "maximized_window": null,
                "floating_windows": ring(&[window(5, "okular", [100, 100, 300, 300])], 0),
                "layout": {"Default": "BSP"}
            })
        };
        json!({
            "monitors": ring(&[json!({
                "id": 9,
                "device": "DP-1",
                "size": {"left": 0, "top": 0, "right": 1600, "bottom": 900},
                "work_area_size": {"left": 0, "top": 0, "right": 1600, "bottom": 900},
                "workspaces": ring(&[workspace(vec![stack, single]), workspace(vec![elsewhere])], 0)
            })], 0)
        })
    }

    #[test]
    fn a_stack_shows_one_window_and_another_workspace_shows_none() {
        let windows = windows_of(&state());
        let shown = |id: u32| {
            windows
                .iter()
                .find(|window| window.id == id)
                .map(|window| window.visible)
        };
        // the one in front of the stack, the single window, and the floating one are shown
        assert_eq!(shown(2), Some(true));
        assert_eq!(shown(3), Some(true));
        assert_eq!(shown(5), Some(true));
        // the one behind it, and the one of the workspace that is not showing, are not
        assert_eq!(shown(1), Some(false));
        assert_eq!(shown(4), Some(false));
    }

    #[test]
    fn a_window_is_its_program_title_and_edges() {
        let windows = windows_of(&state());
        let firefox = windows
            .iter()
            .find(|window| window.id == 3)
            .expect("window");
        assert_eq!(firefox.owner, "firefox");
        assert_eq!(firefox.title.as_deref(), Some("window 3"));
        assert_eq!(firefox.rect, [800, 0, 1600, 600]);
        assert_eq!(firefox.cloaked, None);
    }

    #[test]
    fn no_state_is_no_windows() {
        assert!(windows_of(&json!({})).is_empty());
    }
}
