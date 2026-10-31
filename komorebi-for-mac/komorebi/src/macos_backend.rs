//! `MacosBackend`: the shared [`komorebi_backend::Backend`] over the existing [`MacosApi`] and
//! [`Window`] methods (set_rect, focus, raise, hide, restore, minimize, unminimize, close, alive,
//! window_rect and minimized): it holds no Accessibility call of its own.
//!
//! **Nothing uses it yet.** The daemon still holds `Window` values (element, application and
//! observer) and calls them directly; this adapter exists so that the trait is known to be
//! implementable here before the core depends on it (step 7c of the consolidation). The parts
//! that could not be written without changing what the daemon does are listed in
//! `docs/dev/seam.md`, section 9, and commented where they are.
//!
//! The trait names a window by an id, and nothing on macOS acts from a bare id: a window is its
//! Accessibility element. So the backend looks windows up in the **id registry** of consequence
//! (1), `ax_registry`: the daemon
//! registers each `Window` it learns of ([`MacosBackend::register`]) and forgets it when it is
//! gone, and every method looks the element up by id. An id the registry does not hold is a
//! window that is gone.
//!
//! Monitors are the other thing that cannot be a plain call: `MacosApi::latest_monitor_information`
//! is an `NSScreen` query that is only legal on the main thread, and the rule is that no `Backend`
//! method waits for main. [`Backend::monitors`] therefore reads a cache, which the display
//! callback refreshes on main with [`MacosBackend::refresh_monitors`].

use crate::LibraryError;
use crate::core::Rect;
use crate::core::WindowHidingPosition;
use crate::macos_api::MacosApi;
use crate::skylight::BatchedUpdates;
use crate::window::Window;
use komorebi_backend::Backend;
use komorebi_backend::Error;
use komorebi_backend::MonitorId;
use komorebi_backend::MonitorInfo;
use komorebi_backend::Result;
use komorebi_backend::WindowId;
use komorebi_backend::WindowInfo;
use objc2::MainThreadMarker;
use parking_lot::Mutex;

/// The shared [`Backend`] for macOS.
///
/// Capabilities: none. macOS has no workspaces of komorebi's to delegate, no maximize, no
/// opacity and no focus follows mouse (`docs/dev/seam.md` section 2.1), so those methods answer
/// `Unsupported`.
pub struct MacosBackend {
    monitors: Mutex<Vec<MonitorInfo>>,
    hiding_position: WindowHidingPosition,
    batches: Mutex<Vec<BatchedUpdates>>,
}

impl Default for MacosBackend {
    fn default() -> Self {
        Self::new(WindowHidingPosition::default())
    }
}

fn platform(error: impl std::error::Error + Send + Sync + 'static) -> Error {
    Error::from_platform(error)
}

impl MacosBackend {
    /// Returns a backend with an empty registry and no monitors. `hiding_position` is the corner
    /// [`Backend::hide`] parks windows in: on macOS it is an argument of every hide, taken from
    /// the monitor's configuration, and the trait has no place for it.
    pub fn new(hiding_position: WindowHidingPosition) -> Self {
        Self {
            monitors: Mutex::new(Vec::new()),
            hiding_position,
            batches: Mutex::new(Vec::new()),
        }
    }

    /// Makes the window known to the backend, by its `CGWindowID`.
    pub fn register(&self, window: Window) {
        crate::ax_registry::register(&window);
    }

    /// Forgets a window that is gone.
    pub fn forget(&self, id: u32) {
        crate::ax_registry::forget(id);
    }

    /// Reads the monitors from `NSScreen` into the cache that [`Backend::monitors`] answers
    /// from. The `MainThreadMarker` is the proof that this is called on the main thread, from
    /// the display reconfiguration callback.
    pub fn refresh_monitors(&self, _main: MainThreadMarker) -> Result<()> {
        let monitors = MacosApi::latest_monitor_information()
            .map_err(platform)?
            .into_iter()
            .map(|m| MonitorInfo {
                id: MonitorId::from_u32(m.id),
                size: m.size,
                work_area: m.work_area_size,
                name: Some(m.device),
                serial_number_id: Some(m.serial_number_id),
            })
            .collect();

        *self.monitors.lock() = monitors;
        Ok(())
    }

    fn lookup(&self, id: WindowId) -> Result<Window> {
        crate::ax_registry::window(id)
    }

    /// Says why a call about a window failed. A failed write is how macOS finds out that a window
    /// has died, so the window is asked whether it is still there (an Accessibility round trip,
    /// as the reaper does after a failed write).
    fn failed(
        id: WindowId,
        window: &Window,
        error: impl std::error::Error + Send + Sync + 'static,
    ) -> Error {
        if window.alive() {
            platform(error)
        } else {
            Error::WindowGone(id)
        }
    }

    fn info(window: &Window) -> Option<WindowInfo> {
        if !window.alive() {
            return None;
        }

        Some(WindowInfo {
            id: WindowId::from_u32(window.id),
            pid: window.application.process_id,
            // the Accessibility position and size, in points; a failure leaves the default
            rect: window.window_rect().map(Rect::from).unwrap_or_default(),
            minimized: window.minimized(),
            alive: true,
            title: window.title(),
            // macOS has no window class: a bundle identifier, role and subrole instead
            class: None,
            exe: window.exe(),
            path: window.path().map(|p| p.to_string_lossy().into_owned()),
            fullscreen: None,
            maximized: None,
            normal: None,
        })
    }
}

impl Backend for MacosBackend {
    fn windows(&self) -> Vec<WindowInfo> {
        // The registry is the only list there is: the CoreGraphics list joined with the
        // Accessibility elements by title lives inside `load_workspace_information`, which fills
        // a `WindowManager`, and the daemon registers what it finds
        let registered = crate::ax_registry::windows();
        let mut windows: Vec<_> = registered.iter().filter_map(Self::info).collect();
        windows.sort_by_key(|w| w.id);
        windows
    }

    fn window(&self, id: WindowId) -> Option<WindowInfo> {
        Self::info(&self.lookup(id).ok()?)
    }

    fn foreground_window(&self) -> Option<WindowId> {
        MacosApi::foreground_window_id().map(WindowId::from_u32)
    }

    fn monitors(&self) -> Result<Vec<MonitorInfo>> {
        let monitors = self.monitors.lock().clone();
        if monitors.is_empty() {
            return Err(Error::platform(
                "no monitors known yet: the display callback has not refreshed the cache",
            ));
        }

        Ok(monitors)
    }

    fn frame(&self, id: WindowId) -> Result<Rect> {
        // the Accessibility position and size only, without the title, executable and path
        // lookups of `window`
        let window = self.lookup(id)?;
        MacosApi::window_rect(&window.element)
            .map(Rect::from)
            .map_err(|e| Self::failed(id, &window, e))
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        let point = MacosApi::cursor_pos();
        #[allow(clippy::cast_possible_truncation)]
        Some((point.x as i32, point.y as i32))
    }

    fn set_rect(&self, id: WindowId, rect: &Rect) -> Result<()> {
        // `Window::set_rect` writes directly (two Accessibility writes) or hands the move to
        // the animator, depending on the animation settings; both are requests
        let window = self.lookup(id)?;
        window
            .set_rect(rect)
            .map_err(|e| Self::failed(id, &window, e))
    }

    fn focus(&self, id: WindowId, mouse_follows_focus: bool) -> Result<()> {
        let window = self.lookup(id)?;
        window
            .focus(mouse_follows_focus)
            .map_err(|e| Self::failed(id, &window, e))
    }

    fn raise(&self, id: WindowId) -> Result<()> {
        // macOS has no raise without focus: this sets AXMain and AXFocused, so it focuses
        let window = self.lookup(id)?;
        window.raise().map_err(|e| Self::failed(id, &window, e))
    }

    fn hide(&self, id: WindowId) -> Result<()> {
        // a sliver in a corner of the display, observable by the app and the user
        let mut window = self.lookup(id)?;
        window
            .hide(self.hiding_position)
            .map_err(|e| Self::failed(id, &window, e))
    }

    fn restore(&self, id: WindowId) -> Result<()> {
        let mut window = self.lookup(id)?;
        window.restore().map_err(|e| Self::failed(id, &window, e))
    }

    fn minimize(&self, id: WindowId) -> Result<()> {
        let mut window = self.lookup(id)?;
        window.minimize().map_err(|e| Self::failed(id, &window, e))
    }

    fn unminimize(&self, id: WindowId) -> Result<()> {
        let mut window = self.lookup(id)?;
        window
            .unminimize()
            .map_err(|e| Self::failed(id, &window, e))
    }

    fn close(&self, id: WindowId) -> Result<()> {
        // presses the close button; `Ok` when the window has none
        let window = self.lookup(id)?;
        window.close().map_err(|e| Self::failed(id, &window, e))
    }

    fn center_cursor_in(&self, rect: &Rect) -> Result<()> {
        MacosApi::center_cursor_in_rect(rect).map_err(|e: LibraryError| platform(e))
    }

    fn monitor_from_point(&self, x: i32, y: i32) -> Option<MonitorId> {
        MacosApi::monitor_from_point(objc2_core_foundation::CGPoint::new(
            f64::from(x),
            f64::from(y),
        ))
        .map(MonitorId::from_u32)
    }

    fn begin_batch(&self) {
        // SkyLight holds back everything that changes on screen until the guard is dropped
        self.batches.lock().push(BatchedUpdates::begin());
    }

    fn end_batch(&self) {
        drop(self.batches.lock().pop());
    }
}
