use crate::border_manager::BORDER_OFFSET;
use crate::border_manager::BORDER_WIDTH;
use crate::border_manager::STYLE;
use crate::container::Container;
use crate::stackbar_manager::STACKBARS_CONTAINERS;
use crate::stackbar_manager::STACKBAR_FOCUSED_TEXT_COLOUR;
use crate::stackbar_manager::STACKBAR_LABEL;
use crate::stackbar_manager::STACKBAR_TAB_BACKGROUND_COLOUR;
use crate::stackbar_manager::STACKBAR_TAB_HEIGHT;
use crate::stackbar_manager::STACKBAR_TAB_WIDTH;
use crate::stackbar_manager::STACKBAR_UNFOCUSED_TEXT_COLOUR;
use crate::WindowsApi;
use crate::DEFAULT_CONTAINER_PADDING;
use crate::WINDOWS_11;
use crossbeam_utils::atomic::AtomicConsume;
use komorebi_core::BorderStyle;
use komorebi_core::Rect;
use komorebi_core::StackbarLabel;
use std::sync::mpsc;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Gdi::CreateFontIndirectW;
use windows::Win32::Graphics::Gdi::CreatePen;
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::Graphics::Gdi::DrawTextW;
use windows::Win32::Graphics::Gdi::GetDC;
use windows::Win32::Graphics::Gdi::Rectangle;
use windows::Win32::Graphics::Gdi::ReleaseDC;
use windows::Win32::Graphics::Gdi::RoundRect;
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
use windows::Win32::UI::WindowsAndMessaging::SetLayeredWindowAttributes;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW;
use windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW;
use windows::Win32::UI::WindowsAndMessaging::LWA_COLORKEY;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED;
use windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
use windows::Win32::UI::WindowsAndMessaging::WS_POPUP;
use windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE;

#[derive(Debug)]
pub struct Stackbar {
    pub hwnd: isize,
}

impl From<isize> for Stackbar {
    fn from(value: isize) -> Self {
        Self { hwnd: value }
    }
}

impl Stackbar {
    pub const fn hwnd(&self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn create(id: &str) -> color_eyre::Result<Self> {
        let name: Vec<u16> = format!("komostackbar-{id}\0").encode_utf16().collect();
        let class_name = PCWSTR(name.as_ptr());

        let h_module = WindowsApi::module_handle_w()?;

        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(Self::callback),
            hInstance: h_module.into(),
            lpszClassName: class_name,
            hbrBackground: WindowsApi::create_solid_brush(0),
            ..Default::default()
        };

        let _ = WindowsApi::register_class_w(&window_class);

        let (hwnd_sender, hwnd_receiver) = mpsc::channel();

        let name_cl = name.clone();
        std::thread::spawn(move || -> color_eyre::Result<()> {
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

    pub fn destroy(&self) -> color_eyre::Result<()> {
        WindowsApi::close_window(self.hwnd())
    }

    pub fn update(
        &self,
        container_padding: i32,
        container: &mut Container,
        layout: &Rect,
    ) -> color_eyre::Result<()> {
        let width = STACKBAR_TAB_WIDTH.load_consume();
        let height = STACKBAR_TAB_HEIGHT.load_consume();
        let gap = DEFAULT_CONTAINER_PADDING.load_consume();
        let background = STACKBAR_TAB_BACKGROUND_COLOUR.load_consume();
        let focused_text_colour = STACKBAR_FOCUSED_TEXT_COLOUR.load_consume();
        let unfocused_text_colour = STACKBAR_UNFOCUSED_TEXT_COLOUR.load_consume();

        let mut stackbars_containers = STACKBARS_CONTAINERS.lock();
        stackbars_containers.insert(self.hwnd, container.clone());

        let mut layout = *layout;
        let workspace_specific_offset =
            BORDER_WIDTH.load_consume() + BORDER_OFFSET.load_consume() + container_padding;

        layout.top -= workspace_specific_offset + STACKBAR_TAB_HEIGHT.load_consume();
        layout.left -= workspace_specific_offset;

        WindowsApi::position_window(self.hwnd(), &layout, false)?;

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

            for (i, window) in container.windows().iter().enumerate() {
                if window.hwnd == container.focused_window().copied().unwrap_or_default().hwnd {
                    SetTextColor(hdc, COLORREF(focused_text_colour));
                } else {
                    SetTextColor(hdc, COLORREF(unfocused_text_colour));
                }

                let left = gap + (i as i32 * (width + gap));
                let mut rect = Rect {
                    top: 0,
                    left,
                    right: left + width,
                    bottom: height,
                };

                match *STYLE.lock() {
                    BorderStyle::System => {
                        if *WINDOWS_11 {
                            RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 20, 20);
                        } else {
                            Rectangle(hdc, rect.left, rect.top, rect.right, rect.bottom);
                        }
                    }
                    BorderStyle::Rounded => {
                        RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 20, 20);
                    }
                    BorderStyle::Square => {
                        Rectangle(hdc, rect.left, rect.top, rect.right, rect.bottom);
                    }
                }

                let label = match STACKBAR_LABEL.load() {
                    StackbarLabel::Process => {
                        let exe = window.exe()?;
                        exe.trim_end_matches(".exe").to_string()
                    }
                    StackbarLabel::Title => window.title()?,
                };

                let mut tab_title: Vec<u16> = label.encode_utf16().collect();

                rect.left_padding(10);
                rect.right_padding(10);

                DrawTextW(
                    hdc,
                    &mut tab_title,
                    &mut rect.into(),
                    DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_END_ELLIPSIS,
                );
            }

            ReleaseDC(self.hwnd(), hdc);
            DeleteObject(hpen);
            DeleteObject(hbrush);
            DeleteObject(hfont);
        }

        Ok(())
    }

    pub fn get_position_from_container_layout(&self, layout: &Rect) -> Rect {
        Rect {
            bottom: STACKBAR_TAB_HEIGHT.load_consume(),
            ..*layout
        }
    }

    unsafe extern "system" fn callback(
        hwnd: HWND,
        msg: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        unsafe {
            match msg {
                WM_LBUTTONDOWN => {
                    let stackbars_containers = STACKBARS_CONTAINERS.lock();
                    if let Some(container) = stackbars_containers.get(&hwnd.0) {
                        let x = l_param.0 as i32 & 0xFFFF;
                        let y = (l_param.0 as i32 >> 16) & 0xFFFF;

                        let width = STACKBAR_TAB_WIDTH.load_consume();
                        let height = STACKBAR_TAB_HEIGHT.load_consume();
                        let gap = DEFAULT_CONTAINER_PADDING.load_consume();

                        let focused_window_idx = container.focused_window_idx();
                        let focused_window_rect = WindowsApi::window_rect(
                            container
                                .focused_window()
                                .cloned()
                                .unwrap_or_default()
                                .hwnd(),
                        )
                        .unwrap_or_default();

                        for (index, window) in container.windows().iter().enumerate() {
                            let left = gap + (index as i32 * (width + gap));
                            let right = left + width;
                            let top = 0;
                            let bottom = height;

                            if x >= left && x <= right && y >= top && y <= bottom {
                                // If we are focusing a window that isn't currently focused in the
                                // stackbar, make sure we update its location so that it doesn't render
                                // on top of other tiles before eventually ending up in the correct
                                // tile
                                if index != focused_window_idx {
                                    if let Err(err) =
                                        window.set_position(&focused_window_rect, false)
                                    {
                                        tracing::error!(
                                        "stackbar WM_LBUTTONDOWN repositioning error: hwnd {} ({})",
                                        *window,
                                        err
                                    );
                                    }
                                }

                                // Restore the window corresponding to the tab we have clicked
                                window.restore();
                                if let Err(err) = window.focus(false) {
                                    tracing::error!(
                                        "stackbar WMLBUTTONDOWN focus error: hwnd {} ({})",
                                        *window,
                                        err
                                    );
                                }
                            } else {
                                // Hide any windows in the stack that don't correspond to the window
                                // we have clicked
                                window.hide();
                            }
                        }
                    }

                    LRESULT(0)
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, w_param, l_param),
            }
        }
    }
}
