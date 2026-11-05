use crate::border_manager::BORDER_OFFSET;
use crate::border_manager::BORDER_OFFSET_ADJUSTMENT;
use crate::border_manager::BORDER_RADIUS;
use crate::border_manager::BORDER_WIDTH;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use dispatch2::DispatchQueue;
use komorebi_themes::colour::Rgb;
use objc2::MainThreadMarker;
use objc2::MainThreadOnly;
use objc2::ffi::NSInteger;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSBackingStoreType;
use objc2_app_kit::NSColor;
use objc2_app_kit::NSNormalWindowLevel;
use objc2_app_kit::NSView;
use objc2_app_kit::NSWindow;
use objc2_app_kit::NSWindowAnimationBehavior;
use objc2_app_kit::NSWindowCollectionBehavior;
use objc2_app_kit::NSWindowOrderingMode;
use objc2_app_kit::NSWindowStyleMask;
use objc2_core_graphics::CGColor;
use objc2_foundation::NSDictionary;
use objc2_foundation::NSRect;
use objc2_quartz_core::CALayer;
use std::ops::Deref;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

#[derive(Debug)]
pub struct NsWindow {
    pub window: Retained<NSWindow>,
    pub layer: Retained<CALayer>,
}

unsafe impl Send for NsWindow {}

impl NsWindow {
    pub fn new(ns_rect: NSRect, target_window_id: u32) -> eyre::Result<NsWindow> {
        let offset = BORDER_OFFSET.load(Ordering::Relaxed) as f64 + BORDER_OFFSET_ADJUSTMENT as f64;

        let mut ns_rect = ns_rect;

        ns_rect.origin.x -= offset;
        ns_rect.origin.y -= offset;
        ns_rect.size.width += offset * 2.0;
        ns_rect.size.height += offset * 2.0;

        let (tx, rx) = mpsc::channel();

        DispatchQueue::main().exec_async(move || {
            autoreleasepool(|_| {
                let mtm = unsafe { MainThreadMarker::new_unchecked() };

                // Create window
                let window_frame = ns_rect;

                let window = unsafe {
                    let window = NSWindow::alloc(mtm);
                    NSWindow::initWithContentRect_styleMask_backing_defer(
                        window,
                        window_frame,
                        NSWindowStyleMask::Borderless,
                        NSBackingStoreType::Buffered,
                        false,
                    )
                };

                // Make transparent
                window.setBackgroundColor(Some(&NSColor::clearColor()));
                window.setAnimationBehavior(NSWindowAnimationBehavior::None);
                window.disableSnapshotRestoration();
                window.setPreservesContentDuringLiveResize(false);
                window.setRestorable(false);

                window.setHasShadow(false);
                window.setOpaque(false);
                window.setLevel(NSNormalWindowLevel);
                window.setIgnoresMouseEvents(true);

                window.setCollectionBehavior(
                    NSWindowCollectionBehavior::CanJoinAllSpaces |
                        NSWindowCollectionBehavior::Stationary |
                        NSWindowCollectionBehavior::IgnoresCycle |
                        NSWindowCollectionBehavior::Transient
                );

                let content_view = {
                    let view = NSView::alloc(mtm);
                    NSView::initWithFrame(view, window_frame)
                };

                content_view.setWantsLayer(true);
                content_view.setAutoresizesSubviews(false);

                // Create and configure the layer for the border
                let layer = {
                    let layer = CALayer::new();
                    layer.setFrame(ns_rect);
                    layer.setActions(Some(&NSDictionary::new()));

                    unsafe {
                        // transparent
                        let clear = CGColor::new_generic_rgb(0.0, 0.0, 0.0, 0.0);
                        let clear_ptr = clear.deref() as *const _;
                        let _: () = msg_send![&layer, setBackgroundColor: clear_ptr];

                        let red = CGColor::new_generic_rgb(1.0, 0.0, 0.0, 1.0);
                        let red_ptr = red.deref() as *const _;
                        let _: () = msg_send![&layer, setBorderColor: red_ptr];
                        let _: () = msg_send![&layer, setBorderWidth: BORDER_WIDTH.load(Ordering::Relaxed) as f64];

                        let corner_radius: f64 = BORDER_RADIUS.load(Ordering::Relaxed) as f64;
                        let _: () = msg_send![&layer, setCornerRadius: corner_radius];

                        layer
                    }
                };

                content_view.setLayer(Some(&layer));

                window.setContentView(Some(&content_view));
                window.setMovableByWindowBackground(false);
                window.makeKeyAndOrderFront(None);
                window.orderWindow_relativeTo(NSWindowOrderingMode::Below, NSInteger::from(target_window_id as i16));
                if let Err(error) = tx.send(NsWindow { window, layer }) {
                    tracing::error!("could not send NSWindow created for border: {error}")
                }

            })
        });

        rx.recv()
            .ok()
            .ok_or_eyre("could not create a border NSWindow")
    }

    pub fn set_border_color(&self, rgb: Rgb) {
        unsafe {
            // this is ass
            let color = CGColor::new_generic_rgb(
                rgb.r as f64 / 255.0,
                rgb.g as f64 / 255.0,
                rgb.b as f64 / 255.0,
                1.0,
            );
            let color_ptr = color.deref() as *const _;
            let _: () = msg_send![&self.layer, setBorderColor: color_ptr];
        }
    }

    pub fn set_border_width(&self, width: f64) {
        unsafe {
            let _: () = msg_send![&self.layer, setBorderWidth: width];
        }
    }
}
