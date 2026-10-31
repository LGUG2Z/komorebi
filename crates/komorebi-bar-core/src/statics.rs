use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

pub static MAX_LABEL_WIDTH: AtomicI32 = AtomicI32::new(400);
pub static MONITOR_LEFT: AtomicI32 = AtomicI32::new(0);
pub static MONITOR_TOP: AtomicI32 = AtomicI32::new(0);
pub static MONITOR_RIGHT: AtomicI32 = AtomicI32::new(0);
pub static MONITOR_INDEX: AtomicUsize = AtomicUsize::new(0);
pub static BAR_HEIGHT: f32 = 50.0;
pub static DEFAULT_PADDING: f32 = 10.0;

/// Flag to indicate that a widget has consumed a click event this frame.
/// This prevents the bar's global mouse handler from also processing the click.
pub static WIDGET_CLICKED: AtomicBool = AtomicBool::new(false);

/// Mark that a widget has consumed a click event this frame.
pub fn mark_widget_clicked() {
    WIDGET_CLICKED.store(true, Ordering::SeqCst);
}

/// Check if a widget has consumed a click event this frame and reset the flag.
pub fn take_widget_clicked() -> bool {
    WIDGET_CLICKED.swap(false, Ordering::SeqCst)
}

pub static AUTO_SELECT_FILL_COLOUR: AtomicU32 = AtomicU32::new(0);
pub static AUTO_SELECT_TEXT_COLOUR: AtomicU32 = AtomicU32::new(0);
