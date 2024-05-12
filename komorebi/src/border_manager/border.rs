use crate::border_manager::WindowKind;
use crate::border_manager::BORDER_OFFSET;
use crate::border_manager::BORDER_WIDTH;
use crate::border_manager::FOCUSED;
use crate::border_manager::FOCUS_STATE;
use crate::border_manager::MONOCLE;
use crate::border_manager::RECT_STATE;
use crate::border_manager::STACK;
use crate::border_manager::STYLE;
use crate::border_manager::UNFOCUSED;
use crate::border_manager::Z_ORDER;
use crate::WindowsApi;
use crate::WINDOWS_11;

use komorebi_core::ActiveWindowBorderStyle;
use komorebi_core::Rect;

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::BOOL;
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
use windows::Win32::Graphics::Gdi::RoundRect;
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

pub extern "system" fn border_hwnds(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let hwnds = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };

    if let Ok(class) = WindowsApi::real_window_class_w(hwnd) {
        if class.starts_with("komoborder") {
            hwnds.push(hwnd.0);
        }
    }

    true.into()
}

pub struct Border {
    pub hwnd: isize,
}

impl From<isize> for Border {
    fn from(value: isize) -> Self {
        Self { hwnd: value }
    }
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
            hbrBackground: WindowsApi::create_solid_brush(0),
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
                            FOCUS_STATE
                                .lock()
                                .get(&window.0)
                                .copied()
                                .unwrap_or(WindowKind::Unfocused)
                        };

                        // Set up the brush to draw the border
                        let mut ps = PAINTSTRUCT::default();
                        let hdc = BeginPaint(window, &mut ps);
                        let hpen = CreatePen(
                            PS_SOLID | PS_INSIDEFRAME,
                            BORDER_WIDTH.load(Ordering::SeqCst),
                            COLORREF(match focus_kind {
                                WindowKind::Unfocused => UNFOCUSED.load(Ordering::SeqCst),
                                WindowKind::Single => FOCUSED.load(Ordering::SeqCst),
                                WindowKind::Stack => STACK.load(Ordering::SeqCst),
                                WindowKind::Monocle => MONOCLE.load(Ordering::SeqCst),
                            }),
                        );

                        let hbrush = WindowsApi::create_solid_brush(0);

                        // Draw the border
                        SelectObject(hdc, hpen);
                        SelectObject(hdc, hbrush);
                        // TODO(raggi): this is approximately the correct curvature for
                        // the top left of a Windows 11 window (DWMWCP_DEFAULT), but
                        // often the bottom right has a different shape. Furthermore if
                        // the window was made with DWMWCP_ROUNDSMALL then this is the
                        // wrong size.  In the future we should read the DWM properties
                        // of windows and attempt to match appropriately.
                        match *STYLE.lock() {
                            ActiveWindowBorderStyle::System => {
                                if *WINDOWS_11 {
                                    RoundRect(hdc, 0, 0, rect.right, rect.bottom, 20, 20);
                                } else {
                                    Rectangle(hdc, 0, 0, rect.right, rect.bottom);
                                }
                            }
                            ActiveWindowBorderStyle::Rounded => {
                                RoundRect(hdc, 0, 0, rect.right, rect.bottom, 20, 20);
                            }
                            ActiveWindowBorderStyle::Square => {
                                Rectangle(hdc, 0, 0, rect.right, rect.bottom);
                            }
                        }
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
