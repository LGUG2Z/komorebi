//! `WindowsBackend`: the shared [`komorebi_backend::Backend`] over the existing static
//! [`WindowsApi`] and [`Window`] methods.
//!
//! **Nothing uses it yet.** The daemon still calls `WindowsApi` and `Window` directly; this
//! adapter exists so that the trait is known to be implementable here before the core depends on
//! it (step 7c of the consolidation). Every method that could not be written without changing
//! what the daemon does is listed in `docs/dev/seam.md`, section 9, and commented where it is.
//!
//! A window is named by its `HWND` ([`WindowId::from_hwnd`] keeps every bit), a monitor by its
//! `HMONITOR` ([`MonitorId::from_hmonitor`]). There are no workspaces here (komorebi's own are
//! made with hide and restore), so every [`Capability::Workspaces`] method answers `Unsupported`.

use crate::core::Rect;
use crate::window::Window;
use crate::windows_api::WindowsApi;
use color_eyre::eyre;
use komorebi_backend::Backend;
use komorebi_backend::Capability;
use komorebi_backend::Error;
use komorebi_backend::MonitorId;
use komorebi_backend::MonitorInfo;
use komorebi_backend::Result;
use komorebi_backend::WindowId;
use komorebi_backend::WindowInfo;
use windows::Win32::Foundation::POINT;

/// The shared [`Backend`] for Windows. It holds nothing: `WindowsApi` is stateless.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsBackend;

/// The window, if Windows still has it.
fn live(id: WindowId) -> Result<Window> {
    let hwnd = id.to_hwnd();
    if WindowsApi::is_window(hwnd) {
        Ok(Window::from(hwnd))
    } else {
        Err(Error::WindowGone(id))
    }
}

/// Says why a call about a window failed: the window died, or Windows refused.
fn failed(id: WindowId, report: eyre::Report) -> Error {
    if WindowsApi::is_window(id.to_hwnd()) {
        Error::Platform(report.into())
    } else {
        Error::WindowGone(id)
    }
}

fn platform(report: eyre::Report) -> Error {
    Error::Platform(report.into())
}

impl WindowsBackend {
    fn info(window: Window) -> Option<WindowInfo> {
        let hwnd = window.hwnd;
        if !window.alive() {
            return None;
        }

        Some(WindowInfo {
            id: WindowId::from_hwnd(hwnd),
            pid: i32::try_from(window.process_id()).unwrap_or_default(),
            // Windows reports the visible bounds (without the shadow); an error leaves the
            // default, which a caller cannot tell from a window at the origin: see seam.md 9
            rect: window.window_rect().unwrap_or_default(),
            minimized: window.is_miminized(),
            alive: true,
            title: window.title().ok(),
            class: window.class().ok(),
            exe: window.exe().ok(),
            path: window.path().ok(),
            // fullscreen has no source here: what komorebi calls it is decided in the core
            fullscreen: None,
            maximized: Some(window.is_maximized()),
            // "a normal application window" is decided by the core's rules on Windows
            normal: None,
        })
    }
}

impl Backend for WindowsBackend {
    fn windows(&self) -> Vec<WindowInfo> {
        // `alt_tab_windows` is the nearest Windows has to a candidate list: the windows that
        // `load_workspace_information` offers to the core are filtered inside a callback that
        // fills the containers of a `WindowManager`, which this adapter does not have
        let mut windows: Vec<_> = WindowsApi::alt_tab_windows()
            .unwrap_or_default()
            .into_iter()
            .filter_map(Self::info)
            .collect();
        windows.sort_by_key(|w| w.id);
        windows
    }

    fn window(&self, id: WindowId) -> Option<WindowInfo> {
        Self::info(Window::from(id.to_hwnd()))
    }

    fn foreground_window(&self) -> Option<WindowId> {
        WindowsApi::foreground_window()
            .ok()
            .map(WindowId::from_hwnd)
    }

    fn monitors(&self) -> Result<Vec<MonitorInfo>> {
        let monitors = WindowsApi::valid_hmonitors()
            .map_err(platform)?
            .into_iter()
            .map(|(_, hmonitor)| {
                let monitor = WindowsApi::monitor(hmonitor).map_err(platform)?;
                Ok(MonitorInfo {
                    id: MonitorId::from_hmonitor(monitor.id),
                    size: monitor.size,
                    work_area: monitor.work_area_size,
                    name: Some(monitor.name),
                    serial_number_id: monitor.serial_number_id,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        if monitors.is_empty() {
            return Err(Error::platform("Windows reports no monitors"));
        }

        Ok(monitors)
    }

    fn frame(&self, id: WindowId) -> Result<Rect> {
        // one `DwmGetWindowAttribute` (fallback `GetWindowRect`), no title, class or exe lookups,
        // and no `IsWindow` first: the Win32 error is the answer, as for every `window_rect`
        // caller; `failed` says whether the window died
        Window::from(id.to_hwnd())
            .window_rect()
            .map_err(|e| failed(id, e))
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        WindowsApi::cursor_pos().ok().map(|p| (p.x, p.y))
    }

    fn set_rect(&self, id: WindowId, rect: &Rect) -> Result<()> {
        let window = live(id)?;
        // `top` (raise to the front as well) is a Windows-only argument that the shared
        // contract does not have: it is false here and a caller raises with `raise`.
        // `Window::set_position` also keeps its animation, slide-in and live-resize guards
        // and skips the write when the frame is already right: see seam.md 9.
        window.set_rect(rect).map_err(|e| failed(id, e))
    }

    fn focus(&self, id: WindowId, mouse_follows_focus: bool) -> Result<()> {
        live(id)?
            .focus(mouse_follows_focus)
            .map_err(|e| failed(id, e))
    }

    fn raise(&self, id: WindowId) -> Result<()> {
        live(id)?.raise().map_err(|e| failed(id, e))
    }

    fn hide(&self, id: WindowId) -> Result<()> {
        // `Window::hide` cannot fail (its `ShowWindow` result is ignored) and also hides the
        // window's border and records it in HIDDEN_HWNDS: a dead window is told by `live` only
        live(id)?.hide();
        Ok(())
    }

    fn restore(&self, id: WindowId) -> Result<()> {
        live(id)?.restore();
        Ok(())
    }

    fn minimize(&self, id: WindowId) -> Result<()> {
        // `Window::minimize` leaves komorebi-bar alone and says nothing about it
        live(id)?.minimize();
        Ok(())
    }

    fn unminimize(&self, id: WindowId) -> Result<()> {
        // Windows has no unminimize of its own: this is `SW_SHOWNOACTIVATE`, as `restore` uses
        // for the `Hide` and `Minimize` hiding behaviours, and without the HIDDEN_HWNDS and
        // border bookkeeping of `Window::restore`
        live(id)?;
        Window::from(id.to_hwnd()).unminimize();
        Ok(())
    }

    fn close(&self, id: WindowId) -> Result<()> {
        live(id)?.close().map_err(|e| failed(id, e))
    }

    fn center_cursor_in(&self, rect: &Rect) -> Result<()> {
        WindowsApi::center_cursor_in_rect(rect).map_err(platform)
    }

    fn supports(&self, capability: Capability) -> bool {
        matches!(
            capability,
            Capability::Maximize | Capability::Opacity | Capability::FocusFollowsMouse
        )
    }

    fn maximize(&self, id: WindowId, maximize: bool) -> Result<()> {
        let window = live(id)?;
        if maximize {
            window.maximize();
        } else {
            window.unmaximize();
        }
        Ok(())
    }

    fn set_opacity(&self, id: WindowId, alpha: u8) -> Result<()> {
        use crate::styles::ExtendedWindowStyle;

        // `Window::transparent` and `opaque` take their alpha from the transparency manager's
        // global and may animate; this is the plain write they end in, with the LAYERED style
        // that it needs
        let window = live(id)?;
        let mut style = window.ex_style().map_err(|e| failed(id, e))?;
        style.insert(ExtendedWindowStyle::LAYERED);
        window.update_ex_style(&style).map_err(|e| failed(id, e))?;
        WindowsApi::set_transparent(id.to_hwnd(), alpha).map_err(|e| failed(id, e))
    }

    fn set_focus_follows_mouse(&self, enable: bool) -> Result<()> {
        // the system setting only; komorebi's own mouse-hook implementation
        // (process_movement.rs) is not started by this
        if enable {
            WindowsApi::enable_focus_follows_mouse()
        } else {
            WindowsApi::disable_focus_follows_mouse()
        }
        .map_err(platform)
    }

    fn monitor_from_point(&self, x: i32, y: i32) -> Option<MonitorId> {
        // MONITOR_DEFAULTTONEAREST: the nearest monitor, never `None`
        Some(MonitorId::from_hmonitor(WindowsApi::monitor_from_point(
            POINT { x, y },
        )))
    }
}
