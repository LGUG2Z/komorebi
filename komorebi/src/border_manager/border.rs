use crate::WINDOWS_11;
use crate::WindowsApi;
use crate::border_manager::BORDER_OFFSET;
use crate::border_manager::BORDER_WIDTH;
use crate::border_manager::RenderTarget;
use crate::border_manager::STYLE;
use crate::border_manager::WindowKind;
use crate::border_manager::window_kind_colour;
use crate::core::BorderStyle;
use crate::core::Rect;
use crate::windows_api;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use windows::Win32::Foundation::FALSE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::Foundation::TRUE;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U;
use windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT;
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
use windows::Win32::Graphics::Direct2D::D2D1_BRUSH_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_FACTORY_TYPE_MULTI_THREADED;
use windows::Win32::Graphics::Direct2D::D2D1_HWND_RENDER_TARGET_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_PRESENT_OPTIONS_IMMEDIATELY;
use windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_TYPE_DEFAULT;
use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;
use windows::Win32::Graphics::Direct2D::D2D1CreateFactory;
use windows::Win32::Graphics::Direct2D::ID2D1Factory;
use windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush;
use windows::Win32::Graphics::Dwm::DWM_BB_BLURREGION;
use windows::Win32::Graphics::Dwm::DWM_BB_ENABLE;
use windows::Win32::Graphics::Dwm::DWM_BLURBEHIND;
use windows::Win32::Graphics::Dwm::DwmEnableBlurBehindWindow;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::Graphics::Gdi::CreateRectRgn;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_LOCATIONCHANGE;
use windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::IDC_ARROW;
use windows::Win32::UI::WindowsAndMessaging::LoadCursorW;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN;
use windows::Win32::UI::WindowsAndMessaging::SetCursor;
use windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::WM_CREATE;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_PAINT;
use windows::Win32::UI::WindowsAndMessaging::WM_SETCURSOR;
use windows::Win32::UI::WindowsAndMessaging::WM_USER;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows_core::BOOL;
use windows_core::PCWSTR;
use windows_numerics::Matrix3x2;

/// Custom WM_USER message that tells the border window thread to call update_brushes() on itself,
/// avoiding a data race between the border manager thread and the border's message loop thread.
pub const WM_UPDATE_BRUSHES: u32 = WM_USER + 1;

/// Custom WM_USER message used to drive the border in lockstep with an active
/// movement animation. The receiving WndProc takes the newest rect the animation
/// has for it from `border_manager::take_animated_rect` and applies it as the
/// new tracked rect.
pub const WM_ANIMATE_RECT: u32 = WM_USER + 2;

/// Custom WM_USER message that tells the border window thread to invalidate its own window, so
/// that it is painted after the window has been shown, which is done on this thread too when
/// windows are handled asynchronously.
pub const WM_INVALIDATE: u32 = WM_USER + 3;

pub struct RenderFactory(ID2D1Factory);
unsafe impl Sync for RenderFactory {}
unsafe impl Send for RenderFactory {}

impl Deref for RenderFactory {
    type Target = ID2D1Factory;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[allow(clippy::expect_used)]
static RENDER_FACTORY: LazyLock<RenderFactory> = unsafe {
    LazyLock::new(|| {
        RenderFactory(
            D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_MULTI_THREADED, None)
                .expect("creating RENDER_FACTORY failed"),
        )
    })
};

static BRUSH_PROPERTIES: LazyLock<D2D1_BRUSH_PROPERTIES> =
    LazyLock::new(|| D2D1_BRUSH_PROPERTIES {
        opacity: 1.0,
        transform: Matrix3x2::identity(),
    });

/// Apply a new tracked rect to the border on its own message-loop thread.
/// Updates `window_rect`, calls `set_position`, and re-renders if size/position
/// changed. Used by both `EVENT_OBJECT_LOCATIONCHANGE` (real window movements)
/// and `WM_ANIMATE_RECT` (animation-driven movements while the source is cloaked).
///
/// SAFETY: caller must ensure `border_pointer` is non-null, points to a live
/// `Border`, and that we are running on the border's WndProc thread.
unsafe fn apply_tracked_rect(border_pointer: *mut Border, rect: Rect) {
    unsafe {
        let reference_hwnd = (*border_pointer).tracking_hwnd;
        // what was last drawn, and not `window_rect`, which the border manager sets from its
        // own thread to where it found the window, whether the border has been drawn there or not
        let drawn = (*border_pointer).rendered_rect;
        (*border_pointer).window_rect = rect;

        if let Err(error) = (*border_pointer).set_position(&rect, reference_hwnd) {
            tracing::error!("failed to update border position {error}");
        }

        if !needs_redraw(&drawn, &rect) || (*border_pointer).render_target.is_none() {
            return;
        }

        // double-check destruction flag before rendering
        if (*border_pointer).is_destroying.load(Ordering::Acquire) {
            return;
        }

        let render_target = match (*border_pointer).render_target.as_ref() {
            Some(rt) => rt,
            None => return,
        };

        let border_width = (*border_pointer).width;
        let border_offset = (*border_pointer).offset;

        (*border_pointer).rounded_rect.rect = D2D_RECT_F {
            left: (border_width / 2 - border_offset) as f32,
            top: (border_width / 2 - border_offset) as f32,
            right: (rect.right - border_width / 2 + border_offset) as f32,
            bottom: (rect.bottom - border_width / 2 + border_offset) as f32,
        };

        let _ = render_target.Resize(&D2D_SIZE_U {
            width: rect.right as u32,
            height: rect.bottom as u32,
        });

        let window_kind = (*border_pointer).window_kind;
        let Some(brush) = (*border_pointer).brushes.get(&window_kind) else {
            return;
        };

        render_target.BeginDraw();
        render_target.Clear(None);

        let style = match (*border_pointer).style {
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
                render_target.DrawRoundedRectangle(
                    &(*border_pointer).rounded_rect,
                    brush,
                    border_width as f32,
                    None,
                );
            }
            BorderStyle::Square => {
                render_target.DrawRectangle(
                    &(*border_pointer).rounded_rect.rect,
                    brush,
                    border_width as f32,
                    None,
                );
            }
            _ => {}
        }

        let _ = render_target.EndDraw(None, None);
        (*border_pointer).rendered_rect = rect;
    }
}

/// Whether a border that was last drawn around `drawn` is to be drawn again for a window that
/// is at `rect`
fn needs_redraw(drawn: &Rect, rect: &Rect) -> bool {
    !(rect.is_same_size_as(drawn) && rect.has_same_position_as(drawn))
}

pub extern "system" fn border_hwnds(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let hwnds = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
    let hwnd = hwnd.0 as isize;

    if let Ok(class) = WindowsApi::real_window_class_w(hwnd)
        && class.starts_with("komoborder")
    {
        hwnds.push(hwnd);
    }

    true.into()
}

#[derive(Debug, Clone)]
pub struct Border {
    pub hwnd: isize,
    pub id: String,
    pub monitor_idx: Option<usize>,
    pub render_target: Option<RenderTarget>,
    pub tracking_hwnd: isize,
    pub window_rect: Rect,
    /// The rect that the border was last drawn around: nothing is drawn again for a window that
    /// is where it was drawn, whatever `window_rect` says
    pub rendered_rect: Rect,
    pub window_kind: WindowKind,
    pub style: BorderStyle,
    pub width: i32,
    pub offset: i32,
    pub brush_properties: D2D1_BRUSH_PROPERTIES,
    pub rounded_rect: D2D1_ROUNDED_RECT,
    pub brushes: HashMap<WindowKind, ID2D1SolidColorBrush>,
    pub is_destroying: Arc<AtomicBool>,
}

impl From<isize> for Border {
    fn from(value: isize) -> Self {
        Self {
            hwnd: value,
            id: String::new(),
            monitor_idx: None,
            render_target: None,
            tracking_hwnd: 0,
            window_rect: Rect::default(),
            rendered_rect: Rect::default(),
            window_kind: WindowKind::Unfocused,
            style: STYLE.load(),
            width: BORDER_WIDTH.load(Ordering::Relaxed),
            offset: BORDER_OFFSET.load(Ordering::Relaxed),
            brush_properties: D2D1_BRUSH_PROPERTIES::default(),
            rounded_rect: D2D1_ROUNDED_RECT::default(),
            brushes: HashMap::new(),
            is_destroying: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Border {
    pub const fn hwnd(&self) -> HWND {
        HWND(windows_api::as_ptr!(self.hwnd))
    }

    pub fn create(
        id: &str,
        tracking_hwnd: isize,
        monitor_idx: usize,
    ) -> color_eyre::Result<Box<Self>> {
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

        let (border_sender, border_receiver) = mpsc::channel();

        let instance = h_module.0 as isize;
        let container_id = id.to_owned();
        std::thread::spawn(move || -> color_eyre::Result<()> {
            let mut border = Self {
                hwnd: 0,
                id: container_id,
                monitor_idx: Some(monitor_idx),
                render_target: None,
                tracking_hwnd,
                window_rect: WindowsApi::window_rect(tracking_hwnd).unwrap_or_default(),
                rendered_rect: Rect::default(),
                window_kind: WindowKind::Unfocused,
                style: STYLE.load(),
                width: BORDER_WIDTH.load(Ordering::Relaxed),
                offset: BORDER_OFFSET.load(Ordering::Relaxed),
                brush_properties: Default::default(),
                rounded_rect: Default::default(),
                brushes: HashMap::new(),
                is_destroying: Arc::new(AtomicBool::new(false)),
            };

            let border_pointer = &raw mut border;
            let hwnd =
                WindowsApi::create_border_window(PCWSTR(name.as_ptr()), instance, border_pointer)?;

            let boxed = unsafe {
                (*border_pointer).hwnd = hwnd;
                Box::from_raw(border_pointer)
            };
            border_sender.send(boxed)?;

            let mut msg: MSG = MSG::default();

            loop {
                unsafe {
                    if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
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

        let mut border = border_receiver.recv()?;
        tracing::trace!(
            "border {}: created for {id} to track {tracking_hwnd}",
            border.hwnd
        );

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

        border.update_brushes()?;

        Ok(border)
    }

    pub fn update_brushes(&mut self) -> color_eyre::Result<()> {
        let hwnd_render_target_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: HWND(windows_api::as_ptr!(self.hwnd)),
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
                self.brush_properties = *BRUSH_PROPERTIES.deref();
                for window_kind in [
                    WindowKind::Single,
                    WindowKind::Stack,
                    WindowKind::Monocle,
                    WindowKind::Unfocused,
                    WindowKind::Floating,
                    WindowKind::UnfocusedLocked,
                ] {
                    let color = window_kind_colour(window_kind);
                    let color = D2D1_COLOR_F {
                        r: ((color & 0xFF) as f32) / 255.0,
                        g: (((color >> 8) & 0xFF) as f32) / 255.0,
                        b: (((color >> 16) & 0xFF) as f32) / 255.0,
                        a: 1.0,
                    };

                    if let Ok(brush) =
                        render_target.CreateSolidColorBrush(&color, Some(&self.brush_properties))
                    {
                        self.brushes.insert(window_kind, brush);
                    }
                }

                render_target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

                self.render_target = Some(RenderTarget(render_target));

                self.rounded_rect = {
                    let radius = 8.0 + self.width as f32 / 2.0;
                    D2D1_ROUNDED_RECT {
                        rect: Default::default(),
                        radiusX: radius,
                        radiusY: radius,
                    }
                };

                Ok(())
            },
            Err(error) => Err(error.into()),
        }
    }

    pub fn destroy(&self) -> color_eyre::Result<()> {
        // signal that we're destroying - prevents new render operations from starting
        self.is_destroying.store(true, Ordering::Release);

        // small delay to allow in-flight render operations to complete
        std::thread::sleep(std::time::Duration::from_millis(10));

        // WM_DESTROY will clear GWLP_USERDATA and drop the render target before D2D
        // frees its internal HwndPresenter during WM_NCDESTROY
        WindowsApi::close_window(self.hwnd)
    }

    /// Post a message to the border's own message loop thread requesting a brush update.
    /// This ensures update_brushes() always runs on the window thread that owns the D2D
    /// render target, preventing a data race with concurrent WndProc render operations.
    pub fn request_brush_update(&self) {
        let _ = unsafe {
            PostMessageW(
                Option::from(self.hwnd()),
                WM_UPDATE_BRUSHES,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }

    pub fn set_position(&self, rect: &Rect, reference_hwnd: isize) -> color_eyre::Result<()> {
        let mut rect = *rect;
        rect.add_margin(self.width);
        rect.add_padding(-self.offset);

        // a border that is held is put where it is to be and left hidden: it is
        // shown along with its window, not when it is created
        let show = !crate::border_manager::is_held(reference_hwnd);
        tracing::trace!(
            "border {}: set to {rect:?} above {reference_hwnd}, shown: {show}",
            self.hwnd
        );
        WindowsApi::set_border_pos(self.hwnd, &rect, reference_hwnd, show)?;

        Ok(())
    }

    // this triggers WM_PAINT in the callback below, from the thread of the border's window: a
    // window that is shown asynchronously is shown by that thread, and what is invalidated before
    // it is shown is not painted when it is, as the show does not redraw it
    pub fn invalidate(&self) {
        let _ = unsafe {
            PostMessageW(
                Option::from(self.hwnd()),
                WM_INVALIDATE,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }

    pub extern "system" fn callback(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            match message {
                WM_SETCURSOR => match LoadCursorW(None, IDC_ARROW) {
                    Ok(cursor) => {
                        SetCursor(Some(cursor));
                        LRESULT(0)
                    }
                    Err(error) => {
                        tracing::error!("{error}");
                        LRESULT(1)
                    }
                },
                WM_CREATE => {
                    let mut border_pointer: *mut Border =
                        GetWindowLongPtrW(window, GWLP_USERDATA) as _;

                    if border_pointer.is_null() {
                        let create_struct: *mut CREATESTRUCTW = lparam.0 as *mut _;
                        border_pointer = (*create_struct).lpCreateParams as *mut _;
                        SetWindowLongPtrW(window, GWLP_USERDATA, border_pointer as _);
                    }

                    LRESULT(0)
                }
                EVENT_OBJECT_DESTROY => {
                    let border_pointer: *mut Border = GetWindowLongPtrW(window, GWLP_USERDATA) as _;

                    if border_pointer.is_null() {
                        return LRESULT(0);
                    }

                    // we don't actually want to destroy the window here, just hide it for quicker
                    // visual feedback to the user; the actual destruction will be handled by the
                    // core border manager loop
                    WindowsApi::hide_window(window.0 as isize);
                    LRESULT(0)
                }
                EVENT_OBJECT_LOCATIONCHANGE => {
                    let border_pointer: *mut Border = GetWindowLongPtrW(window, GWLP_USERDATA) as _;

                    if border_pointer.is_null() {
                        return LRESULT(0);
                    }

                    if (*border_pointer).is_destroying.load(Ordering::Acquire) {
                        return LRESULT(0);
                    }

                    let reference_hwnd = (*border_pointer).tracking_hwnd;

                    // The window is hidden while a ghost stands in for it, and
                    // is not where the ghost is, which is where the border
                    // belongs: the animation moves it.
                    if crate::border_manager::leaves_border_alone(reference_hwnd) {
                        tracing::trace!(
                            "border: left the border of hwnd {reference_hwnd} to its ghost's animation"
                        );
                        return LRESULT(0);
                    }

                    let rect = WindowsApi::window_rect(reference_hwnd).unwrap_or_default();
                    apply_tracked_rect(border_pointer, rect);
                    LRESULT(0)
                }
                WM_ANIMATE_RECT => {
                    // The newest rect there is, which is all there is to draw
                    // if the animation sent more than one since the last time.
                    let Some(rect) = crate::border_manager::take_animated_rect(window.0 as isize)
                    else {
                        return LRESULT(0);
                    };

                    let border_pointer: *mut Border = GetWindowLongPtrW(window, GWLP_USERDATA) as _;

                    if border_pointer.is_null() {
                        return LRESULT(0);
                    }

                    if (*border_pointer).is_destroying.load(Ordering::Acquire) {
                        return LRESULT(0);
                    }

                    apply_tracked_rect(border_pointer, rect);
                    LRESULT(0)
                }
                WM_PAINT => {
                    tracing::trace!("border {}: WM_PAINT", window.0 as isize);
                    if let Err(error) = WindowsApi::window_rect(window.0 as isize) {
                        tracing::trace!(
                            "border {}: not painted, its own rect cannot be read: {error}",
                            window.0 as isize
                        );
                    }

                    if let Ok(rect) = WindowsApi::window_rect(window.0 as isize) {
                        let border_pointer: *mut Border =
                            GetWindowLongPtrW(window, GWLP_USERDATA) as _;

                        if border_pointer.is_null() {
                            tracing::trace!(
                                "border {}: not painted, it has no border data",
                                window.0 as isize
                            );
                            return LRESULT(0);
                        }

                        if (*border_pointer).is_destroying.load(Ordering::Acquire) {
                            tracing::trace!(
                                "border {}: not painted, it is being destroyed",
                                window.0 as isize
                            );
                            return LRESULT(0);
                        }

                        let reference_hwnd = (*border_pointer).tracking_hwnd;

                        // Update position to update the ZOrder
                        let border_window_rect = (*border_pointer).window_rect;

                        tracing::trace!("updating border position");
                        if let Err(error) =
                            (*border_pointer).set_position(&border_window_rect, reference_hwnd)
                        {
                            tracing::error!("failed to update border position {error}");
                        }

                        if (*border_pointer).render_target.is_none() {
                            tracing::trace!(
                                "border {}: not painted, it has no render target",
                                window.0 as isize
                            );
                        }

                        if let Some(render_target) = (*border_pointer).render_target.as_ref() {
                            // double-check destruction flag before rendering
                            if (*border_pointer).is_destroying.load(Ordering::Acquire) {
                                tracing::trace!(
                                    "border {}: not painted, it is being destroyed",
                                    window.0 as isize
                                );
                                return LRESULT(0);
                            }

                            (*border_pointer).width = BORDER_WIDTH.load(Ordering::Relaxed);
                            (*border_pointer).offset = BORDER_OFFSET.load(Ordering::Relaxed);

                            let border_width = (*border_pointer).width;
                            let border_offset = (*border_pointer).offset;

                            (*border_pointer).rounded_rect.rect = D2D_RECT_F {
                                left: (border_width / 2 - border_offset) as f32,
                                top: (border_width / 2 - border_offset) as f32,
                                right: (rect.right - border_width / 2 + border_offset) as f32,
                                bottom: (rect.bottom - border_width / 2 + border_offset) as f32,
                            };

                            let _ = render_target.Resize(&D2D_SIZE_U {
                                width: rect.right as u32,
                                height: rect.bottom as u32,
                            });

                            // Get window kind and color
                            let window_kind = (*border_pointer).window_kind;
                            if !(*border_pointer).brushes.contains_key(&window_kind) {
                                tracing::trace!(
                                    "border {}: not painted, it has no brush for {window_kind:?}",
                                    window.0 as isize
                                );
                            }

                            if let Some(brush) = (*border_pointer).brushes.get(&window_kind) {
                                tracing::trace!(
                                    "border {}: painting {window_kind:?}",
                                    window.0 as isize
                                );
                                render_target.BeginDraw();
                                render_target.Clear(None);

                                (*border_pointer).style = STYLE.load();

                                // Calculate border radius based on style
                                let style = match (*border_pointer).style {
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
                                        render_target.DrawRoundedRectangle(
                                            &(*border_pointer).rounded_rect,
                                            brush,
                                            border_width as f32,
                                            None,
                                        );
                                    }
                                    BorderStyle::Square => {
                                        render_target.DrawRectangle(
                                            &(*border_pointer).rounded_rect.rect,
                                            brush,
                                            border_width as f32,
                                            None,
                                        );
                                    }
                                    _ => {}
                                }

                                let _ = render_target.EndDraw(None, None);
                            }
                        }
                    }
                    let _ = ValidateRect(Option::from(window), None);
                    LRESULT(0)
                }
                WM_INVALIDATE => {
                    let _ = InvalidateRect(Option::from(window), None, false);
                    LRESULT(0)
                }
                WM_UPDATE_BRUSHES => {
                    let border_pointer: *mut Border = GetWindowLongPtrW(window, GWLP_USERDATA) as _;
                    if border_pointer.is_null() {
                        return LRESULT(0);
                    }
                    if (*border_pointer).is_destroying.load(Ordering::Acquire) {
                        return LRESULT(0);
                    }
                    if let Err(error) = (*border_pointer).update_brushes() {
                        tracing::error!("failed to update brushes: {error}");
                    }
                    (*border_pointer).invalidate();
                    LRESULT(0)
                }
                WM_DESTROY => {
                    let border_pointer: *mut Border = GetWindowLongPtrW(window, GWLP_USERDATA) as _;
                    if !border_pointer.is_null() {
                        (*border_pointer).render_target = None;
                        (*border_pointer).brushes.clear();
                        SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                    }
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(window, message, wparam, lparam),
            }
        }
    }
}

/// Tell a border, from any thread, that the animation has a rect for it to
/// draw, which it takes with `border_manager::take_animated_rect`.
pub fn post_animated_rect(border_hwnd: isize) {
    let _ = unsafe {
        PostMessageW(
            Option::from(HWND(border_hwnd as *mut std::ffi::c_void)),
            WM_ANIMATE_RECT,
            WPARAM(0),
            LPARAM(0),
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, width: i32, height: i32) -> Rect {
        Rect {
            left,
            top,
            right: width,
            bottom: height,
        }
    }

    #[test]
    fn a_border_is_drawn_again_for_a_window_that_is_not_where_it_was_drawn() {
        let drawn = rect(1310, 37, 2500, 2086);

        assert!(!needs_redraw(&drawn, &rect(1310, 37, 2500, 2086)));
        assert!(needs_redraw(&drawn, &rect(1210, 37, 2700, 2086)), "bigger");
        assert!(needs_redraw(&drawn, &rect(1210, 37, 2500, 2086)), "moved");
        assert!(needs_redraw(&drawn, &rect(1310, 37, 2500, 2000)), "shorter");

        // a border that has been drawn nowhere is drawn for every window
        assert!(needs_redraw(&Rect::default(), &drawn));
    }

    /// Whether a border that is made and shown the way the border manager does is painted
    #[allow(clippy::expect_used)]
    fn is_painted(handling: crate::WindowHandlingBehaviour) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::CreateWindowExW;
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
        use windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE;
        use windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW;
        use windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE;
        use windows::core::w;

        let tracked = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("komorebi-border-test"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                200,
                200,
                400,
                300,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a window to put a border around");

        let tracked_hwnd = tracked.0 as isize;
        let previous = crate::WINDOW_HANDLING_BEHAVIOUR.load();
        crate::WINDOW_HANDLING_BEHAVIOUR.store(handling);

        let mut border =
            Border::create("komorebi-border-test", tracked_hwnd, 0).expect("a border to paint");
        let rect = rect(200, 200, 400, 300);
        border.window_rect = rect;

        // as the border manager makes a border for a window
        border
            .set_position(&rect, tracked_hwnd)
            .expect("a border to put in place");
        border.invalidate();

        std::thread::sleep(std::time::Duration::from_millis(500));

        // the paint sizes what it draws to its window, which it does for nothing else
        let painted =
            unsafe { std::ptr::read_volatile(&raw const border.rounded_rect.rect.right) } > 0.0;

        crate::WINDOW_HANDLING_BEHAVIOUR.store(previous);

        let border = Box::into_raw(border);
        unsafe {
            let _ = (*border).destroy();
            let _ = DestroyWindow(tracked);
        }

        painted
    }

    #[test]
    fn a_new_border_is_painted_when_windows_are_handled_synchronously() {
        assert!(is_painted(crate::WindowHandlingBehaviour::Sync));
    }

    #[test]
    fn a_new_border_is_painted_when_windows_are_handled_asynchronously() {
        assert!(is_painted(crate::WindowHandlingBehaviour::Async));
    }
}
