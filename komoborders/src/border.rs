use komoborders_client::ZOrder;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Gdi::BeginPaint;
use windows::Win32::Graphics::Gdi::CreatePen;
use windows::Win32::Graphics::Gdi::EndPaint;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::Graphics::Gdi::Rectangle;
use windows::Win32::Graphics::Gdi::SelectObject;
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::Graphics::Gdi::PAINTSTRUCT;
use windows::Win32::Graphics::Gdi::PS_INSIDEFRAME;
use windows::Win32::Graphics::Gdi::PS_SOLID;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW;
use windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_PAINT;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;

use crate::FocusKind;
use crate::FOCUSED_STATE;
use crate::RECT_STATE;
use komorebi::Rgb;
use komorebi::WindowsApi;
use komorebi_client::Rect;

pub static TRANSPARENCY: u32 = 0;
pub static BORDER_WIDTH: AtomicI32 = AtomicI32::new(8);
pub static BORDER_OFFSET: AtomicI32 = AtomicI32::new(-1);

lazy_static! {
    pub static ref Z_ORDER: Arc<Mutex<ZOrder>> = Arc::new(Mutex::new(ZOrder::Bottom));
    pub static ref FOCUSED: AtomicU32 = AtomicU32::new(u32::from(komorebi_client::Colour::Rgb(
        Rgb::new(66, 165, 245)
    )));
    pub static ref UNFOCUSED: AtomicU32 = AtomicU32::new(u32::from(komorebi_client::Colour::Rgb(
        Rgb::new(128, 128, 128)
    )));
    pub static ref MONOCLE: AtomicU32 = AtomicU32::new(u32::from(komorebi_client::Colour::Rgb(
        Rgb::new(255, 51, 153)
    )));
    pub static ref STACK: AtomicU32 = AtomicU32::new(u32::from(komorebi_client::Colour::Rgb(
        Rgb::new(0, 165, 66)
    )));
}

pub struct Border {
    pub hwnd: isize,
}

impl Border {
    pub const fn hwnd(&self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn create(id: &str) -> color_eyre::Result<Self> {
        let name: Vec<u16> = format!("komoborder-{id}\0").encode_utf16().collect();
        let class_name = PCWSTR(name.as_ptr());

        let h_module = WindowsApi::module_handle_w()?;

        let window_class = WNDCLASSW {
            hInstance: h_module.into(),
            lpszClassName: class_name,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(Self::callback),
            hbrBackground: WindowsApi::create_solid_brush(TRANSPARENCY),
            ..Default::default()
        };

        let _ = WindowsApi::register_class_w(&window_class);

        let (hwnd_sender, hwnd_receiver) = mpsc::channel();

        std::thread::spawn(move || -> color_eyre::Result<()> {
            let hwnd = WindowsApi::create_border_window(PCWSTR(name.as_ptr()), h_module)?;
            hwnd_sender.send(hwnd)?;

            let mut message = MSG::default();
            unsafe {
                while GetMessageW(&mut message, HWND(hwnd), 0, 0).into() {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }

            Ok(())
        });

        Ok(Self {
            hwnd: hwnd_receiver.recv()?,
        })
    }

    pub fn destroy(&self) -> color_eyre::Result<()> {
        WindowsApi::destroy_window(self.hwnd())
    }

    pub fn update(&self, rect: &Rect) -> color_eyre::Result<()> {
        // Make adjustments to the border
        let mut rect = *rect;
        rect.add_margin(BORDER_WIDTH.load(Ordering::SeqCst));
        rect.add_padding(-BORDER_OFFSET.load(Ordering::SeqCst));

        // Store the border rect so that it can be used by the callback
        {
            let mut rects = RECT_STATE.lock();
            rects.insert(self.hwnd, rect);
        }

        // Update the position of the border
        WindowsApi::set_border_pos(self.hwnd(), &rect, HWND((*Z_ORDER.lock()).into()))?;

        // Invalidate the rect to trigger the callback to update colours etc.
        self.invalidate();

        Ok(())
    }

    pub fn invalidate(&self) {
        let _ = unsafe { InvalidateRect(self.hwnd(), None, false) };
    }

    pub extern "system" fn callback(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            match message {
                WM_PAINT => {
                    let rects = RECT_STATE.lock();

                    // With the rect that we stored in Self::update
                    if let Some(rect) = rects.get(&window.0).copied() {
                        // Grab the focus kind for this border
                        let focus_kind = {
                            FOCUSED_STATE
                                .lock()
                                .get(&window.0)
                                .copied()
                                .unwrap_or(FocusKind::Unfocused)
                        };

                        // Set up the brush to draw the border
                        let mut ps = PAINTSTRUCT::default();
                        let hdc = BeginPaint(window, &mut ps);
                        let hpen = CreatePen(
                            PS_SOLID | PS_INSIDEFRAME,
                            BORDER_WIDTH.load(Ordering::SeqCst),
                            COLORREF(match focus_kind {
                                FocusKind::Unfocused => UNFOCUSED.load(Ordering::SeqCst),
                                FocusKind::Single => FOCUSED.load(Ordering::SeqCst),
                                FocusKind::Stack => STACK.load(Ordering::SeqCst),
                                FocusKind::Monocle => MONOCLE.load(Ordering::SeqCst),
                            }),
                        );

                        let hbrush = WindowsApi::create_solid_brush(TRANSPARENCY);

                        // Draw the border
                        SelectObject(hdc, hpen);
                        SelectObject(hdc, hbrush);
                        Rectangle(hdc, 0, 0, rect.right, rect.bottom);
                        EndPaint(window, &ps);
                        ValidateRect(window, None);
                    }

                    LRESULT(0)
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(window, message, wparam, lparam),
            }
        }
    }
}
