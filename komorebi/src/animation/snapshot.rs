//! A still picture of a window, for a ghost to show.
//!
//! A ghost that is a live thumbnail of the window it stands in for shows
//! whatever the window is painting, and a window that has just been resized
//! paints a blank frame, or nothing at all, before it paints its new layout.
//! A ghost has to hold on to what the window looked like when it started
//! moving, so the window is captured once, and the picture is shown through a
//! window of our own, which DWM can scale on the GPU like any other.

use color_eyre::eyre;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Dwm::DWMWA_CLOAK;
use windows::Win32::Graphics::Dwm::DwmSetWindowAttribute;
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
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Gdi::ReleaseDC;
use windows::Win32::Graphics::Gdi::SelectObject;
use windows::Win32::Storage::Xps::PRINT_WINDOW_FLAGS;
use windows::Win32::Storage::Xps::PrintWindow;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNA;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::ULW_ALPHA;
use windows::Win32::UI::WindowsAndMessaging::UpdateLayeredWindow;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TRANSPARENT;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::core::BOOL;
use windows::core::PCWSTR;
use windows::core::w;

use crate::WindowsApi;

/// Ask for what the window looks like to the compositor, which is the only
/// way to capture a window that is drawn with DirectComposition
const PW_RENDERFULLCONTENT: u32 = 2;

/// How round the corners of windows are, in pixels: 8 at 100% scale, a little
/// more where the display is scaled up, so a little more than 8 for them all.
const CORNER_RADIUS: i32 = 10;

/// The picture of a window, as it was shown on screen: without the invisible
/// borders it has around it.
pub struct Snapshot {
    bitmap: HBITMAP,
    /// Where the pixels of the bitmap are, which is as long as the bitmap is
    pixels: usize,
    pub width: i32,
    pub height: i32,
}

// A GDI bitmap is not tied to the thread that made it
unsafe impl Send for Snapshot {}

impl Drop for Snapshot {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.bitmap.into());
        }
    }
}

fn create_dib(dc: HDC, width: i32, height: i32) -> eyre::Result<(HBITMAP, &'static mut [u32])> {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // top-down
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut c_void = std::ptr::null_mut();
    let bitmap = unsafe { CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }?;

    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(bitmap.into());
        }
        eyre::bail!("a bitmap of {width}x{height} has no pixels");
    }

    // The pixels live as long as the bitmap, which the caller owns
    let pixels =
        unsafe { std::slice::from_raw_parts_mut(bits.cast::<u32>(), (width * height) as usize) };

    Ok((bitmap, pixels))
}

/// Copy a `width` x `height` area, starting at (`x`, `y`) in `source`, which
/// is `source_width` pixels wide, into `destination`.
fn copy_area(
    source: &[u32],
    source_width: i32,
    x: i32,
    y: i32,
    destination: &mut [u32],
    width: i32,
    height: i32,
) {
    for row in 0..height {
        let from = ((y + row) * source_width + x) as usize;
        let to = (row * width) as usize;
        destination[to..to + width as usize].copy_from_slice(&source[from..from + width as usize]);
    }
}

/// Make every pixel opaque. (Premultiplied, so the colour is as it was.)
fn make_opaque(pixels: &mut [u32]) {
    for pixel in pixels.iter_mut() {
        *pixel |= 0xFF00_0000;
    }
}

/// Whether the pixels of a picture are all opaque, as far as a grid of them
/// across the picture says, which is a lot cheaper than looking at every one
/// of the millions there can be. The picture a window renders of itself
/// normally is, and then there is nothing to do.
fn looks_opaque(pixels: &[u32], width: i32, height: i32) -> bool {
    let step_x = (width / 32).max(1) as usize;
    let step_y = (height / 32).max(1) as usize;

    (0..height as usize).step_by(step_y).all(|y| {
        (0..width as usize)
            .step_by(step_x)
            .all(|x| pixels[y * width as usize + x] >> 24 == 0xFF)
    })
}

/// Make the pixels outside the rounded corners of a window, which has opaque
/// pixels, fully transparent. (Premultiplied, so those are all zero.)
fn round_corners(pixels: &mut [u32], width: i32, height: i32, radius: i32) {
    let radius = radius.min(width / 2).min(height / 2).max(0);

    for corner_y in 0..radius {
        for corner_x in 0..radius {
            // the distance from the middle of the circle the corner is a
            // quarter of, to the middle of this pixel
            let dx = (radius - corner_x) as f32 - 0.5;
            let dy = (radius - corner_y) as f32 - 0.5;

            if dx * dx + dy * dy > (radius * radius) as f32 {
                for (x, y) in [
                    (corner_x, corner_y),
                    (width - 1 - corner_x, corner_y),
                    (corner_x, height - 1 - corner_y),
                    (width - 1 - corner_x, height - 1 - corner_y),
                ] {
                    pixels[(y * width + x) as usize] = 0;
                }
            }
        }
    }
}

/// What a window rendered of itself, and where in that its visible frame is
struct Rendered {
    bitmap: HBITMAP,
    pixels: &'static [u32],
    window_width: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Drop for Rendered {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.bitmap.into());
        }
    }
}

/// Have a window render itself, which it does synchronously, whatever it takes
/// it to do so, into a bitmap of the size of the window.
fn render(hwnd: isize) -> eyre::Result<Rendered> {
    let handle = HWND(hwnd as *mut c_void);

    let mut window = RECT::default();
    unsafe { GetWindowRect(handle, &mut window) }?;
    let window_width = window.right - window.left;
    let window_height = window.bottom - window.top;

    // what is on screen is the extended frame, inside the invisible borders
    let frame = WindowsApi::window_rect(hwnd)?;
    let x = (frame.left - window.left).clamp(0, window_width);
    let y = (frame.top - window.top).clamp(0, window_height);
    let width = frame.right.min(window_width - x);
    let height = frame.bottom.min(window_height - y);

    if width <= 0 || height <= 0 {
        eyre::bail!("hwnd {hwnd} has no visible area to take a picture of");
    }

    let start = std::time::Instant::now();

    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));

        let result = (|| {
            let (bitmap, pixels) = create_dib(dc, window_width, window_height)?;
            let allocated = start.elapsed();

            let previous = SelectObject(dc, bitmap.into());
            let printed = PrintWindow(handle, dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT));
            SelectObject(dc, previous);

            tracing::trace!(
                "snapshot: hwnd {hwnd} {window_width}x{window_height}: allocate {:.1} ms, \
                 PrintWindow {:.1} ms",
                allocated.as_secs_f64() * 1000.0,
                (start.elapsed() - allocated).as_secs_f64() * 1000.0,
            );

            let rendered = Rendered {
                bitmap,
                pixels,
                window_width,
                x,
                y,
                width,
                height,
            };

            if printed.as_bool() {
                Ok(rendered)
            } else {
                Err(eyre::eyre!("hwnd {hwnd} would not render itself"))
            }
        })();

        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);

        result
    }
}

/// Ask a window to paint itself, as it is now. Having a window render itself
/// is synchronous, which makes it a way to have a window that has just been
/// resized lay itself out at its new size, without waiting for it to get round
/// to it. What it renders is of no use here, so nothing is made of it.
pub fn paint_now(hwnd: isize) {
    let start = std::time::Instant::now();
    let _ = render(hwnd);
    tracing::trace!(
        "snapshot: hwnd {hwnd} painted itself in {:.1} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

impl Snapshot {
    /// Take a picture of a window as it is now. This asks the window to
    /// render itself, so it can take a while for one that is busy, and it is
    /// not for the thread that handles komorebi's events.
    pub fn capture(hwnd: isize) -> eyre::Result<Self> {
        let rendered = render(hwnd)?;

        let start = std::time::Instant::now();

        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));

            let result = create_dib(dc, rendered.width, rendered.height).map(|(bitmap, pixels)| {
                copy_area(
                    rendered.pixels,
                    rendered.window_width,
                    rendered.x,
                    rendered.y,
                    pixels,
                    rendered.width,
                    rendered.height,
                );

                if !looks_opaque(pixels, rendered.width, rendered.height) {
                    make_opaque(pixels);
                }
                round_corners(pixels, rendered.width, rendered.height, CORNER_RADIUS);

                Self {
                    bitmap,
                    pixels: pixels.as_ptr() as usize,
                    width: rendered.width,
                    height: rendered.height,
                }
            });

            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);

            tracing::trace!(
                "snapshot: hwnd {hwnd}: picture {:.1} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );

            result
        }
    }

    /// A picture of one colour, for tests
    #[cfg(test)]
    pub fn solid(width: i32, height: i32, color: u32) -> Self {
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let (bitmap, pixels) = create_dib(dc, width, height).unwrap();
            pixels.fill(color | 0xFF00_0000);
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);

            Self {
                bitmap,
                pixels: pixels.as_ptr() as usize,
                width,
                height,
            }
        }
    }

    /// Another picture that is the same as this one
    pub fn duplicate(&self) -> eyre::Result<Self> {
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));

            let result = create_dib(dc, self.width, self.height).map(|(bitmap, pixels)| {
                let source = std::slice::from_raw_parts(
                    self.pixels as *const u32,
                    (self.width * self.height) as usize,
                );
                pixels.copy_from_slice(source);

                Self {
                    bitmap,
                    pixels: pixels.as_ptr() as usize,
                    width: self.width,
                    height: self.height,
                }
            });

            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);

            result
        }
    }

    fn bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

/// How many pictures are kept of windows that left, at most
const REMEMBERED_MAX: usize = 12;

/// How much memory the pictures that are kept can take up, at most
const REMEMBERED_MAX_BYTES: usize = 256 * 1024 * 1024;

/// How long a picture is kept: what a window looked like that long ago is not
/// what it looks like now, and the window is not shown for long as it was.
const REMEMBERED_MAX_AGE: Duration = Duration::from_secs(10 * 60);

/// How much bigger or smaller than a window a picture of it can be, in pixels,
/// and be shown in its place: it is not scaled to anything much different
const REMEMBERED_SIZE_TOLERANCE: i32 = 4;

/// The pictures of windows as they left, for the ghosts of them to show when
/// they are back. A window that is hidden is not to be asked to render itself:
/// a picture of it can only be taken by having it paint something it is not
/// shown for, and Chromium and Electron windows that have been asked to have
/// been seen to be left blank or drawn off their place until their process is
/// restarted.
#[derive(Default)]
struct Remembered {
    pictures: HashMap<isize, (Instant, Snapshot)>,
}

impl Remembered {
    fn remember(&mut self, hwnd: isize, picture: Snapshot, now: Instant) {
        self.pictures.insert(hwnd, (now, picture));
        self.prune(now);
    }

    /// The picture of the window, which is not kept any more, if there is one
    /// of about this size that is not too old
    fn recall(&mut self, hwnd: isize, size: (i32, i32), now: Instant) -> Option<Snapshot> {
        let (since, picture) = self.pictures.remove(&hwnd)?;

        let fits = (picture.width - size.0).abs() <= REMEMBERED_SIZE_TOLERANCE
            && (picture.height - size.1).abs() <= REMEMBERED_SIZE_TOLERANCE;

        (fits && now.saturating_duration_since(since) < REMEMBERED_MAX_AGE).then_some(picture)
    }

    /// Get rid of what is too old, and then of the oldest while there is too
    /// much of it
    fn prune(&mut self, now: Instant) {
        self.pictures
            .retain(|_, (since, _)| now.saturating_duration_since(*since) < REMEMBERED_MAX_AGE);

        while self.pictures.len() > REMEMBERED_MAX
            || self
                .pictures
                .values()
                .map(|(_, picture)| picture.bytes())
                .sum::<usize>()
                > REMEMBERED_MAX_BYTES
        {
            let Some(oldest) = self
                .pictures
                .iter()
                .min_by_key(|(_, (since, _))| *since)
                .map(|(hwnd, _)| *hwnd)
            else {
                break;
            };

            self.pictures.remove(&oldest);
        }
    }
}

lazy_static! {
    static ref REMEMBERED: Mutex<Remembered> = Mutex::new(Remembered::default());
}

/// Keep a picture of a window that is about to leave
pub fn remember(hwnd: isize, picture: &Snapshot) {
    match picture.duplicate() {
        Ok(copy) => REMEMBERED.lock().remember(hwnd, copy, Instant::now()),
        Err(error) => tracing::trace!("snapshot: could not keep a picture of hwnd {hwnd}: {error}"),
    }
}

/// The picture that was kept of a window that is coming back, if it is one that
/// can be shown for it. `size` is what it is to be shown as.
pub fn recall(hwnd: isize, size: (i32, i32)) -> Option<Snapshot> {
    REMEMBERED.lock().recall(hwnd, size, Instant::now())
}

extern "system" fn snapshot_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn register_class() -> eyre::Result<PCWSTR> {
    static REGISTERED: OnceLock<()> = OnceLock::new();

    let class = w!("komorebi-ghost-snapshot");

    REGISTERED.get_or_init(|| {
        if let Ok(instance) = WindowsApi::module_handle_w() {
            let class = WNDCLASSW {
                hInstance: instance.into(),
                lpszClassName: class,
                lpfnWndProc: Some(snapshot_wnd_proc),
                ..Default::default()
            };
            // already being registered is fine
            let _ = WindowsApi::register_class_w(&class);
        }
    });

    Ok(class)
}

/// A window that shows a `Snapshot` and is never seen itself: it is cloaked,
/// which keeps it composed, so that DWM can be asked for a thumbnail of it.
///
/// It has to be made, and dropped, by a thread that handles messages.
pub struct SnapshotWindow {
    pub hwnd: isize,
}

impl SnapshotWindow {
    pub fn create(snapshot: &Snapshot) -> eyre::Result<Self> {
        let class = register_class()?;
        let instance = WindowsApi::module_handle_w()?;

        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                class,
                class,
                WS_POPUP,
                0,
                0,
                snapshot.width,
                snapshot.height,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let window = Self {
                hwnd: hwnd.0 as isize,
            };

            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let previous = SelectObject(dc, snapshot.bitmap.into());

            let updated = UpdateLayeredWindow(
                hwnd,
                None,
                None,
                Some(&SIZE {
                    cx: snapshot.width,
                    cy: snapshot.height,
                }),
                Some(dc),
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

            SelectObject(dc, previous);
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);

            updated?;

            // cloaked before it is shown, so it is never on screen
            let cloak = BOOL::from(true);
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_CLOAK,
                std::ptr::from_ref(&cloak).cast(),
                std::mem::size_of::<BOOL>() as u32,
            )?;
            let _ = ShowWindow(hwnd, SW_SHOWNA);

            Ok(window)
        }
    }
}

impl Drop for SnapshotWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(HWND(self.hwnd as *mut c_void));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_cut_out_and_the_rest_is_kept() {
        let (width, height) = (40, 30);
        let mut pixels = vec![0xFF10_2030_u32; (width * height) as usize];

        round_corners(&mut pixels, width, height, 10);

        let at = |x: i32, y: i32| pixels[(y * width + x) as usize];

        // the very corners are gone
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(width - 1, 0), 0);
        assert_eq!(at(0, height - 1), 0);
        assert_eq!(at(width - 1, height - 1), 0);

        // the middle and the straight edges are kept
        assert_eq!(at(20, 15), 0xFF10_2030);
        assert_eq!(at(20, 0), 0xFF10_2030);
        assert_eq!(at(0, 15), 0xFF10_2030);
        assert_eq!(at(width - 1, height - 1 - 10), 0xFF10_2030);

        // the cut is round: a pixel just inside the circle stays
        assert_eq!(at(4, 4), 0xFF10_2030);
        assert_eq!(at(1, 1), 0);
    }

    #[test]
    fn corners_of_a_tiny_picture_do_not_run_out_of_bounds() {
        let mut pixels = vec![0xFFFF_FFFF_u32; 9];
        round_corners(&mut pixels, 3, 3, 10);
        assert!(
            pixels
                .iter()
                .all(|pixel| *pixel == 0xFFFF_FFFF || *pixel == 0)
        );
    }

    #[test]
    fn pixels_are_made_opaque_without_changing_their_colour() {
        let mut pixels = vec![0x0010_2030_u32, 0x0000_0000, 0x7F11_2233];
        make_opaque(&mut pixels);
        assert_eq!(pixels, vec![0xFF10_2030, 0xFF00_0000, 0xFF11_2233]);
    }

    #[test]
    fn a_picture_looks_opaque_when_the_pixels_across_it_are() {
        let (width, height) = (100, 80);
        let mut pixels = vec![0xFF00_FF00_u32; (width * height) as usize];
        assert!(looks_opaque(&pixels, width, height));

        // a pixel on the grid that is not
        pixels[0] = 0x0000_FF00;
        assert!(!looks_opaque(&pixels, width, height));
        pixels[0] = 0xFF00_FF00;

        // a whole row of them, on the grid
        for x in 0..width {
            pixels[(4 * width + x) as usize] = 0;
        }
        assert!(!looks_opaque(&pixels, width, height));

        // pictures too small for a grid are looked at pixel by pixel
        assert!(looks_opaque(&[0xFF00_0000_u32; 6], 3, 2));
        assert!(!looks_opaque(
            &[0xFF00_0000, 0xFF00_0000, 0x0, 0xFF00_0000],
            2,
            2
        ));
    }

    #[test]
    fn an_area_is_copied_out_of_a_picture() {
        // 4x3, each pixel numbered by where it is
        let source: Vec<u32> = (0..12).collect();
        let mut area = vec![0_u32; 4];

        copy_area(&source, 4, 1, 1, &mut area, 2, 2);

        assert_eq!(area, vec![5, 6, 9, 10]);
    }

    /// Needs a desktop to put windows on, so it is only run when asked to:
    /// `cargo test -p komorebi snapshot -- --ignored`
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn a_snapshot_window_can_be_shown_by_a_thumbnail_without_being_seen() {
        use windows::Win32::Graphics::Gdi::GetPixel;
        use windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST;
        use windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE;
        use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;

        fn host(x: i32, y: i32, width: i32, height: i32) -> HWND {
            unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                    w!("Static"),
                    w!("snapshot test"),
                    WS_POPUP | windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE,
                    x,
                    y,
                    width,
                    height,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
            }
        }

        // a red window, of a size that is not the size it is shown at
        let snapshot = Snapshot::solid(300, 200, 0x00FF_0000);
        let source = SnapshotWindow::create(&snapshot).expect("a window for the picture");

        let host = host(300, 300, 800, 600);
        unsafe {
            SetWindowPos(host, Some(HWND_TOPMOST), 300, 300, 800, 600, SWP_NOACTIVATE).unwrap();
        }

        let hthumb = WindowsApi::dwm_register_thumbnail(host.0 as isize, source.hwnd)
            .expect("a thumbnail of the picture");

        // stretched over the whole of the host, and from the other side of the
        // screen where the window itself is
        let props = windows::Win32::Graphics::Dwm::DWM_THUMBNAIL_PROPERTIES {
            dwFlags: windows::Win32::Graphics::Dwm::DWM_TNP_VISIBLE
                | windows::Win32::Graphics::Dwm::DWM_TNP_RECTDESTINATION
                | windows::Win32::Graphics::Dwm::DWM_TNP_OPACITY,
            rcDestination: RECT {
                left: 0,
                top: 0,
                right: 800,
                bottom: 600,
            },
            rcSource: RECT::default(),
            opacity: 255,
            fVisible: true.into(),
            fSourceClientAreaOnly: false.into(),
        };
        WindowsApi::dwm_update_thumbnail_properties(hthumb, &props).expect("show the thumbnail");

        // give DWM a moment to compose it
        std::thread::sleep(std::time::Duration::from_millis(400));

        // the part of the screen under the host, and well away from the corners
        let screen = unsafe { GetDC(None) };
        let sample = |x, y| unsafe { GetPixel(screen, x, y).0 };
        // COLORREF is 0x00BBGGRR
        let (middle, far_corner) = (sample(700, 600), sample(300 + 790, 300 + 590));
        unsafe { ReleaseDC(None, screen) };

        unsafe {
            let _ = WindowsApi::dwm_unregister_thumbnail(hthumb);
            let _ = DestroyWindow(host);
        }

        assert_eq!(middle & 0xFF, 0xFF, "red, got {middle:#08x}");
        assert_eq!((middle >> 8) & 0xFF, 0, "red, got {middle:#08x}");
        // stretched, so it reaches the corner of the host as well
        assert_eq!(far_corner & 0xFF, 0xFF, "red, got {far_corner:#08x}");
    }

    fn picture(width: i32, height: i32) -> Snapshot {
        Snapshot::solid(width, height, 0x0010_2030)
    }

    #[test]
    fn a_picture_that_was_kept_is_given_back_once() {
        let mut remembered = Remembered::default();
        let now = Instant::now();

        remembered.remember(1, picture(100, 50), now);

        let back = remembered.recall(1, (100, 50), now).unwrap();
        assert_eq!((back.width, back.height), (100, 50));

        assert!(remembered.recall(1, (100, 50), now).is_none());
    }

    #[test]
    fn a_picture_is_not_shown_for_a_window_of_another_size_or_much_later() {
        let mut remembered = Remembered::default();
        let now = Instant::now();

        // a few pixels either way is the same window
        remembered.remember(1, picture(100, 50), now);
        assert!(remembered.recall(1, (104, 46), now).is_some());

        // but a window that has been given a different shape is not the picture
        remembered.remember(2, picture(100, 50), now);
        assert!(remembered.recall(2, (200, 50), now).is_none());
        assert!(
            remembered.recall(2, (100, 50), now).is_none(),
            "and it is gone"
        );

        // and what a window looked like long ago is not what it looks like
        remembered.remember(3, picture(100, 50), now);
        assert!(
            remembered
                .recall(3, (100, 50), now + REMEMBERED_MAX_AGE)
                .is_none()
        );
    }

    #[test]
    fn the_oldest_pictures_are_the_first_to_go_when_there_are_too_many() {
        let mut remembered = Remembered::default();
        let start = Instant::now();

        for hwnd in 0..(REMEMBERED_MAX as isize + 3) {
            remembered.remember(
                hwnd,
                picture(10, 10),
                start + Duration::from_millis(hwnd as u64),
            );
        }

        assert_eq!(remembered.pictures.len(), REMEMBERED_MAX);
        // the first three were the oldest
        for hwnd in 0..3 {
            assert!(!remembered.pictures.contains_key(&hwnd));
        }
        assert!(remembered.pictures.contains_key(&3));
    }

    #[test]
    fn a_duplicate_is_the_same_picture() {
        let original = picture(20, 10);
        let copy = original.duplicate().unwrap();
        assert_eq!((copy.width, copy.height), (20, 10));
        let pixels = |picture: &Snapshot| unsafe {
            std::slice::from_raw_parts(picture.pixels as *const u32, 200).to_vec()
        };
        assert_eq!(pixels(&original), pixels(&copy));
        assert_ne!(original.pixels, copy.pixels);
    }
}
