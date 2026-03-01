use crate::as_ptr;
use crate::monitor::Monitor;
use crate::windows_api;
use crate::workspace::Workspace;
use crate::WindowManager;
use crate::WindowsApi;

use komorebi_layouts::Rect;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct2D::D2D1_BITMAP_OPTIONS_CANNOT_DRAW;
use windows::Win32::Graphics::Direct2D::D2D1_BITMAP_OPTIONS_TARGET;
use windows::Win32::Graphics::Direct2D::D2D1_BITMAP_PROPERTIES1;
use windows::Win32::Graphics::Direct2D::D2D1_DEVICE_CONTEXT_OPTIONS_NONE;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;
use windows::Win32::Graphics::Direct2D::ID2D1Device;
use windows::Win32::Graphics::Direct2D::ID2D1DeviceContext;
use windows::Win32::Graphics::Direct2D::ID2D1Factory1;
use windows::Win32::Graphics::Direct2D::ID2D1Factory2;
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_10_0;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_10_1;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_1;
use windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_BGRA_SUPPORT;
use windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_DEBUG;
use windows::Win32::Graphics::Direct3D11::D3D11_SDK_VERSION;
use windows::Win32::Graphics::Direct3D11::D3D11CreateDevice;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;
use windows::Win32::Graphics::DirectComposition::DCompositionCreateDevice2;
use windows::Win32::Graphics::DirectComposition::IDCompositionDevice;
use windows::Win32::Graphics::DirectComposition::IDCompositionSurface;
use windows::Win32::Graphics::DirectComposition::IDCompositionTarget;
use windows::Win32::Graphics::DirectComposition::IDCompositionVisual;
use windows::Win32::Graphics::Dxgi::Common::DXGI_ALPHA_MODE_PREMULTIPLIED;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows_future::AsyncActionCompletedHandler;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use windows::Graphics::DirectX::Direct3D11::IDirect3DSurface;
use windows::Win32::Foundation::S_FALSE;
use windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR;
use windows::Win32::Graphics::Direct2D::D2D1_BITMAP_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_BITMAPSOURCE_INTERPOLATION_MODE;
use windows::Win32::Graphics::Direct2D::D2D1_BITMAPSOURCE_INTERPOLATION_MODE_NEAREST_NEIGHBOR;
use windows::Win32::Graphics::Direct2D::D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR;
use windows::Win32::Graphics::Direct3D11::D3D11_RESOURCE_MISC_SHARED;
use windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC;
use windows::Win32::Graphics::Direct3D11::D3D11_USAGE_DEFAULT;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIResource;
use windows::Win32::System::Com::CoIncrementMTAUsage;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::System::WinRT::CreateDispatcherQueueController;
use windows::Win32::System::WinRT::DQTAT_COM_NONE;
use windows::Win32::System::WinRT::DQTYPE_THREAD_CURRENT;
use windows::Win32::System::WinRT::DispatcherQueueOptions;
use windows::Win32::System::WinRT::RO_INIT_MULTITHREADED;
use windows::Win32::System::WinRT::RoInitialize;
use windows_capture::capture::CaptureControl;
use windows_capture::capture::Context;
use windows_capture::capture::GraphicsCaptureApiError;
use windows_capture::graphics_capture_api::GraphicsCaptureApi;
use windows_capture::settings::GraphicsCaptureItemType;
use windows_capture::window::Window;
use std::collections::HashMap;
use std::io;
use std::io::Write;
use std::ops::Deref;
use std::os::raw::c_void;
use std::os::windows::raw::HANDLE;
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Instant;
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
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Direct2D::ID2D1Factory;
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush;
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
use windows::Win32::Graphics::Direct2D::D2D1_BRUSH_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_FACTORY_TYPE_MULTI_THREADED;
use windows::Win32::Graphics::Direct2D::D2D1_HWND_RENDER_TARGET_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_PRESENT_OPTIONS_IMMEDIATELY;
use windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_TYPE_DEFAULT;
use windows::Win32::Graphics::Dwm::DwmEnableBlurBehindWindow;
use windows::Win32::Graphics::Dwm::DWM_BB_BLURREGION;
use windows::Win32::Graphics::Dwm::DWM_BB_ENABLE;
use windows::Win32::Graphics::Dwm::DWM_BLURBEHIND;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows::Win32::Graphics::Gdi::CreateRectRgn;
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
use windows::Win32::UI::WindowsAndMessaging::DispatchMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetMessageW;
use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
use windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
use windows::Win32::UI::WindowsAndMessaging::TranslateMessage;
use windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
use windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA;
use windows::Win32::UI::WindowsAndMessaging::MSG;
use windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::Win32::UI::WindowsAndMessaging::SW_MAXIMIZE;
use windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE;
use windows::Win32::UI::WindowsAndMessaging::WM_CREATE;
use windows::Win32::UI::WindowsAndMessaging::WM_DESTROY;
use windows::Win32::UI::WindowsAndMessaging::WM_PAINT;
use windows::Win32::UI::WindowsAndMessaging::WM_SETCURSOR;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;
use windows_capture::capture::GraphicsCaptureApiHandler;
use windows_capture::settings::Settings;
use windows_core::Interface;
use windows_core::PCWSTR;
use windows_numerics::Matrix3x2;

pub struct RenderFactory(ID2D1Factory1);
unsafe impl Sync for RenderFactory {}
unsafe impl Send for RenderFactory {}

#[derive(Debug, Clone)]
pub struct IDCompositionDeviceLocal(IDCompositionDevice);
unsafe impl Sync for IDCompositionDeviceLocal {}
unsafe impl Send for IDCompositionDeviceLocal {}

#[derive(Debug, Clone)]
pub struct IDCompositionTargetLocal(IDCompositionTarget);
unsafe impl Sync for IDCompositionTargetLocal {}
unsafe impl Send for IDCompositionTargetLocal {}

#[derive(Debug, Clone)]
pub struct IDCompositionVisualLocal(IDCompositionVisual);
unsafe impl Sync for IDCompositionVisualLocal {}
unsafe impl Send for IDCompositionVisualLocal {}

#[derive(Debug, Clone)]
pub struct IDCompositionSurfaceLocal(IDCompositionSurface);
unsafe impl Sync for IDCompositionSurfaceLocal {}
unsafe impl Send for IDCompositionSurfaceLocal {}

impl Deref for RenderFactory {
    type Target = ID2D1Factory1;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

struct D3DResources {
    pub device: ID3D11Device,
    pub device_context: ID3D11DeviceContext,
}

#[allow(clippy::expect_used)]
static RENDER_FACTORY: LazyLock<RenderFactory> = unsafe {
    LazyLock::new(|| {
        RenderFactory(
            D2D1CreateFactory::<ID2D1Factory1>(D2D1_FACTORY_TYPE_MULTI_THREADED, None)
                .expect("creating RENDER_FACTORY failed"),
        )
    })
};

fn create_d3d_device() -> color_eyre::Result<D3DResources> {
    let mut feature_levels = [
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];

    let mut device = None;
    let mut context = None;
    let mut feature_level = D3D_FEATURE_LEVEL(0);

    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE(null_mut()),
            D3D11_CREATE_DEVICE_DEBUG | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&mut feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )?;
    }



    Ok(D3DResources {
        device: device.unwrap(),
        device_context: context.unwrap(),
    })
}

#[derive(Debug, Clone)]
struct D2DResources {
    // render_target: ID2D1RenderTarget,
    // brush: ID2D1SolidColorBrush,
    device: ID2D1Device,
    context: ID2D1DeviceContext,
    target: Option<ID2D1Bitmap1>,
    brush: ID2D1SolidColorBrush,
}


unsafe fn create_d2d_resources(dxgi_device: &IDXGIDevice) -> color_eyre::Result<D2DResources> {
        /* let render_target_properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };

    let render_target: ID2D1RenderTarget =
        RENDER_FACTORY.CreateDxgiSurfaceRenderTarget(dxgi_surface, &render_target_properties)?;
    println!("D2D render target created"); */

    let d2d_device = RENDER_FACTORY.CreateDevice(dxgi_device)?;
    println!("D2D device created"); 

    // Create brush
    /* let brush = render_target.CreateSolidColorBrush(
        &D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        },
        None,
    )?; */

    // println!("D2D brush created");

    let d2d_context = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
    let brush = d2d_context.CreateSolidColorBrush(
        &D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        },
        None,
    )?;
        
    Ok(D2DResources {
        device: d2d_device,
        context: d2d_context,
        target: None,
        brush,
    })
}



#[allow(clippy::expect_used)]
static D3D_DEVICE: LazyLock<D3DResources> = {
    LazyLock::new(|| {
        create_d3d_device().expect("creating D3D_DEVICE failed")
    })
};

#[derive(Debug, Clone)]
pub struct RenderTarget(pub ID2D1RenderTarget);
unsafe impl Send for RenderTarget {}

impl Deref for RenderTarget {
    type Target = ID2D1RenderTarget;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceSwitchWindow {
    pub hwnd: isize,
    pub monitor_id: Option<isize>,
    pub monitor_rect: Option<Rect>,
    pub d3d_device: Option<ID3D11Device>,
    pub d3d_device_context: Option<ID3D11DeviceContext>,
    pub device: Option<IDCompositionDeviceLocal>,
    pub visual: Option<IDCompositionVisualLocal>,
    pub surface: Option<IDCompositionSurfaceLocal>,
    pub target: Option<IDCompositionTargetLocal>,
    pub d2d_resources: Option<D2DResources>,
    pub brush_properties: D2D1_BRUSH_PROPERTIES,
    pub capture_hash_map: HashMap<usize, Arc<Mutex<Capture>>>,
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub hwnd: isize,
    pub last_frame: Arc<Mutex<Option<ID3D11Texture2D>>>,
    pub need_stop: Arc<Mutex<bool>>,
}

trait CustomCaptureApiHandler: GraphicsCaptureApiHandler {
    fn start_free_threaded_with_item<T: TryInto<GraphicsCaptureItemType> + Send + 'static>(
        item: T,
        d3d_device: ID3D11Device,
        d3d_device_context: ID3D11DeviceContext,
        settings: Settings<Self::Flags, T>,
    ) -> Result<CaptureControl<Self, Self::Error>, GraphicsCaptureApiError<Self::Error>>
    where
        Self: Send + 'static,
        <Self as GraphicsCaptureApiHandler>::Flags: Send;
}

impl CustomCaptureApiHandler for Capture {
    fn start_free_threaded_with_item<T: TryInto<GraphicsCaptureItemType> + Send + 'static>(
        item: T,
        d3d_device: ID3D11Device,
        d3d_device_context: ID3D11DeviceContext,
        settings: Settings<Self::Flags, T>,
    ) -> Result<CaptureControl<Self, Self::Error>, GraphicsCaptureApiError<Self::Error>>
    where
        Self: Send + 'static,
        <Self as GraphicsCaptureApiHandler>::Flags: Send,
    {
        let (halt_sender, halt_receiver) = mpsc::channel::<Arc<AtomicBool>>();
        let (callback_sender, callback_receiver) = mpsc::channel::<Arc<Mutex<Self>>>();

        let thread_handle = std::thread::spawn(move || -> Result<(), GraphicsCaptureApiError<Self::Error>> {
            // Initialize WinRT
            static INIT_MTA: OnceLock<()> = OnceLock::new();
            INIT_MTA.get_or_init(|| {
                unsafe {
                    CoIncrementMTAUsage().expect("Failed to increment MTA usage");
                };
            });

            match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
                Ok(_) => (),
                Err(e) => {
                    if e.code() == S_FALSE {
                        // Already initialized
                    } else {
                        return Err(GraphicsCaptureApiError::FailedToInitWinRT);
                    }
                }
            }

            // Create a dispatcher queue for the current thread
            let options = DispatcherQueueOptions {
                dwSize: u32::try_from(std::mem::size_of::<DispatcherQueueOptions>()).unwrap(),
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            };
            let controller = unsafe {
                CreateDispatcherQueueController(options)
                    .map_err(|_| GraphicsCaptureApiError::FailedToCreateDispatcherQueueController)?
            };

            // Get current thread ID
            let thread_id = unsafe { GetCurrentThreadId() };

            // Create direct3d device and context
            // let (d3d_device, d3d_device_context) = create_d3d_device()?;

            // Start capture
            let result = Arc::new(Mutex::new(None));

            let flags = settings.flags();

            let ctx = Context {
                flags: flags.clone(),
                device: d3d_device.clone(),
                device_context: d3d_device_context.clone(),
            };

            let callback = Arc::new(Mutex::new(Self::new(ctx).map_err(GraphicsCaptureApiError::NewHandlerError)?));

            // let item = settings.item().clone();

            let mut capture = GraphicsCaptureApi::new(
                d3d_device,
                d3d_device_context,
                item.try_into().map_err(|_| GraphicsCaptureApiError::ItemConvertFailed)?,
                callback.clone(),
                settings.cursor_capture(),
                settings.draw_border(),
                windows_capture::settings::SecondaryWindowSettings::Default,
                windows_capture::settings::MinimumUpdateIntervalSettings::Default,
                windows_capture::settings::DirtyRegionSettings::Default,
                windows_capture::settings::ColorFormat::Rgba8,
                thread_id,
                result.clone(),
            )
            .map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;

            capture.start_capture().map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;

            // Send halt handle
            let halt_handle = capture.halt_handle();
            halt_sender.send(halt_handle).unwrap();

            // Send callback
            callback_sender.send(callback).unwrap();

            // Message loop
            let mut message = MSG::default();
            unsafe {
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }

            // Shutdown dispatcher queue
            let async_action = controller
                .ShutdownQueueAsync()
                .map_err(|_| GraphicsCaptureApiError::FailedToShutdownDispatcherQueue)?;

            async_action
                .SetCompleted(&AsyncActionCompletedHandler::new(move |_, _| -> Result<(), windows::core::Error> {
                    unsafe { PostQuitMessage(0) };
                    Ok(())
                }))
                .map_err(|_| GraphicsCaptureApiError::FailedToSetDispatcherQueueCompletedHandler)?;

            // Final message loop
            let mut message = MSG::default();
            unsafe {
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }

            // Stop capture
            capture.stop_capture();

            // Uninitialize WinRT
            // unsafe { RoUninitialize() }; // Not sure if this is needed here

            // Check handler result
            let result = result.lock().take();
            if let Some(e) = result {
                return Err(GraphicsCaptureApiError::FrameHandlerError(e));
            }

            Ok(())
        });

        let Ok(halt_handle) = halt_receiver.recv() else {
            match thread_handle.join() {
                Ok(result) => return Err(result.err().unwrap()),
                Err(_) => {
                    return Err(GraphicsCaptureApiError::FailedToJoinThread);
                }
            }
        };

        let Ok(callback) = callback_receiver.recv() else {
            match thread_handle.join() {
                Ok(result) => return Err(result.err().unwrap()),
                Err(_) => {
                    return Err(GraphicsCaptureApiError::FailedToJoinThread);
                }
            }
        };

        Ok(CaptureControl::new(thread_handle, halt_handle, callback))
    }

}

impl windows_capture::capture::GraphicsCaptureApiHandler for Capture {
    type Flags = (i32, i32, isize);
    type Error = color_eyre::Report;

    fn new(ctx: windows_capture::capture::Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            hwnd: ctx.flags.2,
            last_frame: Arc::new(Mutex::new(None)),
            need_stop: Arc::new(Mutex::new(false)),
        })
    }


    // Called every time a new frame is available.
    fn on_frame_arrived(
        &mut self,
        frame: &mut windows_capture::frame::Frame,
        capture_control: windows_capture::graphics_capture_api::InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if *self.need_stop.lock() {
            capture_control.stop();
            return Ok(());
        }
        let  texture = frame.as_raw_texture();
        self.last_frame.lock().replace(texture.clone());
        /* if let Some(render_target) = self.render_target.lock().as_ref() {
            let mut bitmap: Option<ID2D1Bitmap> = None;
            unsafe {
                // let mut surface = frame.as_raw_surface();
        //         let mut desc = D3D11_TEXTURE2D_DESC::default();
        //          texture.GetDesc(&mut desc);
        //         println!("desc: {desc:?}");
        //         
        // let texture_desc = D3D11_TEXTURE2D_DESC {
        //     Width: desc.Width,
        //     Height: desc.Height,
        //     MipLevels: desc.MipLevels,
        //     ArraySize: desc.ArraySize,
        //             Format: desc.Format,
        //     SampleDesc: desc.SampleDesc,
        //     Usage: D3D11_USAGE_DEFAULT,
        //     BindFlags: desc.BindFlags,
        //     CPUAccessFlags: 0,
        //     MiscFlags: desc.MiscFlags | D3D11_RESOURCE_MISC_SHARED.0 as u32,
        // };
        //         let device: ID3D11Device = texture.GetDevice()?;
        //         println!("device: {device:?}");
        //         let device_context= device.GetImmediateContext()?;
        //         println!("device_context: {device_context:?}");
        //
        //         let mut shared_texture: Option<ID3D11Texture2D> = None;
        //         device.CreateTexture2D(
        //             &texture_desc,
        //             None,
        //             Some(&mut shared_texture));
        //         println!("shared_texture: {shared_texture:?}");
        //         device_context.CopyResource(shared_texture.as_ref().unwrap(), texture);
        //         println!("CopyResource");
        //
        //         let resource: IDXGIResource = shared_texture.as_ref().unwrap().cast()?;
        //         println!("resource: {resource:?}");
                // let handle = resource.GetSharedHandle().unwrap();
                let result = render_target.CreateSharedBitmap(
                    &IDXGISurface::IID as *const windows_core::GUID,
                    std::mem::transmute_copy(&mut texture),
                    Some(&D2D1_BITMAP_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    }   ,
                    dpiX: 96.0,
                    dpiY: 96.0,
                    }),
                    &mut bitmap,
                );
                println!("CreateSharedBitmap result: {result:?}");

            }
            let mut last_frame = self.last_frame.lock();
            *last_frame = bitmap;
            drop(last_frame);
        } */
        Ok(())
    }

    // Optional handler called when the capture item (usually a window) is closed.
    fn on_closed(&mut self) -> Result<(), Self::Error> {
        println!("Capture session ended");

        Ok(())
    }
}

impl WorkspaceSwitchWindow {
    pub const fn hwnd(&self) -> HWND {
        HWND(windows_api::as_ptr!(self.hwnd))
    }

    pub fn create(monitor: Monitor) -> color_eyre::Result<Box<Self>> {
        // println!("create workspace for rect: {monitor:#?}");
        let monitor_id: isize = monitor.id;
        let name: Vec<u16> = format!("komoanimation-{monitor_id}\0")
            .encode_utf16()
            .collect();
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

        let instance = h_module.0 as isize;

        let (workspace_switch_window_sender, workspace_switch_window_receiver) = mpsc::channel();

        std::thread::spawn(move || -> color_eyre::Result<()> {
            let mut workspace_switch_window = Self {
                hwnd: 0,
                d3d_device: None,
                d3d_device_context: None,
                device: None,
                visual: None,
                surface: None,
                target: None,
                d2d_resources: None,
                monitor_id: Some(monitor.id),
                monitor_rect: Some(monitor.size),
                brush_properties: D2D1_BRUSH_PROPERTIES {
                    opacity: 1.0,
                    transform: Matrix3x2::identity(),
                },
                capture_hash_map: HashMap::default(),
            };
            let workspace_switch_window_pointer = &raw mut workspace_switch_window;
            let hwnd = WindowsApi::create_workspace_switch_window(
                PCWSTR(name.as_ptr()),
                instance,
                workspace_switch_window_pointer,
            )?;

            let boxed = unsafe {
                (*workspace_switch_window_pointer).hwnd = hwnd;
                Box::from_raw(workspace_switch_window_pointer)
            };
            workspace_switch_window_sender.send(boxed)?;

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

        let mut workspace_switch_window = workspace_switch_window_receiver.recv()?;

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

            let _ = DwmEnableBlurBehindWindow(workspace_switch_window.hwnd(), &bh);
        }

        /* let hwnd_render_target_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: HWND(windows_api::as_ptr!(workspace_switch_window.hwnd)),
            pixelSize: Default::default(),
            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
        }; */

        unsafe {
            let dxgi_device: IDXGIDevice = D3D_DEVICE.device.cast()?;
                    // 1. Create DComp device
            let device: IDCompositionDevice = DCompositionCreateDevice2(&dxgi_device)?;
            println!("DComp device created");

            // 2. Create target for window
            let target = device.CreateTargetForHwnd(workspace_switch_window.hwnd(), true)?;
            println!("DComp target created");

            // 3. Create visual
            let visual = device.CreateVisual()?;
            println!("DComp visual created");

            let surface = device.CreateSurface(
                workspace_switch_window.monitor_rect.unwrap().right as u32,
                workspace_switch_window.monitor_rect.unwrap().bottom as u32,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                DXGI_ALPHA_MODE_PREMULTIPLIED,
            )?;

            println!("DComp surface created");

            // 5. Set visual content
            visual.SetContent(&surface)?;

            println!("DComp visual content set");

            // 6. Set as root visual
            target.SetRoot(&visual)?;


            // Keep references to prevent early release
            let dxgi_device = dxgi_device.clone();

            let d3d_device: ID3D11Device = dxgi_device.cast()?;
            let d3d_device_context: ID3D11DeviceContext = d3d_device.GetImmediateContext()?;
            workspace_switch_window.d3d_device = Some(d3d_device);
            workspace_switch_window.d3d_device_context = Some(d3d_device_context);
            workspace_switch_window.device = Some(IDCompositionDeviceLocal(device));
            workspace_switch_window.visual = Some(IDCompositionVisualLocal(visual));
            workspace_switch_window.surface = Some(IDCompositionSurfaceLocal(surface));
            workspace_switch_window.target = Some(IDCompositionTargetLocal(target));

            workspace_switch_window.d2d_resources = Some(create_d2d_resources(
                &dxgi_device,
            )?);

            println!("D3D device created");

            Ok(workspace_switch_window)
        }



    }

    pub fn begin_draw(&mut self) -> color_eyre::Result<()> {
        unsafe {
            if let Some(surface) = self.surface.as_ref() {
                let mut offset = POINT { x: 0, y: 0 };
                let dxgi_surface: IDXGISurface = surface.0.BeginDraw(None, &mut offset)?;

                if let Some(d2d_resources) = self.d2d_resources.as_mut() {

                    let bitmap_properties = D2D1_BITMAP_PROPERTIES1 {
                            pixelFormat: D2D1_PIXEL_FORMAT {
                                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                            }   ,
                            dpiX: 96.0,
                            dpiY: 96.0,
                            colorContext: std::mem::ManuallyDrop::new(None),
                            bitmapOptions:  D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                    };
                    let d2d_target = d2d_resources.context.CreateBitmapFromDxgiSurface(
                        &dxgi_surface,
                        Some(&bitmap_properties),
                    )?;

                    d2d_resources.context.SetTarget(&d2d_target);
                    d2d_resources.target = Some(d2d_target);

                    d2d_resources.context.BeginDraw();
                    d2d_resources.context.Clear(None);
                }

            }

            Ok(())
        }
    }

    pub fn end_draw(&mut self) {
        unsafe {
            if let Some(mut d2d_resources) = self.d2d_resources.as_mut() {
                let result = d2d_resources.context.EndDraw(None, None);
                println!("EndDraw d2d result: {result:?}");
                d2d_resources.target = None;
            }

            if let Some(surface) = self.surface.as_ref() {
                let result = surface.0.EndDraw();
                println!("EndDraw dxgi result: {result:?}");
            }

            if let Some(device) = self.device.as_ref() {
                let result = device.0.Commit();

                println!("Commit dcomp result: {result:?}");
            }
        }
    }

    pub fn draw_workspace(&mut self, workspace: &Workspace, x_offset: i32) -> color_eyre::Result<()> {
        unsafe {
                if let Some(d2d_resources) = self.d2d_resources.as_ref() {
                    if let Some(monitor_rect) = self.monitor_rect.as_ref() {
                        if workspace.containers().len() > 0 {
                            for (container_index, container) in workspace.containers().iter().enumerate() {
                                if self.capture_hash_map.contains_key(&container_index) {
                                    continue;
                                }

                                let layout = workspace.latest_layout.get(container_index).unwrap();
                                for window in container.windows() {
                                    if !window.is_visible() {
                                        println!("window not visible");
                                        continue;
                                    }

                                    let hwnd = window.hwnd;
                                    let settings = Settings::new(
                                        Window::from_raw_hwnd(as_ptr!(hwnd)), 
                                    windows_capture::settings::CursorCaptureSettings::WithoutCursor,
                                    windows_capture::settings::DrawBorderSettings::WithoutBorder,
                                    windows_capture::settings::SecondaryWindowSettings::Default,
                                    windows_capture::settings::MinimumUpdateIntervalSettings::Default,
                                    windows_capture::settings::DirtyRegionSettings::Default,
                                    windows_capture::settings::ColorFormat::Rgba8,
                                        (layout.right, layout.bottom, hwnd),
                                    );
                                    let capture = Capture::start_free_threaded_with_item(
                                    Window::from_raw_hwnd(as_ptr!(hwnd)),
                                    self.d3d_device.as_ref().unwrap().clone(),
                                    self.d3d_device_context.as_ref().unwrap().clone(),
                                    settings
                                )?;
                                    self.capture_hash_map.insert(container_index, capture.callback());
                                    println!("capture started: {container_index} {hwnd}");
                                }
                            }
                            for (index, rect) in workspace.latest_layout.iter().enumerate() {

                                let target_rect = D2D_RECT_F {
                                    left: (rect.left - monitor_rect.left + x_offset) as f32,
                                    top: (rect.top - monitor_rect.top) as f32,
                                    right: ((rect.left - monitor_rect.left) + rect.right + x_offset)
                                        as f32,
                                    bottom: ((rect.top - monitor_rect.top) + rect.bottom) as f32,
                                };
                                //
                                d2d_resources.context.DrawRectangle(&target_rect, &d2d_resources.brush, 3.0, None);
                                // d2d_resources.context.FillRectangle(&target_rect, &d2d_resources.brush);
                                println!("rect: {target_rect:?}");
                                
                                let capture = self.capture_hash_map.get(&index);
                                if capture.is_none() {
                                    continue;
                                }

                                let capture = capture.unwrap().lock();
                                println!("capture: {capture:?}");
                                let last_frame = &capture.last_frame;
                                println!("last_frame: {last_frame:?}");

                                if let Some(mut texture) = last_frame.lock().as_ref() {
                                    let bitmap_properties = D2D1_BITMAP_PROPERTIES1 {
                                            pixelFormat: D2D1_PIXEL_FORMAT {
                                                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                                                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                                            }   ,
                                            dpiX: 96.0,
                                            dpiY: 96.0,
                                            colorContext: std::mem::ManuallyDrop::new(None),
                                            bitmapOptions:  D2D1_BITMAP_OPTIONS_TARGET,
                                    };
                                    let texture_surface: IDXGISurface = texture.cast()?;
                                    let bitmap = d2d_resources.context.CreateBitmapFromDxgiSurface(
                                        &texture_surface,
                                        Some(&bitmap_properties as *const _),
                                    )?;
                                    println!("bitmap: {bitmap:?}");
                                    // println!("CreateSharedBitmap result: {:?}");
                                    d2d_resources.context.DrawBitmap(
                                        &bitmap,
                                        Some(&target_rect as *const _),
                                        1.0,
                                        D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                                        None,
                                        None,
                                    );
                                }
                            }
                        }
                    }
                }
        }
        Ok(())
    }

    pub fn destroy(&mut self) -> color_eyre::Result<()> {
        self.monitor_id = None;
        self.monitor_rect = None;
        for capture in self.capture_hash_map.values() {
            *capture.lock().need_stop.lock() = true;
        }

        self.capture_hash_map.clear();

        unsafe {
            SetWindowLongPtrW(self.hwnd(), GWLP_USERDATA, 0);
        }
        WindowsApi::close_window(self.hwnd)
    }

    pub extern "system" fn callback(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            match message {
                WM_CREATE => {
                    let mut workspace_switch_window_pointer: *mut WorkspaceSwitchWindow =
                        GetWindowLongPtrW(window, GWLP_USERDATA) as _;

                    if workspace_switch_window_pointer.is_null() {
                        let create_struct: *mut CREATESTRUCTW = lparam.0 as *mut _;
                        workspace_switch_window_pointer = (*create_struct).lpCreateParams as *mut _;
                        SetWindowLongPtrW(
                            window,
                            GWLP_USERDATA,
                            workspace_switch_window_pointer as _,
                        );
                    }

                    if let Some(rect) = (*workspace_switch_window_pointer).monitor_rect {
                        println!("monitor rect: {rect:?}");
                        ShowWindow(window, SW_HIDE);
                        let old_rect = WindowsApi::window_rect(window.0 as isize).unwrap();
                        println!("old rect: {old_rect:?}");
                        WindowsApi::position_window(window.0 as isize, &rect, false, false);
                        ShowWindow(window, SW_MAXIMIZE);
                        let new_rect = WindowsApi::window_rect(window.0 as isize).unwrap();
                        println!("new rect: {new_rect:?}");
                    }

                    // InvalidateRect(Some(window), None, false);

                    LRESULT(0)
                }

                WM_PAINT => {
                    if let Ok(rect) = WindowsApi::window_rect(window.0 as isize) {
                        let workspace_switch_pointer: *mut WorkspaceSwitchWindow =
                            GetWindowLongPtrW(window, GWLP_USERDATA) as _;

                        if workspace_switch_pointer.is_null() {
                            return LRESULT(0);
                        }

                        tracing::trace!("updating workspace switch");
                        // if let Err(error) =
                        //     (*workspace_switch_pointer).set_position(&border_window_rect, reference_hwnd)
                        // {
                        //     tracing::error!("failed to update border position {error}");
                        // }

                        /* if let Some(render_target) =
                            (*workspace_switch_pointer).render_target.lock().as_ref()
                        {
                            let _ = render_target.Resize(&D2D_SIZE_U {
                                width: rect.right as u32,
                                height: rect.bottom as u32,
                            });

                            // Get window kind and color
                            if let Some(brush) = (*workspace_switch_pointer).brush.as_ref() {
                                render_target.BeginDraw();
                                render_target.Clear(None);

                                render_target.FillRectangle(
                                    &D2D_RECT_F {
                                        left: 0.0,
                                        top: 0.0,
                                        right: rect.right as f32,
                                        bottom: rect.bottom as f32,
                                    },
                                    brush,
                                );

                                let _ = render_target.EndDraw(None, None);
                            }
                        } */
                    }
                    let _ = ValidateRect(Option::from(window), None);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    let workspace_switch_window_pointer: *mut WorkspaceSwitchWindow =
                        GetWindowLongPtrW(window, GWLP_USERDATA) as _;
                    if !workspace_switch_window_pointer.is_null() {
                        (*workspace_switch_window_pointer).device = None;
                        (*workspace_switch_window_pointer).surface = None;
                        (*workspace_switch_window_pointer).visual = None;
                        (*workspace_switch_window_pointer).d2d_resources = None;
                        (*workspace_switch_window_pointer).target = None;
                        (*workspace_switch_window_pointer).d3d_device = None;
                        (*workspace_switch_window_pointer).d3d_device_context = None;
                        // (*workspace_switch_window_pointer).brushes.clear();
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
