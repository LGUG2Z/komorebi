//! A still picture of a window, for a ghost to show.
//!
//! A ghost that is a live thumbnail of the window it stands in for shows
//! whatever the window is painting, and a window that has just been resized
//! paints a blank frame, or nothing at all, before it paints its new layout.
//! A ghost has to hold on to what the window looked like when it started
//! moving, so the window is captured once, and the picture is shown through a
//! window of our own, which DWM can scale on the GPU like any other.

use color_eyre::eyre;
use std::ffi::c_void;
use std::sync::OnceLock;
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

/// Make every pixel opaque, except for the ones outside the rounded corners of
/// a window, which are made fully transparent. (Premultiplied, so those are
/// all zero.)
fn round_corners(pixels: &mut [u32], width: i32, height: i32, radius: i32) {
    let radius = radius.min(width / 2).min(height / 2).max(0);

    for pixel in pixels.iter_mut() {
        *pixel |= 0xFF00_0000;
    }

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

/// Ask a window to paint itself, as it is now. Taking a picture of a window
/// makes it render synchronously (see `Snapshot::capture`), which is also a
/// way to have a window that has just been resized lay itself out at its new
/// size, without waiting for it to get round to it.
pub fn paint_now(hwnd: isize) {
    let start = std::time::Instant::now();
    let _ = Snapshot::capture(hwnd);
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
                let (full, full_pixels) = create_dib(dc, window_width, window_height)?;
                let allocated = start.elapsed();
                let previous = SelectObject(dc, full.into());
                let printed = PrintWindow(handle, dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT));
                SelectObject(dc, previous);
                let rendered = start.elapsed();

                let result = if printed.as_bool() {
                    create_dib(dc, width, height).map(|(bitmap, pixels)| {
                        copy_area(full_pixels, window_width, x, y, pixels, width, height);
                        round_corners(pixels, width, height, CORNER_RADIUS);
                        Self {
                            bitmap,
                            width,
                            height,
                        }
                    })
                } else {
                    Err(eyre::eyre!("hwnd {hwnd} would not render itself"))
                };

                let _ = DeleteObject(full.into());

                tracing::trace!(
                    "snapshot: hwnd {hwnd} {window_width}x{window_height}: allocate {:.1} ms, \
                     PrintWindow {:.1} ms, picture {:.1} ms",
                    allocated.as_secs_f64() * 1000.0,
                    (rendered - allocated).as_secs_f64() * 1000.0,
                    (start.elapsed() - rendered).as_secs_f64() * 1000.0,
                );

                result
            })();

            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);

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
                width,
                height,
            }
        }
    }
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
    fn corners_are_cut_out_and_the_rest_is_opaque() {
        let (width, height) = (40, 30);
        let mut pixels = vec![0x0010_2030_u32; (width * height) as usize];

        round_corners(&mut pixels, width, height, 10);

        let at = |x: i32, y: i32| pixels[(y * width + x) as usize];

        // the very corners are gone
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(width - 1, 0), 0);
        assert_eq!(at(0, height - 1), 0);
        assert_eq!(at(width - 1, height - 1), 0);

        // the middle and the straight edges are kept, and opaque
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
        let mut pixels = vec![0x00FF_FFFF_u32; 9];
        round_corners(&mut pixels, 3, 3, 10);
        assert!(
            pixels
                .iter()
                .all(|pixel| *pixel == 0xFFFF_FFFF || *pixel == 0)
        );
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
}
