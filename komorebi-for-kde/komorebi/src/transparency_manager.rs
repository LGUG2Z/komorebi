//! Transparency for the windows that do not have the focus (`transparency`, `transparency_alpha`,
//! `transparency_ignore_rules`, `komorebic transparency`).
//!
//! komorebi decides which windows are see-through, and the compositor draws them so: the opacity
//! of a window is a property of KWin's, which the backend sets. [`apply`] is called whenever
//! anything could have changed, and sends only what differs from what it sent before.
//!
//! The rules, on the workspace each monitor shows: the focused window of every container is
//! see-through unless the container has the focus on the focused monitor; a monocled window is
//! opaque, unless it is on a monitor that is not the focused one; a workspace that does not tile
//! has no transparent windows; ignored windows never are. These are the rules of komorebi for
//! Windows, where the settings come from.

use crate::REGEX_IDENTIFIERS;
use crate::TRANSPARENCY_BLACKLIST;
use crate::backend::backend;
use crate::window::Window;
use crate::window::should_act;
use crate::window_manager::WindowManager;
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

pub static TRANSPARENCY_ENABLED: AtomicBool = AtomicBool::new(false);
pub static TRANSPARENCY_ALPHA: AtomicU8 = AtomicU8::new(200);

/// The opacity of a window that is not see-through at all.
const OPAQUE: u8 = 255;

/// Returns true if and only if `transparency_ignore_rules` match the window. A window whose title,
/// executable, class or path cannot be read is not ignored.
fn ignored(window: Window) -> bool {
    let blacklist = TRANSPARENCY_BLACKLIST.lock();
    if blacklist.is_empty() {
        return false;
    }

    let (Some(title), Some(exe), Some(class), Some(path)) =
        (window.title(), window.exe(), window.class(), window.path())
    else {
        return false;
    };

    let regex_identifiers = REGEX_IDENTIFIERS.lock();
    should_act(
        &title,
        &exe,
        &[class.as_str()],
        &path.to_string_lossy(),
        &blacklist,
        &regex_identifiers,
    )
    .is_some()
}

/// Returns the windows that should be see-through now, by the rules in the [module docs](self),
/// whether transparency is on or not.
pub fn transparent_windows(wm: &WindowManager) -> Vec<u32> {
    let mut out = vec![];
    let focused_monitor_idx = wm.focused_monitor_idx();

    for (monitor_idx, monitor) in wm.monitors().iter().enumerate() {
        let on_focused_monitor = monitor_idx == focused_monitor_idx;

        // only what each monitor shows; the rest is not on screen
        let Some(workspace) = monitor.focused_workspace() else {
            continue;
        };

        if !workspace.tile {
            continue;
        }

        if let Some(monocle) = &workspace.monocle_container {
            if !on_focused_monitor
                && let Some(window) = monocle.focused_window()
                && !ignored(*window)
            {
                out.push(window.id);
            }

            continue;
        }

        for (idx, container) in workspace.containers().iter().enumerate() {
            if on_focused_monitor && idx == workspace.focused_container_idx() {
                continue;
            }

            if let Some(window) = container.focused_window()
                && !ignored(*window)
            {
                out.push(window.id);
            }
        }
    }

    out
}

/// Makes the windows as see-through as the settings and the state of the window manager say.
///
/// The backend is told only what differs from what it was last told, which the window manager
/// keeps in [`WindowManager::opacity_applied`]: windows that should not be see-through any more
/// are made opaque, and the others are given [`TRANSPARENCY_ALPHA`]. With transparency off, every
/// window is opaque. A failure is logged.
pub fn apply(wm: &mut WindowManager) {
    let alpha = TRANSPARENCY_ALPHA.load(Ordering::SeqCst);
    let wanted: HashSet<u32> = if TRANSPARENCY_ENABLED.load(Ordering::SeqCst) {
        transparent_windows(wm).into_iter().collect()
    } else {
        HashSet::new()
    };

    // opaque again: everything that was see-through and should not be any more
    let no_longer: Vec<u32> = wm
        .opacity_applied
        .keys()
        .filter(|id| !wanted.contains(id))
        .copied()
        .collect();
    for id in no_longer {
        if backend().is_window(id)
            && let Err(error) = backend().set_opacity(id, OPAQUE)
        {
            tracing::warn!("could not make window {id} opaque: {error}");
        }
        wm.opacity_applied.remove(&id);
    }

    for id in wanted {
        if wm.opacity_applied.get(&id) == Some(&alpha) {
            continue;
        }

        match backend().set_opacity(id, alpha) {
            Ok(()) => {
                wm.opacity_applied.insert(id, alpha);
            }
            Err(error) => tracing::warn!("could not make window {id} transparent: {error}"),
        }
    }
}

/// Makes every window komorebi made see-through opaque again, when komorebi exits.
pub fn restore(wm: &mut WindowManager) {
    for (id, _) in wm.opacity_applied.drain() {
        if backend().is_window(id) {
            let _ = backend().set_opacity(id, OPAQUE);
        }
    }
}
