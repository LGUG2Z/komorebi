use crate::border_manager::window_kind_colour;
use crate::border_manager::WindowKind;
use crate::border_manager::BORDER_OFFSET;
use crate::border_manager::BORDER_WIDTH;
use crate::border_manager::FOCUS_STATE;
use crate::border_manager::RENDER_TARGETS;
use crate::border_manager::STYLE;
use crate::border_manager::Z_ORDER;
use crate::core::BorderStyle;
use crate::core::Rect;
use crate::windows_api;
use crate::WindowsApi;
use crate::WINDOWS_11;
use std::ops::Deref;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::LazyLock;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Foundation::FALSE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::TRUE;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U;
use windows::Win32::Graphics::Direct2D::D2D1CreateFactory;
use windows::Win32::Graphics::Direct2D::ID2D1Factory;
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
use windows::Win32::Graphics::Direct2D::D2D1_BRUSH_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_FACTORY_TYPE_MULTI_THREADED;
use windows::Win32::Graphics::Direct2D::D2D1_HWND_RENDER_TARGET_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_PRESENT_OPTIONS_IMMEDIATELY;
use windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_TYPE_DEFAULT;
use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;
use windows::Win32::Graphics::Dwm::DwmEnableBlurBehindWindow;
use windows::Win32::Graphics::Dwm::DWM_BB_BLURREGION;
use windows::Win32::Graphics::Dwm::DWM_BB_ENABLE;
use windows::Win32::Graphics::Dwm::DWM_BLURBEHIND;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::Graphics::Gdi::BeginPaint;
use windows::Win32::Graphics::Gdi::CreateRectRgn;
use windows::Win32::Graphics::Gdi::EndPaint;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::Graphics::Gdi::PAINTSTRUCT;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_PAINT;
use windows::Win32::UI::WindowsAndMessaging::WM_SIZE;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows_core::PCWSTR;

#[allow(clippy::expect_used)]
static RENDER_FACTORY: LazyLock<ID2D1Factory> = unsafe {
    LazyLock::new(|| {
        D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_MULTI_THREADED, None)
            .expect("creating RENDER_FACTORY failed")
    })
};

static BRUSH_PROPERTIES: LazyLock<D2D1_BRUSH_PROPERTIES> =
    LazyLock::new(|| D2D1_BRUSH_PROPERTIES {
        opacity: 1.0,
        transform: Matrix3x2::identity(),
    });

pub extern "system" fn border_hwnds(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let hwnds = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
    let hwnd = hwnd.0 as isize;

    if let Ok(class) = WindowsApi::real_window_class_w(hwnd) {
        if class.starts_with("komoborder") {
            hwnds.push(hwnd);
        }
    }

    true.into()
}

#[derive(Debug)]
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
        HWND(windows_api::as_ptr!(self.hwnd))
    }

    pub fn create(id: &str) -> color_eyre::Result<Self> {
        let name: Vec<u16> = format!("komoborder-{id}\0").encode_utf16().collect();
        let class_name = PCWSTR(name.as_ptr());

        let h_module = WindowsApi::module_handle_w()?;

        let window_class = WNDCLASSW {
            hInstance: h_module.into(),
            lpszClassName: class_name,
            lpfnWndProc: Some(Self::callback),
            hbrBackground: WindowsApi::create_solid_brush(0),
            ..Default::default()
        };

        let _ = WindowsApi::register_class_w(&window_class);

        let (hwnd_sender, hwnd_receiver) = mpsc::channel();

        let instance = h_module.0 as isize;
        std::thread::spawn(move || -> color_eyre::Result<()> {
            let hwnd = WindowsApi::create_border_window(PCWSTR(name.as_ptr()), instance)?;
            hwnd_sender.send(hwnd)?;

            let mut msg: MSG = MSG::default();

            loop {
                unsafe {
                    if !GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                        tracing::debug!("border window event processing thread shutdown");
                        break;
                    };
                    // TODO: error handling
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            Ok(())
        });

        let hwnd = hwnd_receiver.recv()?;
        let border = Self { hwnd };

        // I have literally no idea, apparently this is to get rid of the black pixels
        // around the edges of rounded corners? @lukeyou05 borrowed this from PowerToys
        unsafe {
            let pos: i32 = -GetSystemMetrics(SM_CXVIRTUALSCREEN) - 8;
            let hrgn = CreateRectRgn(pos, 0, pos + 1, 1);
            let mut bh: DWM_BLURBEHIND = Default::default();
            if !hrgn.is_invalid() {
                bh = DWM_BLURBEHIND {
                    dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
                    fEnable: TRUE,
                    hRgnBlur: hrgn,
                    fTransitionOnMaximized: FALSE,
                };
            }

            let _ = DwmEnableBlurBehindWindow(border.hwnd(), &bh);
        }

        let hwnd_render_target_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: HWND(windows_api::as_ptr!(hwnd)),
            pixelSize: Default::default(),
            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
        };

        let render_target_properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_UNKNOWN,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };

        match unsafe {
            RENDER_FACTORY
                .CreateHwndRenderTarget(&render_target_properties, &hwnd_render_target_properties)
        } {
            Ok(render_target) => unsafe {
                render_target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                let mut render_targets = RENDER_TARGETS.lock();
                render_targets.insert(hwnd, render_target);
                Ok(border)
            },
            Err(error) => Err(error.into()),
        }
    }

    pub fn destroy(&self) -> color_eyre::Result<()> {
        let mut render_targets = RENDER_TARGETS.lock();
        render_targets.remove(&self.hwnd);
        WindowsApi::close_window(self.hwnd)
    }

    pub fn update(&self, rect: &Rect, should_invalidate: bool) -> color_eyre::Result<()> {
        // Make adjustments to the border
        let mut rect = *rect;
        rect.add_margin(BORDER_WIDTH.load(Ordering::Relaxed));
        rect.add_padding(-BORDER_OFFSET.load(Ordering::Relaxed));

        // Update the position of the border if required
        // This effectively handles WM_MOVE
        // Also if I remove this no borders render at all lol
        if !WindowsApi::window_rect(self.hwnd)?.eq(&rect) {
            WindowsApi::set_border_pos(self.hwnd, &rect, Z_ORDER.load().into())?;
        }

        // Invalidate the rect to trigger the callback to update colours etc.
        if should_invalidate {
            self.invalidate();
        }

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
                WM_SIZE | WM_PAINT => {
                    if let Ok(rect) = WindowsApi::window_rect(window.0 as isize) {
                        let render_targets = RENDER_TARGETS.lock();
                        if let Some(render_target) = render_targets.get(&(window.0 as isize)) {
                            let pixel_size = D2D_SIZE_U {
                                width: rect.right as u32,
                                height: rect.bottom as u32,
                            };

                            let border_width = BORDER_WIDTH.load(Ordering::SeqCst);
                            let border_offset = BORDER_OFFSET.load(Ordering::SeqCst);

                            let rect = D2D_RECT_F {
                                left: (border_width / 2 - border_offset) as f32,
                                top: (border_width / 2 - border_offset) as f32,
                                right: (rect.right - border_width / 2 + border_offset) as f32,
                                bottom: (rect.bottom - border_width / 2 + border_offset) as f32,
                            };

                            let _ = render_target.Resize(&pixel_size);

                            // Get window kind and color
                            let window_kind = FOCUS_STATE
                                .lock()
                                .get(&(window.0 as isize))
                                .copied()
                                .unwrap_or(WindowKind::Unfocused);

                            let color = window_kind_colour(window_kind);
                            let color = D2D1_COLOR_F {
                                r: ((color & 0xFF) as f32) / 255.0,
                                g: (((color >> 8) & 0xFF) as f32) / 255.0,
                                b: (((color >> 16) & 0xFF) as f32) / 255.0,
                                a: 1.0,
                            };

                            if let Ok(brush) = render_target
                                .CreateSolidColorBrush(&color, Some(BRUSH_PROPERTIES.deref()))
                            {
                                render_target.BeginDraw();
                                render_target.Clear(None);

                                // Calculate border radius based on style
                                let style = match STYLE.load() {
                                    BorderStyle::System => {
                                        if *WINDOWS_11 {
                                            BorderStyle::Rounded
                                        } else {
                                            BorderStyle::Square
                                        }
                                    }
                                    BorderStyle::Rounded => BorderStyle::Rounded,
                                    BorderStyle::Square => BorderStyle::Square,
                                };

                                match style {
                                    BorderStyle::Rounded => {
                                        let radius = 8.0 + border_width as f32 / 2.0;
                                        let rounded_rect = D2D1_ROUNDED_RECT {
                                            rect,
                                            radiusX: radius,
                                            radiusY: radius,
                                        };

                                        render_target.DrawRoundedRectangle(
                                            &rounded_rect,
                                            &brush,
                                            border_width as f32,
                                            None,
                                        );
                                    }
                                    BorderStyle::Square => {
                                        let rect = D2D_RECT_F {
                                            left: rect.left,
                                            top: rect.top,
                                            right: rect.right,
                                            bottom: rect.bottom,
                                        };

                                        render_target.DrawRectangle(
                                            &rect,
                                            &brush,
                                            border_width as f32,
                                            None,
                                        );
                                    }
                                    _ => {}
                                }

                                let _ = render_target.EndDraw(None, None);

                                // If we don't do this we'll get spammed with WM_PAINT according to Raymond Chen
                                // https://stackoverflow.com/questions/41783234/why-does-my-call-to-d2d1rendertargetdrawtext-result-in-a-wm-paint-being-se#comment70756781_41783234
                                let _ = BeginPaint(window, &mut PAINTSTRUCT::default());
                                let _ = EndPaint(window, &PAINTSTRUCT::default());
                            }
                        }
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
