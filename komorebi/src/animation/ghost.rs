//! Ghost windows: thumbnails of a picture of a window that is being animated.
//!
//! While a movement animation runs, the real window stays cloaked and still,
//! and a thumbnail of it is moved and scaled instead. Every thumbnail is
//! composed onto one shared host window that covers the whole virtual screen,
//! so that animating a ghost costs a single `DwmUpdateThumbnailProperties` call
//! per frame: no window has to be moved, and all the ghosts of an animation
//! can be committed to the compositor together.
//!
//! The host is created and driven by a dedicated owner thread, because a
//! window has to be created by a thread that pumps its messages. Everything
//! that touches the host's z-order or the number of live thumbnails goes
//! through that thread. Updating a registered thumbnail has no thread
//! affinity and is done directly by the animating thread.

use color_eyre::eyre;
use crossbeam_channel::Sender;
use crossbeam_channel::bounded;
use crossbeam_channel::unbounded;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Dwm::DWM_THUMBNAIL_PROPERTIES;
use windows::Win32::Graphics::Dwm::DWM_TNP_OPACITY;
use windows::Win32::Graphics::Dwm::DWM_TNP_RECTDESTINATION;
use windows::Win32::Graphics::Dwm::DWM_TNP_RECTSOURCE;
use windows::Win32::Graphics::Dwm::DWM_TNP_VISIBLE;
use windows::Win32::Graphics::Gdi::AC_SRC_ALPHA;
use windows::Win32::Graphics::Gdi::AC_SRC_OVER;
use windows::Win32::Graphics::Gdi::BI_RGB;
use windows::Win32::Graphics::Gdi::BITMAPINFO;
use windows::Win32::Graphics::Gdi::BITMAPINFOHEADER;
use windows::Win32::Graphics::Gdi::BLENDFUNCTION;
use windows::Win32::Graphics::Gdi::CreateCompatibleDC;
use windows::Win32::Graphics::Gdi::CreateDIBSection;
use windows::Win32::Graphics::Gdi::DIB_RGB_COLORS;
use windows::Win32::Graphics::Gdi::DeleteDC;
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::Graphics::Gdi::GetDC;
use windows::Win32::Graphics::Gdi::ReleaseDC;
use windows::Win32::Graphics::Gdi::SelectObject;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
use windows::Win32::UI::WindowsAndMessaging::HWND_NOTOPMOST;
use windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PM_REMOVE;
use windows::Win32::UI::WindowsAndMessaging::PeekMessageW;
use windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN;
use windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN;
use windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN;
use windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::ULW_ALPHA;
use windows::Win32::UI::WindowsAndMessaging::UpdateLayeredWindow;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TRANSPARENT;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE;
use windows::core::PCWSTR;

use crate::WindowsApi;
use crate::animation::snapshot::Snapshot;
use crate::animation::snapshot::SnapshotWindow;
use crate::animation::transition::clip_thumbnail;
use crate::core::Rect;

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

/// Thumbnails currently registered against the host. Returns to zero when no
/// ghost is alive; anything else is a leak of compositor handles.
static REGISTERED_THUMBNAILS: AtomicI64 = AtomicI64::new(0);

/// How many ghost thumbnails are currently registered with DWM.
pub fn registered_thumbnails() -> i64 {
    REGISTERED_THUMBNAILS.load(Ordering::SeqCst)
}

enum GhostCmd {
    Create {
        snapshot: Snapshot,
        start_rect: Rect,
        clip: Option<Rect>,
        reply: Sender<eyre::Result<(isize, (i32, i32))>>,
    },
    Destroy {
        hthumb: isize,
    },
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
/// doesn't pay the spawn + class-registration + host window creation cost.
/// Idempotent. Calling this only from a code path that is gated on
/// `GHOST_MOVEMENT_ENABLED` keeps the lazy guarantee for users who never
/// enable ghost movement.
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

/// The bounds of the virtual screen: (x, y, width, height).
fn virtual_screen() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

/// The state that only the owner thread touches.
struct GhostHost {
    hwnd: isize,
    bounds: (i32, i32, i32, i32),
    /// The thumbnails registered against the host, and the window each one
    /// shows, which is kept for as long as the thumbnail is. The host is only on top of other windows while there are any.
    thumbnails: HashMap<isize, SnapshotWindow>,
}

impl GhostHost {
    fn create() -> eyre::Result<Self> {
        register_ghost_class()?;

        let instance = WindowsApi::module_handle_w()?;
        let class_name = PCWSTR(GHOST_CLASS_NAME.as_ptr());
        let bounds = virtual_screen();

        // Layered, so that the parts of the host that no thumbnail covers are
        // transparent, and transparent for input, so that it never gets in
        // the way of the mouse.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                class_name,
                class_name,
                WS_POPUP | WS_VISIBLE,
                bounds.0,
                bounds.1,
                bounds.2,
                bounds.3,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };

        unsafe { init_layered_transparent(hwnd) };

        let host = Self {
            hwnd: hwnd.0 as isize,
            bounds,
            thumbnails: HashMap::new(),
        };

        // Idle, the host stays out of the way of everything, the taskbar's
        // auto-hide animation included.
        host.set_topmost(false);

        Ok(host)
    }

    fn set_topmost(&self, topmost: bool) {
        unsafe {
            let _ = SetWindowPos(
                HWND(self.hwnd as *mut c_void),
                Some(if topmost {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                }),
                self.bounds.0,
                self.bounds.1,
                self.bounds.2,
                self.bounds.3,
                SWP_NOACTIVATE,
            );
        }
    }

    /// Follow the virtual screen when monitors have come or gone since the
    /// host was last sized.
    fn fit_to_virtual_screen(&mut self) {
        let bounds = virtual_screen();
        if bounds == self.bounds {
            return;
        }

        self.bounds = bounds;
        unsafe {
            let _ = SetWindowPos(
                HWND(self.hwnd as *mut c_void),
                None,
                bounds.0,
                bounds.1,
                bounds.2,
                bounds.3,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }

    fn origin(&self) -> (i32, i32) {
        (self.bounds.0, self.bounds.1)
    }

    /// Whether there are no ghosts
    fn is_idle(&self) -> bool {
        self.thumbnails.is_empty()
    }

    fn register(
        &mut self,
        snapshot: &Snapshot,
        start_rect: Rect,
        clip: Option<Rect>,
    ) -> eyre::Result<(isize, (i32, i32))> {
        self.fit_to_virtual_screen();

        let source = SnapshotWindow::create(snapshot)?;

        // On top before the first thumbnail can be seen
        if self.thumbnails.is_empty() {
            self.set_topmost(true);
        }

        let origin = self.origin();
        let registered =
            WindowsApi::dwm_register_thumbnail(self.hwnd, source.hwnd).and_then(|hthumb| {
                let props = properties_for(
                    drawn_part(start_rect, (snapshot.width, snapshot.height), clip),
                    origin,
                    255,
                );
                WindowsApi::dwm_update_thumbnail_properties(hthumb, &props)
                    .inspect_err(|_| {
                        let _ = WindowsApi::dwm_unregister_thumbnail(hthumb);
                    })
                    .map(|()| hthumb)
            });

        match registered {
            Ok(hthumb) => {
                self.thumbnails.insert(hthumb, source);
                REGISTERED_THUMBNAILS.fetch_add(1, Ordering::SeqCst);
                tracing::trace!(
                    "ghost owner: registered thumbnail {hthumb}, {} registered",
                    registered_thumbnails()
                );
                Ok((hthumb, origin))
            }
            Err(error) => {
                if self.thumbnails.is_empty() {
                    self.set_topmost(false);
                }
                Err(error)
            }
        }
    }

    fn unregister(&mut self, hthumb: isize) {
        if let Err(error) = WindowsApi::dwm_unregister_thumbnail(hthumb) {
            tracing::warn!("ghost owner: failed to unregister thumbnail {hthumb}: {error}");
        }

        if self.thumbnails.remove(&hthumb).is_some() {
            REGISTERED_THUMBNAILS.fetch_sub(1, Ordering::SeqCst);
        }

        tracing::trace!(
            "ghost owner: unregistered thumbnail {hthumb}, {} registered",
            registered_thumbnails()
        );

        if self.thumbnails.is_empty() {
            self.set_topmost(false);
        }
    }
}

/// Give a layered window a 1x1 fully transparent backing. A layered window
/// that was never given one is not reliably composed, and thumbnails drawn on
/// it do not show up on every GPU.
unsafe fn init_layered_transparent(hwnd: HWND) {
    unsafe {
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 1,
                biHeight: 1,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut c_void = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);
            return;
        };

        if !bits.is_null() {
            std::ptr::write_bytes(bits.cast::<u8>(), 0, 4);
        }

        let previous = SelectObject(mem_dc, bitmap.into());

        let _ = UpdateLayeredWindow(
            hwnd,
            None,
            None,
            Some(&SIZE { cx: 1, cy: 1 }),
            Some(mem_dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            }),
            ULW_ALPHA,
        );

        SelectObject(mem_dc, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);
    }
}

/// How often the owner thread looks for messages for its windows when no
/// command comes. While there are ghosts that is often, so that they are never
/// kept waiting; while there are none the windows it has are a host nobody can
/// see and nothing is sent to, and waking up every few milliseconds for them
/// keeps the processor from sleeping.
fn poll_interval(idle: bool) -> Duration {
    if idle {
        IDLE_POLL_INTERVAL
    } else {
        BUSY_POLL_INTERVAL
    }
}

const BUSY_POLL_INTERVAL: Duration = Duration::from_millis(8);
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(250);

fn run_owner_loop(cmd_rx: crossbeam_channel::Receiver<GhostCmd>) {
    let mut host = match GhostHost::create() {
        Ok(host) => Some(host),
        Err(error) => {
            tracing::error!("ghost owner: failed to create the host window: {error}");
            None
        }
    };

    loop {
        // Drain any pending Win32 messages (DWM/system messages destined for the host).
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        match cmd_rx.recv_timeout(poll_interval(host.as_ref().is_some_and(GhostHost::is_idle))) {
            Ok(GhostCmd::Create {
                snapshot,
                start_rect,
                clip,
                reply,
            }) => {
                let result = match host.as_mut() {
                    Some(host) => host.register(&snapshot, start_rect, clip),
                    None => Err(eyre::eyre!("the ghost host window could not be created")),
                };
                let _ = reply.send(result);
            }
            Ok(GhostCmd::Destroy { hthumb }) => {
                if let Some(host) = host.as_mut() {
                    host.unregister(hthumb);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// A rect (left/top and width/height, in screen coordinates) as a thumbnail
/// destination (in the client coordinates of the host).
fn destination(rect: Rect, origin: (i32, i32)) -> RECT {
    RECT {
        left: rect.left - origin.0,
        top: rect.top - origin.1,
        right: rect.left - origin.0 + rect.right,
        bottom: rect.top - origin.1 + rect.bottom,
    }
}

/// A rect, in pixels of the picture, as the part of it that is shown
fn part_of_source(part: Rect) -> RECT {
    RECT {
        left: part.left,
        top: part.top,
        right: part.left + part.right,
        bottom: part.top + part.bottom,
    }
}

fn thumbnail_properties(destination: RECT, source: RECT, opacity: u8) -> DWM_THUMBNAIL_PROPERTIES {
    DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_VISIBLE | DWM_TNP_RECTDESTINATION | DWM_TNP_RECTSOURCE | DWM_TNP_OPACITY,
        rcDestination: destination,
        rcSource: source,
        opacity,
        fVisible: true.into(),
        fSourceClientAreaOnly: false.into(),
    }
}

/// A thumbnail that is not shown, because none of it is where it may be
fn hidden_thumbnail_properties() -> DWM_THUMBNAIL_PROPERTIES {
    DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_VISIBLE,
        rcDestination: RECT::default(),
        rcSource: RECT::default(),
        opacity: 0,
        fVisible: false.into(),
        fSourceClientAreaOnly: false.into(),
    }
}

/// A "ghost" of a window, used during movement animations: a still picture of
/// the window, taken when the animation starts, that is moved and scaled in the
/// window's place while the window itself, cloaked, is laid out again at its
/// new size. The ghost is automatically disposed on drop, but callers should
/// prefer explicit `dispose()` to surface errors.
pub struct GhostWindow {
    hthumb: isize,
    /// Where the host's client area starts in screen coordinates
    origin: (i32, i32),
    /// How big the picture is, in pixels
    source_size: (i32, i32),
    /// How far the picture reaches out of the window on every side: the room
    /// for its border
    margin: i32,
    /// What the ghost was last made to show
    shown: Mutex<Option<Shown>>,
    disposed: bool,
}

/// What a ghost shows: where it is drawn and which part of its picture goes
/// there, or nothing, and how opaque it is
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown {
    drawn: Option<(Rect, Rect)>,
    opacity: u8,
}

impl GhostWindow {
    /// Take a picture of the window and show it where the window is. The
    /// picture is taken before the window is touched.
    ///
    /// None of it is shown outside `clip`, if there is one, not even for the
    /// moment before the first update.
    pub fn create(src_hwnd: isize, start_rect: Rect, clip: Option<Rect>) -> eyre::Result<Self> {
        let start = std::time::Instant::now();
        let snapshot = Snapshot::capture(src_hwnd)?;

        Self::create_from(snapshot, src_hwnd, start_rect, clip, start)
    }

    /// Like `create`, with a picture that has been taken already, of the window
    /// `src_hwnd`. `start` is when it began to be made, for the log.
    pub fn create_from(
        snapshot: Snapshot,
        src_hwnd: isize,
        start_rect: Rect,
        clip: Option<Rect>,
        start: std::time::Instant,
    ) -> eyre::Result<Self> {
        let captured = start.elapsed();
        let source_size = (snapshot.width, snapshot.height);
        let margin = snapshot.margin;

        // the picture is the window and its border, which is bigger than it
        let mut start_rect = start_rect;
        start_rect.add_margin(margin);

        let (reply_tx, reply_rx) = bounded::<eyre::Result<(isize, (i32, i32))>>(1);
        ghost_owner()
            .cmd_tx
            .send(GhostCmd::Create {
                snapshot,
                start_rect,
                clip,
                reply: reply_tx,
            })
            .map_err(|e| eyre::eyre!("ghost owner channel send failed: {e}"))?;
        let (hthumb, origin) = reply_rx.recv()??;

        tracing::trace!(
            "ghost: created for hwnd {src_hwnd} in {:.1} ms ({:.1} ms of it the picture)",
            start.elapsed().as_secs_f64() * 1000.0,
            captured.as_secs_f64() * 1000.0,
        );

        Ok(Self {
            hthumb,
            origin,
            source_size,
            margin,
            // it is registered where the window is, with all of its picture that
            // is to be seen, and opaque
            shown: Mutex::new(Some(Shown {
                drawn: drawn_part(start_rect, source_size, clip),
                opacity: 255,
            })),
            disposed: false,
        })
    }

    /// Move and scale the ghost to `rect`, and make it `opacity` opaque.
    /// Updating a registered thumbnail has no thread affinity, so this is
    /// applied on the calling thread and is done by the time it returns.
    ///
    /// A ghost that is already there is left alone: asking DWM for what is
    /// already so still has it compose the frame again.
    pub fn update(&self, rect: Rect, opacity: u8) -> eyre::Result<()> {
        self.update_clipped(rect, opacity, None)
    }

    /// Like `update`, but nothing of the ghost is drawn outside `clip`: the
    /// part of it that is outside is cut off, and it is not shown at all when
    /// all of it is.
    pub fn update_clipped(&self, rect: Rect, opacity: u8, clip: Option<Rect>) -> eyre::Result<()> {
        let mut rect = rect;
        rect.add_margin(self.margin);
        let drawn = drawn_part(rect, self.source_size, clip);

        if !needs_update(&mut self.shown.lock(), Shown { drawn, opacity }) {
            return Ok(());
        }

        WindowsApi::dwm_update_thumbnail_properties(
            self.hthumb,
            &properties_for(drawn, self.origin, opacity),
        )
    }

    /// Fade the ghost out over a few frames, where it is, for when there is no
    /// animation left to do it in.
    pub fn fade_out(&self, rect: Rect, clip: Option<Rect>) {
        for step in 1..=FADE_OUT_FRAMES {
            let opacity = ghost_opacity(f64::from(step) / f64::from(FADE_OUT_FRAMES), 0.0);
            let _ = self.update_clipped(rect, opacity, clip);
            unsafe {
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            }
        }
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

/// Where a ghost of a picture of `source_size` that is to be at `rect` is to be
/// drawn and which part of the picture goes there, when nothing of it is to be
/// drawn outside `clip`, if there is one. Nothing if none of it is inside.
fn drawn_part(rect: Rect, source_size: (i32, i32), clip: Option<Rect>) -> Option<(Rect, Rect)> {
    match clip {
        Some(clip) => clip_thumbnail(rect, source_size, clip),
        None => Some((
            rect,
            Rect {
                left: 0,
                top: 0,
                right: source_size.0,
                bottom: source_size.1,
            },
        )),
    }
}

/// What to ask DWM for to show what `drawn_part` says, on a host whose client
/// area starts at `origin` on the screen
fn properties_for(
    drawn: Option<(Rect, Rect)>,
    origin: (i32, i32),
    opacity: u8,
) -> DWM_THUMBNAIL_PROPERTIES {
    match drawn {
        Some((rect, part)) => {
            thumbnail_properties(destination(rect, origin), part_of_source(part), opacity)
        }
        None => hidden_thumbnail_properties(),
    }
}

/// Whether showing what a ghost is to show changes anything, given what it
/// was last made to show, which is remembered if it does
fn needs_update(shown: &mut Option<Shown>, next: Shown) -> bool {
    if *shown == Some(next) {
        return false;
    }

    *shown = Some(next);
    true
}

/// How many frames a ghost is faded out over once its animation is over
const FADE_OUT_FRAMES: u32 = 6;

/// How far into an animation the window itself is shown again, under the
/// ghost, which fades out over it from there. The window has been resizing
/// since the animation started, so that is how long it has had to lay itself
/// out at its new size.
pub const REVEAL_AT: f64 = 0.55;

/// How opaque the ghost is, from 0 to 255, `progress` of the way through the
/// animation, when the window was shown again `revealed_at` of the way
/// through: opaque until then, and then fading out smoothly to nothing at the
/// end, where the window is all that is left.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn ghost_opacity(progress: f64, revealed_at: f64) -> u8 {
    if progress >= 1.0 {
        return 0;
    }

    if progress < revealed_at {
        return 255;
    }

    let t = ((progress - revealed_at) / (1.0 - revealed_at).max(f64::EPSILON)).clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);

    ((1.0 - smooth) * 255.0).round() as u8
}

/// Whether a window class belongs to an application that renders through a
/// swap chain (DirectComposition) and so can be left with a stale render
/// target when it is resized while cloaked: Chromium and Electron, Firefox,
/// Windows Terminal, and the .NET Framework UI stacks.
pub fn is_swap_chain_class(class: &str) -> bool {
    class.starts_with("Chrome_WidgetWin_")
        || class == "MozillaWindowClass"
        || class == "CASCADIA_HOSTING_WINDOW_CLASS"
        || class.starts_with("WindowsForms10.")
        || class.starts_with("HwndWrapper")
}

pub fn is_swap_chain_window(hwnd: isize) -> bool {
    WindowsApi::real_window_class_w(hwnd)
        .map(|class| is_swap_chain_class(&class))
        .unwrap_or(false)
}

/// Resize a window to one pixel narrower than `rect` and straight back.
///
/// After a burst of position changes the compositor of a swap chain window
/// can see a final size that is the same as the last one it was told about
/// and so never rebuild its render target; the window is then stuck with a
/// broken internal viewport until it is restarted. Two real size changes
/// make it rebuild.
///
/// The window is checked between the two calls: the first one pumps messages
/// on the window's thread, and in that time the window can be destroyed and
/// its handle reused for an unrelated window that must not be resized.
pub fn nudge_swap_chain_window(hwnd: isize, rect: &Rect, top: bool) {
    if rect.right <= 1 {
        return;
    }

    let Ok(class) = WindowsApi::real_window_class_w(hwnd) else {
        return;
    };

    let narrower = Rect {
        right: rect.right - 1,
        ..*rect
    };

    if WindowsApi::position_window(hwnd, &narrower, top, false).is_err() {
        return;
    }

    if !WindowsApi::is_window(hwnd)
        || WindowsApi::real_window_class_w(hwnd).is_ok_and(|now| now != class)
    {
        return;
    }

    if let Err(error) = WindowsApi::position_window(hwnd, rect, top, false) {
        tracing::warn!(
            "ghost movement: failed to restore hwnd {hwnd} after a swap chain nudge, \
             it is left one pixel narrower until the next layout pass: {error}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_chain_classes() {
        assert!(is_swap_chain_class("Chrome_WidgetWin_0"));
        assert!(is_swap_chain_class("Chrome_WidgetWin_1"));
        assert!(is_swap_chain_class("MozillaWindowClass"));
        assert!(is_swap_chain_class("CASCADIA_HOSTING_WINDOW_CLASS"));
        assert!(is_swap_chain_class("WindowsForms10.Window.8.app.0.1a2b3c"));
        assert!(is_swap_chain_class("HwndWrapper[App.exe;;abc-123]"));
        assert!(!is_swap_chain_class("Notepad"));
        assert!(!is_swap_chain_class("CabinetWClass"));
        assert!(!is_swap_chain_class("Chrome_RenderWidgetHostHWND"));
        assert!(!is_swap_chain_class(""));
    }

    #[test]
    fn a_ghost_is_only_updated_when_it_changes() {
        let rect = |left| Rect {
            left,
            top: 0,
            right: 100,
            bottom: 50,
        };
        let picture = rect(0);
        let at = |left, opacity| Shown {
            drawn: Some((rect(left), picture)),
            opacity,
        };
        let mut shown = Some(at(0, 255));

        // where it is already
        assert!(!needs_update(&mut shown, at(0, 255)));

        // moved, or faded, or both
        assert!(needs_update(&mut shown, at(10, 255)));
        assert!(!needs_update(&mut shown, at(10, 255)));
        assert!(needs_update(&mut shown, at(10, 200)));
        assert!(needs_update(&mut shown, at(20, 100)));

        // and back, which is a change from where it was last
        assert!(needs_update(&mut shown, at(10, 200)));

        // one that was never put anywhere is put somewhere
        let mut never = None;
        assert!(needs_update(&mut never, at(0, 255)));
    }

    #[test]
    fn a_ghost_is_updated_when_the_part_of_it_that_is_drawn_changes() {
        let rect = |left, width| Rect {
            left,
            top: 0,
            right: width,
            bottom: 50,
        };
        let drawn = |part_left, part_width| Shown {
            drawn: Some((rect(0, part_width), rect(part_left, part_width))),
            opacity: 255,
        };
        let nothing = Shown {
            drawn: None,
            opacity: 255,
        };

        let mut shown = Some(drawn(0, 100));

        // the same place, but a different part of the picture
        assert!(needs_update(&mut shown, drawn(20, 100)));
        assert!(!needs_update(&mut shown, drawn(20, 100)));

        // not drawn at all, which is asked for once
        assert!(needs_update(&mut shown, nothing));
        assert!(!needs_update(&mut shown, nothing));

        // and drawn again
        assert!(needs_update(&mut shown, drawn(20, 100)));
    }

    #[test]
    fn the_owner_thread_polls_slowly_when_there_are_no_ghosts() {
        assert_eq!(poll_interval(false), BUSY_POLL_INTERVAL);
        assert_eq!(poll_interval(true), IDLE_POLL_INTERVAL);
        assert!(IDLE_POLL_INTERVAL >= BUSY_POLL_INTERVAL * 10);
    }

    #[test]
    fn the_ghost_is_opaque_until_the_window_is_shown_and_gone_at_the_end() {
        assert_eq!(ghost_opacity(0.0, REVEAL_AT), 255);
        assert_eq!(ghost_opacity(REVEAL_AT - 0.01, REVEAL_AT), 255);
        assert_eq!(ghost_opacity(REVEAL_AT, REVEAL_AT), 255);
        assert_eq!(ghost_opacity(1.0, REVEAL_AT), 0);
        // an animation that overran
        assert_eq!(ghost_opacity(1.3, REVEAL_AT), 0);

        let fade = (0..=20)
            .map(|step| {
                ghost_opacity(
                    REVEAL_AT + (1.0 - REVEAL_AT) * f64::from(step) / 20.0,
                    REVEAL_AT,
                )
            })
            .collect::<Vec<_>>();
        assert!(fade.windows(2).all(|pair| pair[0] >= pair[1]), "{fade:?}");
        // it is half gone half way through the fade, and not before
        assert!((120..=135).contains(&fade[10]), "{fade:?}");
    }

    #[test]
    fn a_window_that_is_shown_late_has_the_ghost_fade_over_what_is_left() {
        // shown 80% of the way through: opaque until then, gone at the end
        assert_eq!(ghost_opacity(0.79, 0.8), 255);
        assert_eq!(ghost_opacity(0.8, 0.8), 255);
        assert!(ghost_opacity(0.9, 0.8) < 255);
        assert_eq!(ghost_opacity(1.0, 0.8), 0);

        // shown at the very end there is nothing left to fade over
        assert_eq!(ghost_opacity(1.0, 1.0), 0);
        assert_eq!(ghost_opacity(0.99, 1.0), 255);
    }

    /// Needs a desktop to put windows on, so it is only run when asked to:
    /// `cargo test -p komorebi ghost -- --ignored`
    ///
    /// What is asked of DWM to clip a ghost to a rect, as if the rect were the
    /// edge of a monitor with another one next to it, draws none of it outside
    /// the rect, and all of the rest of it where it belongs.
    ///
    /// (The host of the ghosts is a layered window, whose thumbnails the screen
    /// cannot be read back from, so this is done on a host that is not.)
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn dwm_draws_a_clipped_thumbnail_only_inside_its_clip() {
        use windows::Win32::Graphics::Gdi::BitBlt;
        use windows::Win32::Graphics::Gdi::CAPTUREBLT;
        use windows::Win32::Graphics::Gdi::CreateCompatibleBitmap;
        use windows::Win32::Graphics::Gdi::CreateSolidBrush;
        use windows::Win32::Graphics::Gdi::DeleteDC;
        use windows::Win32::Graphics::Gdi::DeleteObject;
        use windows::Win32::Graphics::Gdi::GetPixel;
        use windows::Win32::Graphics::Gdi::ROP_CODE;
        use windows::Win32::Graphics::Gdi::SRCCOPY;
        use windows::Win32::Graphics::Gdi::SelectObject;
        use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
        use windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST;
        use windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
        use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
        use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
        use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
        use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
        use windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE;
        use windows::core::w;

        // windows of two colours nothing else on the desktop is: the one that
        // is thumbnailed (magenta) and the host that it is drawn on (green)
        let instance = WindowsApi::module_handle_w().unwrap();
        let window = |class: windows::core::PCWSTR, colour: u32, x, y, width, height| {
            let window_class = WNDCLASSW {
                hInstance: instance.into(),
                lpszClassName: class,
                lpfnWndProc: Some(ghost_wnd_proc),
                hbrBackground: unsafe { CreateSolidBrush(COLORREF(colour)) },
                ..Default::default()
            };
            let _ = WindowsApi::register_class_w(&window_class);

            unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    class,
                    class,
                    WS_POPUP | WS_VISIBLE,
                    x,
                    y,
                    width,
                    height,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )
                .unwrap()
            }
        };

        let source = window(
            w!("komorebi-ghost-test-source"),
            0x00FF_00FF,
            100,
            100,
            300,
            200,
        );
        let host = window(
            w!("komorebi-ghost-test-host"),
            0x0000_FF00,
            600,
            300,
            800,
            600,
        );
        unsafe {
            SetWindowPos(host, Some(HWND_TOPMOST), 600, 300, 800, 600, SWP_NOACTIVATE).unwrap();
        }
        // paint them now: nothing is pumping the messages of this thread
        for hwnd in [source, host] {
            unsafe {
                use windows::Win32::Graphics::Gdi::RDW_ERASE;
                use windows::Win32::Graphics::Gdi::RDW_INVALIDATE;
                use windows::Win32::Graphics::Gdi::RDW_UPDATENOW;
                use windows::Win32::Graphics::Gdi::RedrawWindow;

                let _ = RedrawWindow(
                    Some(hwnd),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW,
                );
            }
        }
        std::thread::sleep(Duration::from_millis(300));

        let hthumb = WindowsApi::dwm_register_thumbnail(host.0 as isize, source.0 as isize)
            .expect("a thumbnail of the window");
        let size = unsafe { windows::Win32::Graphics::Dwm::DwmQueryThumbnailSourceSize(hthumb) }
            .expect("the size of what is thumbnailed");
        let size = (size.cx, size.cy);

        // shown 400 wide, from 600 on the screen, of which what is right of 800
        // is inside the clip
        let dest = Rect {
            left: 600,
            top: 400,
            right: 400,
            bottom: 300,
        };
        let clip = Rect {
            left: 800,
            top: 0,
            right: 1000,
            bottom: 1500,
        };
        let drawn = drawn_part(dest, size, Some(clip));
        assert!(drawn.is_some());
        WindowsApi::dwm_update_thumbnail_properties(
            hthumb,
            &properties_for(drawn, (600, 300), 255),
        )
        .expect("show it clipped");
        std::thread::sleep(Duration::from_millis(400));

        // the screen is read in pixels, and the windows were put where they are in
        // the units of a process that the display's scaling is applied to
        let scale = WindowsApi::window_rect(host.0 as isize)
            .map(|rect| f64::from(rect.right) / 800.0)
            .unwrap_or(1.0);

        let screen = unsafe { windows::Win32::Graphics::Gdi::GetDC(None) };
        let sample = |x: i32, y: i32| unsafe {
            let (x, y) = (
                (f64::from(x) * scale).round() as i32,
                (f64::from(y) * scale).round() as i32,
            );
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, 1, 1);
            let previous = SelectObject(dc, bitmap.into());
            let _ = BitBlt(
                dc,
                0,
                0,
                1,
                1,
                Some(screen),
                x,
                y,
                ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0),
            );
            let pixel = GetPixel(dc, 0, 0).0 & 0xFF_FFFF;
            SelectObject(dc, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            pixel
        };

        // left of the clip, but in the destination: what the host is
        let outside = sample(650, 450);
        // right of the edge of the clip, in the destination: the window
        let inside = sample(950, 450);
        // in the host and not in the destination at all: what the host is
        let elsewhere = sample(650, 350);

        // all of it moved out of the clip: hidden
        let hidden = drawn_part(Rect { left: 100, ..dest }, size, Some(clip));
        assert!(hidden.is_none());
        WindowsApi::dwm_update_thumbnail_properties(
            hthumb,
            &properties_for(hidden, (600, 300), 255),
        )
        .expect("hide it");
        std::thread::sleep(Duration::from_millis(400));
        let moved_out = sample(950, 450);

        unsafe {
            ReleaseDC(None, screen);
            let _ = WindowsApi::dwm_unregister_thumbnail(hthumb);
            let _ = DestroyWindow(source);
            let _ = DestroyWindow(host);
        }

        assert_eq!(
            outside, 0x00FF00,
            "the host outside the clip, got {outside:#08x}"
        );
        assert_eq!(
            inside, 0xFF00FF,
            "the window inside the clip, got {inside:#08x}"
        );
        assert_eq!(
            elsewhere, 0x00FF00,
            "the host outside the destination, got {elsewhere:#08x}"
        );
        assert_eq!(
            moved_out, 0x00FF00,
            "nothing drawn when all of it is outside, got {moved_out:#08x}"
        );
    }
}
