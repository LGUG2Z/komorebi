use color_eyre::eyre;
use crossbeam_channel::Sender;
use crossbeam_channel::bounded;
use crossbeam_channel::unbounded;
use std::sync::OnceLock;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Dwm::DWM_THUMBNAIL_PROPERTIES;
use windows::Win32::Graphics::Dwm::DWM_TNP_OPACITY;
use windows::Win32::Graphics::Dwm::DWM_TNP_RECTDESTINATION;
use windows::Win32::Graphics::Dwm::DWM_TNP_RECTSOURCE;
use windows::Win32::Graphics::Dwm::DWM_TNP_SOURCECLIENTAREAONLY;
use windows::Win32::Graphics::Dwm::DWM_TNP_VISIBLE;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::UI::WindowsAndMessaging::BeginDeferWindowPos;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DeferWindowPos;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::EndDeferWindowPos;
use windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE;
use windows::Win32::UI::WindowsAndMessaging::GetShellWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::HWND_TOP;
use windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PM_REMOVE;
use windows::Win32::UI::WindowsAndMessaging::PeekMessageW;
use windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS;
use windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOREDRAW;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
use windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW;
use windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED;
use windows::core::PCWSTR;

use crate::WindowsApi;
use crate::core::Rect;
use crate::windows_api;

const GHOST_CLASS_NAME: &[u16] = &[
    b'k' as u16,
    b'o' as u16,
    b'm' as u16,
    b'o' as u16,
    b'r' as u16,
    b'e' as u16,
    b'b' as u16,
    b'i' as u16,
    b'-' as u16,
    b'g' as u16,
    b'h' as u16,
    b'o' as u16,
    b's' as u16,
    b't' as u16,
    0,
];

enum GhostCmd {
    Create {
        src_hwnd: isize,
        start_rect: Rect,
        z_above: Option<isize>,
        visible: bool,
        reply: Sender<eyre::Result<(isize, isize)>>,
    },
    UpdateRect {
        host_hwnd: isize,
        hthumb: isize,
        rect: Rect,
    },
    /// Position the ghost on `host` and sample only `source_*` of the source window.
    /// `host: None` hides it, which is how a workspace slide stays on one monitor.
    UpdateClip {
        host_hwnd: isize,
        hthumb: isize,
        host: Option<Rect>,
        source_x: i32,
        source_y: i32,
        source_width: i32,
        source_height: i32,
    },
    Destroy {
        host_hwnd: isize,
        hthumb: isize,
    },
    /// Apply every clip, then wait for composition, before replying.
    /// One command keeps a workspace-slide frame on a single vblank.
    Present {
        clips: Vec<GhostClip>,
        flush: bool,
        reply: Sender<()>,
    },
    CreateOverlay {
        src_hwnd: isize,
        clip: GhostClip,
        reply: Sender<eyre::Result<(isize, isize, isize)>>,
    },
    DestroyOverlay {
        host_hwnd: isize,
        hdesktop: isize,
        hthumb: isize,
    },
}

/// One ghost's crop for a single slide frame.
pub struct GhostClip {
    host_hwnd: isize,
    hthumb: isize,
    host: Option<Rect>,
    source_x: i32,
    source_y: i32,
    source_width: i32,
    source_height: i32,
}

struct GhostOwner {
    cmd_tx: Sender<GhostCmd>,
}

static GHOST_OWNER: OnceLock<GhostOwner> = OnceLock::new();

fn ghost_owner() -> &'static GhostOwner {
    GHOST_OWNER.get_or_init(|| {
        let (tx, rx) = unbounded::<GhostCmd>();
        std::thread::Builder::new()
            .name("komorebi-ghost-owner".into())
            .spawn(move || run_owner_loop(rx))
            .expect("failed to spawn ghost owner thread");
        GhostOwner { cmd_tx: tx }
    })
}

/// Eagerly initialise the ghost owner thread so the first movement animation
/// doesn't pay the spawn + class-registration cost. Idempotent. No-op for
/// users who never enable ghost movement only if it isn't called; calling
/// from a code path that's gated on `GHOST_MOVEMENT_ENABLED` keeps the lazy
/// guarantee.
pub fn prewarm() {
    let _ = ghost_owner();
}

extern "system" fn ghost_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn register_ghost_class() -> eyre::Result<()> {
    let h_module = WindowsApi::module_handle_w()?;
    let class_name = PCWSTR(GHOST_CLASS_NAME.as_ptr());
    let window_class = WNDCLASSW {
        hInstance: h_module.into(),
        lpszClassName: class_name,
        lpfnWndProc: Some(ghost_wnd_proc),
        ..Default::default()
    };
    // RegisterClassW returns 0 on failure with ERROR_CLASS_ALREADY_EXISTS as a
    // benign error if the class is already registered. We tolerate that.
    let _ = WindowsApi::register_class_w(&window_class);
    Ok(())
}

fn run_owner_loop(cmd_rx: crossbeam_channel::Receiver<GhostCmd>) {
    if let Err(error) = register_ghost_class() {
        tracing::error!("ghost owner: failed to register class: {error}");
        return;
    }

    loop {
        // Drain any pending Win32 messages (DWM/system messages destined for our hosts).
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        match cmd_rx.recv_timeout(Duration::from_millis(8)) {
            Ok(cmd) => handle_cmd(cmd),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn handle_cmd(cmd: GhostCmd) {
    match cmd {
        GhostCmd::Create {
            src_hwnd,
            start_rect,
            z_above,
            visible,
            reply,
        } => {
            let result = create_ghost(src_hwnd, start_rect, z_above, visible);
            let _ = reply.send(result);
        }
        GhostCmd::UpdateRect {
            host_hwnd,
            hthumb,
            rect,
        } => {
            if let Err(error) = update_ghost(host_hwnd, hthumb, rect) {
                tracing::trace!("ghost owner: update failed: {error}");
            }
        }
        GhostCmd::UpdateClip {
            host_hwnd,
            hthumb,
            host,
            source_x,
            source_y,
            source_width,
            source_height,
        } => {
            if let Err(error) = update_ghost_clip(
                host_hwnd,
                hthumb,
                host,
                source_x,
                source_y,
                source_width,
                source_height,
            ) {
                tracing::trace!("ghost owner: clip update failed: {error}");
            }
        }
        GhostCmd::Destroy { host_hwnd, hthumb } => {
            destroy_ghost(host_hwnd, hthumb);
        }
        GhostCmd::Present {
            clips,
            flush,
            reply,
        } => {
            present_frame(&clips);
            if flush {
                unsafe {
                    let _ = DwmFlush();
                }
            }
            let _ = reply.send(());
        }
        GhostCmd::CreateOverlay {
            src_hwnd,
            clip,
            reply,
        } => {
            let _ = reply.send(create_overlay(src_hwnd, &clip));
        }
        GhostCmd::DestroyOverlay {
            host_hwnd,
            hdesktop,
            hthumb,
        } => {
            let _ = WindowsApi::dwm_unregister_thumbnail(hdesktop);
            destroy_ghost(host_hwnd, hthumb);
        }
    }
}

/// Move every ghost for this frame, then block until DWM has composed it.
pub fn present_clips(clips: Vec<GhostClip>) -> eyre::Result<()> {
    send_clips(clips, true)
}

/// Move every ghost without waiting for composition, so the caller can make its
/// own changes land in the same frame.
pub fn show_clips(clips: Vec<GhostClip>) -> eyre::Result<()> {
    send_clips(clips, false)
}

fn send_clips(clips: Vec<GhostClip>, flush: bool) -> eyre::Result<()> {
    if clips.is_empty() {
        return Ok(());
    }
    let (reply_tx, reply_rx) = bounded::<()>(1);
    ghost_owner()
        .cmd_tx
        .send(GhostCmd::Present {
            clips,
            flush,
            reply: reply_tx,
        })
        .map_err(|e| eyre::eyre!("ghost owner channel send failed: {e}"))?;
    reply_rx
        .recv()
        .map_err(|e| eyre::eyre!("ghost present reply failed: {e}"))
}

fn instance_handle() -> eyre::Result<isize> {
    let h_module = WindowsApi::module_handle_w()?;
    Ok(h_module.0 as isize)
}

fn create_ghost(
    src_hwnd: isize,
    start_rect: Rect,
    z_above: Option<isize>,
    visible: bool,
) -> eyre::Result<(isize, isize)> {
    let class_name = PCWSTR(GHOST_CLASS_NAME.as_ptr());
    let host_hwnd = WindowsApi::create_ghost_host_window(class_name, instance_handle()?)?;

    // Position the host at start_rect (Rect uses left/top + width/height).
    let z_after = match z_above {
        Some(hwnd) => HWND(windows_api::as_ptr!(hwnd)),
        None => HWND_TOP,
    };
    let flags = if visible {
        SWP_NOACTIVATE | SWP_NOREDRAW | SWP_SHOWWINDOW
    } else {
        SWP_NOACTIVATE | SWP_NOREDRAW
    };
    unsafe {
        let _ = SetWindowPos(
            HWND(windows_api::as_ptr!(host_hwnd)),
            Option::from(z_after),
            start_rect.left,
            start_rect.top,
            start_rect.right,
            start_rect.bottom,
            flags,
        );
    }

    let hthumb = match WindowsApi::dwm_register_thumbnail(host_hwnd, src_hwnd) {
        Ok(h) => h,
        Err(error) => {
            unsafe {
                let _ = DestroyWindow(HWND(windows_api::as_ptr!(host_hwnd)));
            }
            return Err(error);
        }
    };

    let props = thumbnail_properties(start_rect.right, start_rect.bottom);
    if let Err(error) = WindowsApi::dwm_update_thumbnail_properties(hthumb, &props) {
        let _ = WindowsApi::dwm_unregister_thumbnail(hthumb);
        unsafe {
            let _ = DestroyWindow(HWND(windows_api::as_ptr!(host_hwnd)));
        }
        return Err(error);
    }

    // Make the host visible. Layered/transparent ext styles ensure no input.
    unsafe {
        let _ = ShowWindow(
            HWND(windows_api::as_ptr!(host_hwnd)),
            SHOW_WINDOW_CMD(if visible { 8 } else { 0 }),
        );
    }

    Ok((host_hwnd, hthumb))
}

/// Move every visible host in one `DeferWindowPos` batch, so a frame with many
/// windows costs one window-manager pass. Falls back to one move per host if the
/// batch cannot be built.
fn present_frame(clips: &[GhostClip]) {
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW | SWP_SHOWWINDOW;
    let visible = clips.iter().filter(|clip| clip.host.is_some()).count();
    let mut batch = i32::try_from(visible)
        .ok()
        .and_then(|count| unsafe { BeginDeferWindowPos(count) }.ok());

    for clip in clips {
        let Some(host) = clip.host else {
            unsafe {
                let _ = ShowWindow(
                    HWND(windows_api::as_ptr!(clip.host_hwnd)),
                    SHOW_WINDOW_CMD(0),
                );
            }
            continue;
        };

        let Some(hdwp) = batch else {
            if let Err(error) = update_ghost_clip(
                clip.host_hwnd,
                clip.hthumb,
                clip.host,
                clip.source_x,
                clip.source_y,
                clip.source_width,
                clip.source_height,
            ) {
                tracing::trace!("ghost owner: clip update failed: {error}");
            }
            continue;
        };

        // A failed DeferWindowPos frees the batch; the remaining hosts move one by one.
        batch = unsafe {
            DeferWindowPos(
                hdwp,
                HWND(windows_api::as_ptr!(clip.host_hwnd)),
                None,
                host.left,
                host.top,
                host.right,
                host.bottom,
                flags,
            )
        }
        .ok();
        if batch.is_none() {
            if let Err(error) = update_ghost_clip(
                clip.host_hwnd,
                clip.hthumb,
                clip.host,
                clip.source_x,
                clip.source_y,
                clip.source_width,
                clip.source_height,
            ) {
                tracing::trace!("ghost owner: clip update failed: {error}");
            }
            continue;
        }

        let props = clip_properties(
            host,
            clip.source_x,
            clip.source_y,
            clip.source_width,
            clip.source_height,
        );
        if let Err(error) = WindowsApi::dwm_update_thumbnail_properties(clip.hthumb, &props) {
            tracing::trace!("ghost owner: thumbnail update failed: {error}");
        }
    }

    if let Some(hdwp) = batch {
        unsafe {
            let _ = EndDeferWindowPos(hdwp);
        }
    }
}

fn clip_properties(
    host: Rect,
    source_x: i32,
    source_y: i32,
    source_width: i32,
    source_height: i32,
) -> DWM_THUMBNAIL_PROPERTIES {
    DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_VISIBLE
            | DWM_TNP_RECTDESTINATION
            | DWM_TNP_RECTSOURCE
            | DWM_TNP_OPACITY
            | DWM_TNP_SOURCECLIENTAREAONLY,
        rcDestination: RECT {
            left: 0,
            top: 0,
            right: host.right,
            bottom: host.bottom,
        },
        rcSource: RECT {
            left: source_x,
            top: source_y,
            right: source_x + source_width,
            bottom: source_y + source_height,
        },
        opacity: 255,
        fVisible: true.into(),
        fSourceClientAreaOnly: false.into(),
    }
}

fn update_ghost_clip(
    host_hwnd: isize,
    hthumb: isize,
    host: Option<Rect>,
    source_x: i32,
    source_y: i32,
    source_width: i32,
    source_height: i32,
) -> eyre::Result<()> {
    let Some(host) = host else {
        unsafe {
            let _ = ShowWindow(HWND(windows_api::as_ptr!(host_hwnd)), SHOW_WINDOW_CMD(0));
        }
        return Ok(());
    };

    let flags: SET_WINDOW_POS_FLAGS = SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW | SWP_SHOWWINDOW;
    unsafe {
        SetWindowPos(
            HWND(windows_api::as_ptr!(host_hwnd)),
            None,
            host.left,
            host.top,
            host.right,
            host.bottom,
            flags,
        )?;
    }

    let props = clip_properties(host, source_x, source_y, source_width, source_height);
    WindowsApi::dwm_update_thumbnail_properties(hthumb, &props)
}

fn update_ghost(host_hwnd: isize, hthumb: isize, rect: Rect) -> eyre::Result<()> {
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW;
    unsafe {
        SetWindowPos(
            HWND(windows_api::as_ptr!(host_hwnd)),
            None,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            flags,
        )?;
    }

    let props = thumbnail_properties(rect.right, rect.bottom);
    WindowsApi::dwm_update_thumbnail_properties(hthumb, &props)
}

/// A topmost, layered host drawing the desktop under `clip.host` with the source
/// window over it: what its ghost looked like, but opaque, so fading the host
/// crossfades to whatever the real window shows beneath.
fn create_overlay(src_hwnd: isize, clip: &GhostClip) -> eyre::Result<(isize, isize, isize)> {
    let Some(host) = clip.host else {
        eyre::bail!("overlay clip is off screen");
    };
    let desktop = unsafe { GetShellWindow() };
    if desktop.is_invalid() {
        eyre::bail!("no shell window");
    }
    let desktop_rect = WindowsApi::outer_window_rect(desktop.0 as isize)?;

    let class_name = PCWSTR(GHOST_CLASS_NAME.as_ptr());
    let host_hwnd = WindowsApi::create_ghost_host_window(class_name, instance_handle()?)?;
    let hwnd = HWND(windows_api::as_ptr!(host_hwnd));
    let destroy = |thumbs: &[isize]| {
        for thumb in thumbs {
            let _ = WindowsApi::dwm_unregister_thumbnail(*thumb);
        }
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    };

    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as isize);
    }
    if let Err(error) = WindowsApi::set_transparent(host_hwnd, 255) {
        destroy(&[]);
        return Err(error);
    }
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Option::from(HWND_TOPMOST),
            host.left,
            host.top,
            host.right,
            host.bottom,
            SWP_NOACTIVATE | SWP_NOREDRAW,
        );
    }

    let hdesktop = match WindowsApi::dwm_register_thumbnail(host_hwnd, desktop.0 as isize) {
        Ok(thumb) => thumb,
        Err(error) => {
            destroy(&[]);
            return Err(error);
        }
    };
    let desktop_props = clip_properties(
        host,
        host.left - desktop_rect.left,
        host.top - desktop_rect.top,
        host.right,
        host.bottom,
    );
    if let Err(error) = WindowsApi::dwm_update_thumbnail_properties(hdesktop, &desktop_props) {
        destroy(&[hdesktop]);
        return Err(error);
    }

    // Registered second, so it draws over the desktop.
    let hthumb = match WindowsApi::dwm_register_thumbnail(host_hwnd, src_hwnd) {
        Ok(thumb) => thumb,
        Err(error) => {
            destroy(&[hdesktop]);
            return Err(error);
        }
    };
    let props = clip_properties(
        host,
        clip.source_x,
        clip.source_y,
        clip.source_width,
        clip.source_height,
    );
    if let Err(error) = WindowsApi::dwm_update_thumbnail_properties(hthumb, &props) {
        destroy(&[hdesktop, hthumb]);
        return Err(error);
    }

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Ok((host_hwnd, hdesktop, hthumb))
}

fn destroy_ghost(host_hwnd: isize, hthumb: isize) {
    let _ = WindowsApi::dwm_unregister_thumbnail(hthumb);
    unsafe {
        let _ = DestroyWindow(HWND(windows_api::as_ptr!(host_hwnd)));
    }
}

fn thumbnail_properties(width: i32, height: i32) -> DWM_THUMBNAIL_PROPERTIES {
    DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_VISIBLE
            | DWM_TNP_RECTDESTINATION
            | DWM_TNP_OPACITY
            | DWM_TNP_SOURCECLIENTAREAONLY,
        rcDestination: RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
        rcSource: RECT::default(),
        opacity: 255,
        fVisible: true.into(),
        fSourceClientAreaOnly: false.into(),
    }
}

/// A live DWM-thumbnail "ghost" of a source window, used during movement
/// animations. While a ghost is active, the source window is typically cloaked
/// by the caller. The ghost is automatically disposed on drop, but callers
/// should prefer explicit `dispose()` to surface errors.
pub struct GhostWindow {
    host_hwnd: isize,
    hthumb: isize,
    disposed: bool,
}

impl GhostWindow {
    pub fn create(src_hwnd: isize, start_rect: Rect, z_above: Option<isize>) -> eyre::Result<Self> {
        Self::create_with_visibility(src_hwnd, start_rect, z_above, true)
    }

    pub fn create_with_visibility(
        src_hwnd: isize,
        start_rect: Rect,
        z_above: Option<isize>,
        visible: bool,
    ) -> eyre::Result<Self> {
        let (reply_tx, reply_rx) = bounded::<eyre::Result<(isize, isize)>>(1);
        ghost_owner()
            .cmd_tx
            .send(GhostCmd::Create {
                src_hwnd,
                start_rect,
                z_above,
                visible,
                reply: reply_tx,
            })
            .map_err(|e| eyre::eyre!("ghost owner channel send failed: {e}"))?;
        let (host_hwnd, hthumb) = reply_rx.recv()??;
        Ok(Self {
            host_hwnd,
            hthumb,
            disposed: false,
        })
    }

    pub fn host_hwnd(&self) -> isize {
        self.host_hwnd
    }

    pub fn clip(
        &self,
        host: Option<Rect>,
        source_x: i32,
        source_y: i32,
        source_width: i32,
        source_height: i32,
    ) -> GhostClip {
        GhostClip {
            host_hwnd: self.host_hwnd,
            hthumb: self.hthumb,
            host,
            source_x,
            source_y,
            source_width,
            source_height,
        }
    }

    /// Show only the portion of the source that lies on `host`. `None` hides the ghost.
    pub fn update_clip(
        &self,
        host: Option<Rect>,
        source_x: i32,
        source_y: i32,
        source_width: i32,
        source_height: i32,
    ) -> eyre::Result<()> {
        ghost_owner()
            .cmd_tx
            .send(GhostCmd::UpdateClip {
                host_hwnd: self.host_hwnd,
                hthumb: self.hthumb,
                host,
                source_x,
                source_y,
                source_width,
                source_height,
            })
            .map_err(|e| eyre::eyre!("ghost owner channel send failed: {e}"))
    }

    pub fn update_rect(&self, rect: Rect) -> eyre::Result<()> {
        ghost_owner()
            .cmd_tx
            .send(GhostCmd::UpdateRect {
                host_hwnd: self.host_hwnd,
                hthumb: self.hthumb,
                rect,
            })
            .map_err(|e| eyre::eyre!("ghost owner channel send failed: {e}"))
    }

    /// Apply an opacity change directly via `DwmUpdateThumbnailProperties` on
    /// the calling thread. Unlike rect updates (which call `SetWindowPos` and
    /// therefore need the owner thread), opacity-only updates don't have
    /// thread affinity, and going through the channel introduces a race where
    /// the next `DwmFlush()` on the caller's thread can fire before the owner
    /// has processed the SetOpacity command — which collapses what should be
    /// a multi-frame fade into a single visible step.
    pub fn set_opacity(&self, opacity: u8) -> eyre::Result<()> {
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_OPACITY | DWM_TNP_VISIBLE,
            rcDestination: RECT::default(),
            rcSource: RECT::default(),
            opacity,
            fVisible: true.into(),
            fSourceClientAreaOnly: false.into(),
        };
        WindowsApi::dwm_update_thumbnail_properties(self.hthumb, &props)
    }

    pub fn dispose(mut self) -> eyre::Result<()> {
        self.dispose_inner()
    }

    fn dispose_inner(&mut self) -> eyre::Result<()> {
        if self.disposed {
            return Ok(());
        }
        self.disposed = true;
        ghost_owner()
            .cmd_tx
            .send(GhostCmd::Destroy {
                host_hwnd: self.host_hwnd,
                hthumb: self.hthumb,
            })
            .map_err(|e| eyre::eyre!("ghost owner channel send failed: {e}"))
    }
}

impl Drop for GhostWindow {
    fn drop(&mut self) {
        let _ = self.dispose_inner();
    }
}

/// An opaque copy of a ghost over the desktop, laid over its revealed window and
/// faded out as a whole.
pub struct RevealOverlay {
    host_hwnd: isize,
    hdesktop: isize,
    hthumb: isize,
    disposed: bool,
}

impl RevealOverlay {
    pub fn create(src_hwnd: isize, clip: GhostClip) -> eyre::Result<Self> {
        let (reply_tx, reply_rx) = bounded::<eyre::Result<(isize, isize, isize)>>(1);
        ghost_owner()
            .cmd_tx
            .send(GhostCmd::CreateOverlay {
                src_hwnd,
                clip,
                reply: reply_tx,
            })
            .map_err(|e| eyre::eyre!("ghost owner channel send failed: {e}"))?;
        let (host_hwnd, hdesktop, hthumb) = reply_rx.recv()??;
        Ok(Self {
            host_hwnd,
            hdesktop,
            hthumb,
            disposed: false,
        })
    }

    /// Layered alpha applies to the thumbnails too, so this fades the whole copy.
    pub fn set_alpha(&self, alpha: u8) -> eyre::Result<()> {
        WindowsApi::set_transparent(self.host_hwnd, alpha)
    }

    pub fn dispose(mut self) {
        self.dispose_inner();
    }

    fn dispose_inner(&mut self) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        let _ = ghost_owner().cmd_tx.send(GhostCmd::DestroyOverlay {
            host_hwnd: self.host_hwnd,
            hdesktop: self.hdesktop,
            hthumb: self.hthumb,
        });
    }
}

impl Drop for RevealOverlay {
    fn drop(&mut self) {
        self.dispose_inner();
    }
}
