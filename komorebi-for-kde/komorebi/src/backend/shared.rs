//! komorebi for KDE's backends, seen as the shared [`komorebi_backend::Backend`].
//!
//! [`Shared`] wraps any KDE [`Backend`](super::Backend) (the real KWin one or the fake) and
//! implements the trait that every komorebi shares, which is how KDE is its first implementation:
//! the daemon keeps calling its own trait (`u32` ids, `eyre` errors, the look, the wallpaper and
//! the drag frames) until the shared core is adopted, and everything the shared trait asks for is
//! answered by the KDE methods underneath. Nothing about how KDE behaves changes.
//!
//! What the translation does:
//!
//! - an id is a [`WindowId`] made from the `u32` ([`WindowId::from_u32`]); an id too wide for a
//!   `u32` is a window KDE never had;
//! - errors need no translation: KDE's trait already fails with the shared [`Error`];
//! - KDE fills every field of a window, so the optional fields are all `Some`;
//! - every [`Capability`] is supported: KDE has workspaces, maximize, opacity and focus follows
//!   mouse;
//! - a batch does nothing: KDE batches by construction (the answer to a held poll carries every
//!   command that was queued).

use super::Backend;
use komorebi_backend as shared;
use komorebi_backend::Capability;
use komorebi_backend::Error;
use komorebi_backend::MonitorId;
use komorebi_backend::Rect;
use komorebi_backend::WindowId;
use std::sync::Arc;

/// A KDE [`Backend`] as the shared [`komorebi_backend::Backend`].
pub struct Shared<B: Backend + ?Sized>(pub Arc<B>);

impl<B: Backend + ?Sized> Shared<B> {
    /// Returns the KDE backend underneath.
    pub fn kde(&self) -> &B {
        &self.0
    }

    /// Runs a window command through KDE's trait; an id too wide for KDE is a window it never had.
    fn window_command(
        &self,
        id: WindowId,
        command: impl FnOnce(&B, u32) -> shared::Result<()>,
    ) -> shared::Result<()> {
        let Some(raw) = id.to_u32() else {
            return Err(Error::WindowGone(id));
        };
        command(&self.0, raw)
    }
}

fn window_info(info: super::WindowInfo) -> shared::WindowInfo {
    shared::WindowInfo {
        id: WindowId::from_u32(info.id),
        pid: info.pid,
        rect: info.rect,
        minimized: info.minimized,
        alive: info.alive,
        title: Some(info.title),
        class: Some(info.class),
        exe: Some(info.exe),
        path: Some(info.path),
        fullscreen: Some(info.fullscreen),
        maximized: Some(info.maximized),
        normal: Some(info.normal),
    }
}

impl<B: Backend + ?Sized> shared::Backend for Shared<B> {
    fn windows(&self) -> Vec<shared::WindowInfo> {
        self.0.windows().into_iter().map(window_info).collect()
    }

    fn window(&self, id: WindowId) -> Option<shared::WindowInfo> {
        self.0.window(id.to_u32()?).map(window_info)
    }

    fn foreground_window(&self) -> Option<WindowId> {
        self.0.foreground_window().map(WindowId::from_u32)
    }

    fn monitors(&self) -> shared::Result<Vec<shared::MonitorInfo>> {
        Ok(self
            .0
            .monitors()?
            .into_iter()
            .map(|m| shared::MonitorInfo {
                id: MonitorId::from_u32(m.id),
                size: m.size,
                work_area: m.work_area,
                name: Some(m.name),
                serial_number_id: Some(m.serial_number_id),
            })
            .collect())
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        self.0.cursor_pos()
    }

    fn set_rect(&self, id: WindowId, rect: &Rect) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.set_rect(raw, rect))
    }

    fn focus(&self, id: WindowId, mouse_follows_focus: bool) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.focus(raw, mouse_follows_focus))
    }

    fn raise(&self, id: WindowId) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.raise(raw))
    }

    fn hide(&self, id: WindowId) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.hide(raw))
    }

    fn restore(&self, id: WindowId) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.restore(raw))
    }

    fn minimize(&self, id: WindowId) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.minimize(raw))
    }

    fn unminimize(&self, id: WindowId) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.unminimize(raw))
    }

    fn close(&self, id: WindowId) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.close(raw))
    }

    fn center_cursor_in(&self, rect: &Rect) -> shared::Result<()> {
        self.0.center_cursor_in(rect)
    }

    fn supports(&self, _capability: Capability) -> bool {
        true
    }

    fn maximize(&self, id: WindowId, maximize: bool) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.maximize(raw, maximize))
    }

    fn set_opacity(&self, id: WindowId, alpha: u8) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.set_opacity(raw, alpha))
    }

    fn set_focus_follows_mouse(&self, enable: bool) -> shared::Result<()> {
        self.0.set_focus_follows_mouse(enable)
    }

    fn ensure_workspaces(&self, names: &[String]) -> shared::Result<()> {
        self.0.ensure_workspaces(names)
    }

    fn switch_workspace(&self, monitor: MonitorId, idx: usize) -> shared::Result<()> {
        let id = monitor.to_u32().unwrap_or(u32::MAX);
        self.0.switch_workspace(id, idx)
    }

    fn set_window_workspace(&self, id: WindowId, idx: usize) -> shared::Result<()> {
        self.window_command(id, |b, raw| b.set_window_workspace(raw, idx))
    }

    fn window_workspace(&self, id: WindowId) -> shared::Result<Option<usize>> {
        Ok(id.to_u32().and_then(|raw| self.0.window_workspace(raw)))
    }

    fn current_workspaces(&self) -> shared::Result<Vec<(MonitorId, usize)>> {
        Ok(self
            .0
            .current_workspaces()
            .into_iter()
            .map(|(monitor, idx)| (MonitorId::from_u32(monitor), idx))
            .collect())
    }

    // `is_window`, `window_rect` and `monitor_from_point` are the provided ones, over `window` and
    // `monitors`, exactly as KDE's own are; `begin_batch` and `end_batch` do nothing, as KDE's
    // queue is flushed as a whole.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use komorebi_backend::Backend as _;
    use komorebi_backend::contract::Harness;
    use komorebi_backend::contract::check_all;

    struct KdeFake(Shared<FakeBackend>);

    impl Harness for KdeFake {
        type B = Shared<FakeBackend>;

        fn new() -> Self {
            Self(Shared(Arc::new(FakeBackend::with_one_monitor())))
        }

        fn backend(&self) -> &Self::B {
            &self.0
        }

        fn open_window(&self, seed: u32) -> WindowId {
            self.0.kde().open_window(seed, "foot", "foot", "a terminal");
            WindowId::from_u32(seed)
        }

        fn destroy_window(&self, id: WindowId) {
            self.0
                .kde()
                .close_window_now(id.to_u32().expect("a KDE id"));
        }
    }

    #[test]
    fn the_kde_fake_passes_the_shared_contract() {
        check_all::<KdeFake>();
    }

    #[test]
    fn kde_is_the_first_implementation_and_supports_every_capability() {
        let kde = KdeFake::new();

        for capability in Capability::ALL {
            assert!(kde.backend().supports(capability));
        }
    }

    #[test]
    fn a_command_about_a_window_that_is_gone_is_window_gone_and_other_failures_are_not() {
        let kde = KdeFake::new();
        let id = kde.open_window(4);

        let gone = kde.backend().focus(WindowId::from_u32(9), false);

        assert!(matches!(gone, Err(Error::WindowGone(WindowId(9)))));
        assert_eq!(gone.unwrap_err().to_string(), "no such window: 9");
        assert!(kde.backend().focus(id, false).is_ok());
    }

    #[test]
    fn an_id_too_wide_for_kde_is_a_window_kde_never_had() {
        let kde = KdeFake::new();
        let wide = WindowId(u64::from(u32::MAX) + 1);

        assert!(kde.backend().window(wide).is_none());
        assert!(kde.backend().hide(wide).unwrap_err().is_window_gone());
    }

    #[test]
    fn every_field_kde_knows_comes_through() {
        let kde = KdeFake::new();
        let id = kde.open_window(4);

        let info = kde.backend().window(id).expect("window 4 was opened");

        assert_eq!(info.id, id);
        assert_eq!(info.class.as_deref(), Some("foot"));
        assert_eq!(info.normal, Some(true));
        assert_eq!(info.fullscreen, Some(false));
        assert_eq!(
            kde.backend().monitors().expect("one monitor")[0]
                .name
                .as_deref(),
            Some("Virtual-0")
        );
    }
}
