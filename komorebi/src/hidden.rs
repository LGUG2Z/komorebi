use std::sync::atomic::Ordering;
use std::time::Duration;

use color_eyre::Result;
use windows::core::PCSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageA;
use windows::Win32::UI::WindowsAndMessaging::FindWindowA;
use windows::Win32::UI::WindowsAndMessaging::GetMessageA;
use windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW;
use windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSA;

use crate::windows_callbacks;
use crate::WindowsApi;
use crate::HIDDEN_HWND;
use crate::TRANSPARENCY_COLOUR;

#[derive(Debug, Clone, Copy)]
pub struct Hidden {
    pub(crate) hwnd: isize,
}

impl From<isize> for Hidden {
    fn from(hwnd: isize) -> Self {
        Self { hwnd }
    }
}

impl Hidden {
    pub const fn hwnd(self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn create(name: &str) -> Result<()> {
        let name = format!("{name}\0");
        let instance = WindowsApi::module_handle_w()?;
        let class_name = PCSTR(name.as_ptr());
        let brush = WindowsApi::create_solid_brush(TRANSPARENCY_COLOUR);
        let window_class = WNDCLASSA {
            hInstance: instance.into(),
            lpszClassName: class_name,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(windows_callbacks::hidden_window),
            hbrBackground: brush,
            ..Default::default()
        };

        let _atom = WindowsApi::register_class_a(&window_class)?;

        let name_cl = name.clone();
        std::thread::spawn(move || -> Result<()> {
            let hwnd = WindowsApi::create_hidden_window(PCSTR(name_cl.as_ptr()), instance)?;
            let hidden = Self::from(hwnd);

            let mut message = MSG::default();

            unsafe {
                while GetMessageA(&mut message, hidden.hwnd(), 0, 0).into() {
                    DispatchMessageA(&message);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }

            Ok(())
        });

        let mut hwnd = HWND(0);
        while hwnd == HWND(0) {
            hwnd = unsafe { FindWindowA(PCSTR(name.as_ptr()), PCSTR::null()) };
        }

        HIDDEN_HWND.store(hwnd.0, Ordering::SeqCst);

        Ok(())
    }
}
