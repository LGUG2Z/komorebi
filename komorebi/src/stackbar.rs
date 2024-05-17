use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::time::Duration;

use color_eyre::eyre::Result;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use windows::core::PCWSTR;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Gdi::CreateFontIndirectW;
use windows::Win32::Graphics::Gdi::CreatePen;
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::Graphics::Gdi::DrawTextW;
use windows::Win32::Graphics::Gdi::GetDC;
use windows::Win32::Graphics::Gdi::ReleaseDC;
use windows::Win32::Graphics::Gdi::SelectObject;
use windows::Win32::Graphics::Gdi::SetBkColor;
use windows::Win32::Graphics::Gdi::SetTextColor;
use windows::Win32::Graphics::Gdi::DT_CENTER;
use windows::Win32::Graphics::Gdi::DT_END_ELLIPSIS;
use windows::Win32::Graphics::Gdi::DT_SINGLELINE;
use windows::Win32::Graphics::Gdi::DT_VCENTER;
use windows::Win32::Graphics::Gdi::FONT_QUALITY;
use windows::Win32::Graphics::Gdi::FW_BOLD;
use windows::Win32::Graphics::Gdi::LOGFONTW;
use windows::Win32::Graphics::Gdi::PROOF_QUALITY;
use windows::Win32::Graphics::Gdi::PS_SOLID;
use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::RegisterClassW;
use windows::Win32::UI::WindowsAndMessaging::SetLayeredWindowAttributes;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW;
use windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW;
use windows::Win32::UI::WindowsAndMessaging::LWA_COLORKEY;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE;

use komorebi_core::Rect;

use crate::window::Window;
use crate::windows_api::WindowsApi;
use crate::StackbarLabel;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::STACKBAR_FOCUSED_TEXT_COLOUR;
use crate::STACKBAR_LABEL;
use crate::STACKBAR_TAB_BACKGROUND_COLOUR;
use crate::STACKBAR_TAB_HEIGHT;
use crate::STACKBAR_TAB_WIDTH;
use crate::STACKBAR_UNFOCUSED_TEXT_COLOUR;
use crate::WINDOWS_BY_BAR_HWNDS;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Stackbar {
    pub(crate) hwnd: isize,
}

impl Stackbar {
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_LBUTTONDOWN => {
                let win_hwnds_by_topbar = WINDOWS_BY_BAR_HWNDS.lock();
                if let Some(win_hwnds) = win_hwnds_by_topbar.get(&hwnd.0) {
                    let x = l_param.0 as i32 & 0xFFFF;
                    let y = (l_param.0 as i32 >> 16) & 0xFFFF;

                    let width = STACKBAR_TAB_WIDTH.load(Ordering::SeqCst);
                    let height = STACKBAR_TAB_HEIGHT.load(Ordering::SeqCst);
                    let gap = DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst);

                    for (index, win_hwnd) in win_hwnds.iter().enumerate() {
                        let left = gap + (index as i32 * (width + gap));
                        let right = left + width;
                        let top = 0;
                        let bottom = height;

                        if x >= left && x <= right && y >= top && y <= bottom {
                            let window = Window { hwnd: *win_hwnd };
                            window.restore();
                            if let Err(err) = window.focus(false) {
                                tracing::error!("Stackbar focus error: HWND:{} {}", *win_hwnd, err);
                            }
                        }
                    }
                }

                WINDOWS_BY_BAR_HWNDS.force_unlock();
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, w_param, l_param),
        }
    }

    pub const fn hwnd(&self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn create() -> Result<Stackbar> {
        let name: Vec<u16> = "komorebi_stackbar\0".encode_utf16().collect();
        let class_name = PCWSTR(name.as_ptr());

        let h_module = WindowsApi::module_handle_w()?;

        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(Self::window_proc),
            hInstance: h_module.into(),
            lpszClassName: class_name,
            hbrBackground: WindowsApi::create_solid_brush(0),
            ..Default::default()
        };

        unsafe {
            RegisterClassW(&wnd_class);
        }

        let (hwnd_sender, hwnd_receiver) = crossbeam_channel::bounded::<HWND>(1);

        let name_cl = name.clone();
        std::thread::spawn(move || -> Result<()> {
            unsafe {
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                    PCWSTR(name_cl.as_ptr()),
                    PCWSTR(name_cl.as_ptr()),
                    WS_POPUP | WS_VISIBLE,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    h_module,
                    None,
                );

                SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_COLORKEY)?;
                hwnd_sender.send(hwnd)?;

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, hwnd, 0, 0).into() {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }

            Ok(())
        });

        Ok(Self {
            hwnd: hwnd_receiver.recv()?.0,
        })
    }

    pub fn set_position(&self, layout: &Rect, top: bool) -> Result<()> {
        WindowsApi::position_window(self.hwnd(), layout, top)
    }

    pub fn get_position_from_container_layout(&self, layout: &Rect) -> Rect {
        Rect {
            bottom: STACKBAR_TAB_HEIGHT.load(Ordering::SeqCst),
            ..*layout
        }
    }

    pub fn update(&self, windows: &VecDeque<Window>, focused_hwnd: isize) -> Result<()> {
        let width = STACKBAR_TAB_WIDTH.load(Ordering::SeqCst);
        let height = STACKBAR_TAB_HEIGHT.load(Ordering::SeqCst);
        let gap = DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst);
        let background = STACKBAR_TAB_BACKGROUND_COLOUR.load(Ordering::SeqCst);
        let focused_text_colour = STACKBAR_FOCUSED_TEXT_COLOUR.load(Ordering::SeqCst);
        let unfocused_text_colour = STACKBAR_UNFOCUSED_TEXT_COLOUR.load(Ordering::SeqCst);

        unsafe {
            let hdc = GetDC(self.hwnd());

            let hpen = CreatePen(PS_SOLID, 0, COLORREF(background));
            let hbrush = CreateSolidBrush(COLORREF(background));

            SelectObject(hdc, hpen);
            SelectObject(hdc, hbrush);
            SetBkColor(hdc, COLORREF(background));

            let hfont = CreateFontIndirectW(&LOGFONTW {
                lfWeight: FW_BOLD.0 as i32,
                lfQuality: FONT_QUALITY(PROOF_QUALITY.0),
                ..Default::default()
            });

            SelectObject(hdc, hfont);

            for (i, window) in windows.iter().enumerate() {
                if window.hwnd == focused_hwnd {
                    SetTextColor(hdc, COLORREF(focused_text_colour));
                } else {
                    SetTextColor(hdc, COLORREF(unfocused_text_colour));
                }

                let left = gap + (i as i32 * (width + gap));
                let mut tab_box = Rect {
                    top: 0,
                    left,
                    right: left + width,
                    bottom: height,
                };

                WindowsApi::round_rect(hdc, &tab_box, 8);

                let label = match STACKBAR_LABEL.load() {
                    StackbarLabel::Process => {
                        let exe = window.exe()?;
                        exe.trim_end_matches(".exe").to_string()
                    }
                    StackbarLabel::Title => window.title()?,
                };

                let mut tab_title: Vec<u16> = label.encode_utf16().collect();

                tab_box.left_padding(10);
                tab_box.right_padding(10);

                DrawTextW(
                    hdc,
                    &mut tab_title,
                    &mut tab_box.into(),
                    DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_END_ELLIPSIS,
                );
            }

            ReleaseDC(self.hwnd(), hdc);
        }

        let mut windows_hwdns: VecDeque<isize> = VecDeque::new();
        for window in windows {
            windows_hwdns.push_back(window.hwnd);
        }

        WINDOWS_BY_BAR_HWNDS.lock().insert(self.hwnd, windows_hwdns);

        Ok(())
    }

    pub fn hide(&self) {
        WindowsApi::hide_window(self.hwnd())
    }

    pub fn restore(&self) {
        WindowsApi::show_window(self.hwnd(), SW_SHOW)
    }
}
