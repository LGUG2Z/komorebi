//! Pictures of windows that slide across a monitor in place of the windows themselves
//!
//! A borderless, click-through window covers the monitor and holds a layer for each ghost. The
//! layers are moved by Core Animation, which interpolates on the render server, so there is no
//! thread to keep time and no frame to miss. Whatever is outside of the monitor is cut off by the
//! window itself.

use crate::animation::capture::Still;
use crate::animation::transition::progress_samples;
use crate::border_manager::BorderLook;
use crate::core::Rect;
use crate::core::animation::AnimationStyle;
use crate::core_graphics::CoreGraphicsApi;
use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2::MainThreadOnly;
use objc2::class;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSBackingStoreType;
use objc2_app_kit::NSColor;
use objc2_app_kit::NSFloatingWindowLevel;
use objc2_app_kit::NSView;
use objc2_app_kit::NSWindow;
use objc2_app_kit::NSWindowAnimationBehavior;
use objc2_app_kit::NSWindowCollectionBehavior;
use objc2_app_kit::NSWindowStyleMask;
use objc2_core_graphics::CGColor;
use objc2_core_graphics::CGMainDisplayID;
use objc2_foundation::NSArray;
use objc2_foundation::NSDictionary;
use objc2_foundation::NSNumber;
use objc2_foundation::NSPoint;
use objc2_foundation::NSRect;
use objc2_foundation::NSSize;
use objc2_foundation::NSString;
use objc2_quartz_core::CALayer;
use objc2_quartz_core::CATransaction;
use std::sync::mpsc;
use std::time::Duration;

/// How many pieces a slide is cut into for the compositor to interpolate between
const SAMPLES: usize = 64;

/// How long past its time something that should have been done on the main thread is waited for
const MAIN_THREAD_SLACK: Duration = Duration::from_millis(750);

/// A picture of a window, and the places that it slides between
pub struct Ghost {
    pub still: Still,
    /// The border that goes around the window, which slides with it
    pub border: Option<BorderLook>,
    pub from: Rect,
    pub to: Rect,
}

/// What can be touched on the main thread only, carried there and back by the closures that run on it
struct MainOnly<T>(T);

unsafe impl<T> Send for MainOnly<T> {}

struct Layer {
    layer: Retained<CALayer>,
    from: Rect,
    to: Rect,
}

/// The window that covers a monitor with ghosts
pub struct Overlay {
    window: MainOnly<Retained<NSWindow>>,
    root: MainOnly<Retained<CALayer>>,
    layers: MainOnly<Vec<Layer>>,
    clip: Rect,
}

/// Where in the layer of a view, which has its origin at the bottom left, a rect that is in the
/// coordinates of the screen, which have theirs at the top left, belongs
fn layer_rect(rect: Rect, clip: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(
            f64::from(rect.left - clip.left),
            f64::from(clip.bottom - (rect.top - clip.top) - rect.bottom),
        ),
        NSSize::new(f64::from(rect.right), f64::from(rect.bottom)),
    )
}

/// The layer that draws a border around a ghost, outside of it, as the border window does around
/// a window
fn border_layer(look: BorderLook, window: Rect) -> Retained<CALayer> {
    let layer = CALayer::new();

    layer.setFrame(NSRect::new(
        NSPoint::new(-look.outset, -look.outset),
        NSSize::new(
            f64::from(window.right) + look.outset * 2.0,
            f64::from(window.bottom) + look.outset * 2.0,
        ),
    ));
    layer.setActions(Some(&NSDictionary::new()));

    unsafe {
        let clear = CGColor::new_generic_rgb(0.0, 0.0, 0.0, 0.0);
        let colour = CGColor::new_generic_rgb(
            f64::from(look.colour.r) / 255.0,
            f64::from(look.colour.g) / 255.0,
            f64::from(look.colour.b) / 255.0,
            1.0,
        );
        let _: () = msg_send![&layer, setBackgroundColor: &*clear];
        let _: () = msg_send![&layer, setBorderColor: &*colour];
        let _: () = msg_send![&layer, setBorderWidth: look.width];
        let _: () = msg_send![&layer, setCornerRadius: look.radius];
    }

    layer
}

fn on_main<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = mpsc::channel();

    DispatchQueue::main().exec_async(move || {
        let _ = tx.send(autoreleasepool(|_| work()));
    });

    rx.recv_timeout(MAIN_THREAD_SLACK).ok()
}

impl Overlay {
    /// Put the ghosts over the monitor `clip`, at the places that they slide from, and wait for
    /// them to be there
    ///
    /// This is `None` when the main thread did not do it, which is for the caller to carry on without.
    pub fn show(clip: Rect, ghosts: Vec<Ghost>) -> Option<Self> {
        let ghosts = MainOnly(ghosts);

        let (window, root, layers) = on_main(move || {
            let ghosts = ghosts;
            let mtm = unsafe { MainThreadMarker::new_unchecked() };
            let main_display = Rect::from(CoreGraphicsApi::display_bounds(CGMainDisplayID()));

            // the origin of the window is its bottom left, in the coordinates of the screens
            let frame = NSRect::new(
                NSPoint::new(
                    f64::from(clip.left),
                    f64::from(main_display.bottom - clip.top - clip.bottom),
                ),
                NSSize::new(f64::from(clip.right), f64::from(clip.bottom)),
            );

            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    frame,
                    NSWindowStyleMask::Borderless,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };

            window.setBackgroundColor(Some(&NSColor::clearColor()));
            window.setAnimationBehavior(NSWindowAnimationBehavior::None);
            window.setRestorable(false);
            window.setHasShadow(false);
            window.setOpaque(false);
            // above the windows of the apps, and below the menu bar and the Dock
            window.setLevel(NSFloatingWindowLevel);
            window.setIgnoresMouseEvents(true);
            window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Stationary
                    | NSWindowCollectionBehavior::IgnoresCycle
                    | NSWindowCollectionBehavior::Transient,
            );

            let view = NSView::initWithFrame(
                NSView::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), frame.size),
            );
            view.setWantsLayer(true);

            let root = CALayer::new();
            root.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), frame.size));
            root.setMasksToBounds(true);
            root.setActions(Some(&NSDictionary::new()));
            view.setLayer(Some(&root));
            window.setContentView(Some(&view));

            let mut layers = vec![];

            for ghost in ghosts.0 {
                // what slides is a layer with the border below the picture, which is how the border
                // window is below the window: the part of its band that is inside the window is
                // not seen, and the border is not any bigger than the real one
                let layer = CALayer::new();
                let frame = layer_rect(ghost.from, clip);
                layer.setFrame(frame);
                layer.setActions(Some(&NSDictionary::new()));

                if let Some(look) = ghost.border {
                    layer.addSublayer(&border_layer(look, ghost.to));
                }

                let picture = CALayer::new();
                picture.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), frame.size));
                picture.setActions(Some(&NSDictionary::new()));

                // the picture is of a window of that many points, in more pixels than that
                let image: *const AnyObject = &*ghost.still.image as *const _ as *const AnyObject;
                unsafe {
                    let _: () = msg_send![&picture, setContents: image];
                }

                layer.addSublayer(&picture);

                root.addSublayer(&layer);
                layers.push(Layer {
                    layer,
                    from: ghost.from,
                    to: ghost.to,
                });
            }

            // without activating the app or taking the focus from the window that has it
            window.orderFrontRegardless();

            // what is drawn is on screen before anything underneath changes
            CATransaction::flush();

            MainOnly((window, root, layers))
        })
        .map(|shown| shown.0)?;

        Some(Self {
            window: MainOnly(window),
            root: MainOnly(root),
            layers: MainOnly(layers),
            clip,
        })
    }

    /// Slide every ghost from where it is to where it goes, and wait for them to get there
    pub fn slide(&self, duration: Duration, style: AnimationStyle) {
        let layers: Vec<(MainOnly<Retained<CALayer>>, Rect, Rect)> = self
            .layers
            .0
            .iter()
            .map(|layer| (MainOnly(layer.layer.clone()), layer.from, layer.to))
            .collect();
        let clip = self.clip;
        let progress = progress_samples(style, SAMPLES);
        let (done_tx, done_rx) = mpsc::channel::<()>();

        let started = on_main(move || {
            let layers = layers;

            CATransaction::begin();
            CATransaction::setAnimationDuration(duration.as_secs_f64());

            let done_tx = done_tx;
            let completion = RcBlock::new(move || {
                let _ = done_tx.send(());
            });

            unsafe {
                let _: () = msg_send![class!(CATransaction), setCompletionBlock: &*completion];
            }

            for (layer, from, to) in &layers {
                let from_frame = layer_rect(*from, clip);
                let to_frame = layer_rect(*to, clip);

                let from_x = from_frame.origin.x + from_frame.size.width / 2.0;
                let to_x = to_frame.origin.x + to_frame.size.width / 2.0;

                let values: Vec<Retained<NSNumber>> = progress
                    .iter()
                    .map(|p| NSNumber::new_f64(from_x + (to_x - from_x) * p))
                    .collect();
                let values = NSArray::from_retained_slice(&values);

                unsafe {
                    let animation: Retained<AnyObject> = msg_send![
                        class!(CAKeyframeAnimation),
                        animationWithKeyPath: &*NSString::from_str("position.x")
                    ];
                    let _: () = msg_send![&animation, setValues: &*values];
                    let _: () = msg_send![&animation, setDuration: duration.as_secs_f64()];

                    // it stays where it has got to, instead of going back to where it was
                    layer.0.setFrame(to_frame);

                    let _: () = msg_send![
                        &*layer.0,
                        addAnimation: &*animation,
                        forKey: &*NSString::from_str("slide")
                    ];
                }
            }

            CATransaction::commit();
            // the block is kept by the transaction, and this only lets go of ours
            drop(completion);
        });

        if started.is_none() {
            return;
        }

        let _ = done_rx.recv_timeout(duration + MAIN_THREAD_SLACK);
    }

    /// Let the ghosts fade away, which is not waited for, and take the overlay down after
    pub fn fade_out(self, duration: Duration) {
        let window = self.window;
        let root = self.root;
        let _layers = self.layers;

        DispatchQueue::main().exec_async(move || {
            let (window, root) = (window, root);

            autoreleasepool(|_| {
                CATransaction::begin();
                CATransaction::setAnimationDuration(duration.as_secs_f64());

                let window_to_close = MainOnly(window.0.clone());
                let completion = RcBlock::new(move || {
                    window_to_close.0.orderOut(None);
                });

                unsafe {
                    let _: () = msg_send![class!(CATransaction), setCompletionBlock: &*completion];
                    // implicit animations were turned off for the layer, which is how it was made
                    let animation: Retained<AnyObject> = msg_send![
                        class!(CABasicAnimation),
                        animationWithKeyPath: &*NSString::from_str("opacity")
                    ];
                    let from = NSNumber::new_f32(1.0);
                    let to = NSNumber::new_f32(0.0);
                    let _: () = msg_send![&animation, setFromValue: &*from];
                    let _: () = msg_send![&animation, setToValue: &*to];
                    let _: () = msg_send![&animation, setDuration: duration.as_secs_f64()];
                    root.0.setOpacity(0.0);
                    let _: () = msg_send![
                        &*root.0,
                        addAnimation: &*animation,
                        forKey: &*NSString::from_str("fade")
                    ];
                }

                CATransaction::commit();
            });
        });
    }

    /// Take the overlay down at once
    pub fn close(self) {
        let window = self.window;
        let _root = self.root;
        let _layers = self.layers;

        DispatchQueue::main().exec_async(move || {
            let window = window;
            window.0.orderOut(None);
        });
    }
}
