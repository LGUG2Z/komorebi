use std::sync::atomic::Ordering;

use color_eyre::Result;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW;
use windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;

use crate::window::Window;
use crate::windows_callbacks;
use crate::WindowsApi;
use crate::BORDER_HWND;
use crate::BORDER_OFFSET;
use crate::BORDER_RECT;
use crate::BORDER_WIDTH;
use crate::TRANSPARENCY_COLOUR;

#[derive(Debug, Clone, Copy)]
pub struct Border {
    pub(crate) hwnd: isize,
}

impl From<isize> for Border {
    fn from(hwnd: isize) -> Self {
        Self { hwnd }
    }
}

impl Border {
    pub const fn hwnd(self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn create(name: &str) -> Result<()> {
        let name: Vec<u16> = format!("{name}\0").encode_utf16().collect();
        let instance = WindowsApi::module_handle_w()?;
        let class_name = PCWSTR(name.as_ptr());
        let brush = WindowsApi::create_solid_brush(TRANSPARENCY_COLOUR);
        let window_class = WNDCLASSW {
            hInstance: instance.into(),
            lpszClassName: class_name,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(windows_callbacks::border_window),
            hbrBackground: brush,
            ..Default::default()
        };

        let _atom = WindowsApi::register_class_w(&window_class)?;

        let name_cl = name.clone();
        std::thread::spawn(move || -> Result<()> {
            let hwnd = WindowsApi::create_border_window(PCWSTR(name_cl.as_ptr()), instance)?;
            let border = Self::from(hwnd);

            let mut message = MSG::default();

            unsafe {
                while GetMessageW(&mut message, border.hwnd(), 0, 0).into() {
                    DispatchMessageW(&message);
                }
            }

            Ok(())
        });

        let mut hwnd = HWND(0);
        while hwnd == HWND(0) {
            hwnd = unsafe { FindWindowW(PCWSTR(name.as_ptr()), PCWSTR::null()) };
        }

        BORDER_HWND.store(hwnd.0, Ordering::SeqCst);

        Ok(())
    }

    pub fn hide(self) -> Result<()> {
        if self.hwnd == 0 {
            Ok(())
        } else {
            WindowsApi::hide_border_window(self.hwnd())
        }
    }

    pub fn set_position(self, window: Window, activate: bool) -> Result<()> {
        if self.hwnd == 0 {
            Ok(())
        } else {
            if !WindowsApi::is_window(self.hwnd()) {
                Self::create("komorebi-border-window")?;
            }

            let mut rect = WindowsApi::window_rect(window.hwnd())?;
            rect.add_padding(-BORDER_OFFSET.load(Ordering::SeqCst));

            let border_width = BORDER_WIDTH.load(Ordering::SeqCst);
            rect.add_margin(border_width);

            *BORDER_RECT.lock() = rect;

            WindowsApi::position_border_window(self.hwnd(), &rect, activate)
        }
    }
}
