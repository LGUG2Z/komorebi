use crate::core::animation::AnimationStyle;

pub use prefix::AnimationPrefix;
pub use komorebi_config::animation::PerAnimationPrefixConfig;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use step::MotionSpec;

pub mod animator;
pub mod capture;
pub mod ghost;
pub mod lerp;
pub mod prefix;
pub mod stats;
pub mod step;
pub mod style;
pub mod throttle;
pub mod tracks;
pub mod transition;
pub mod workspace_transition;


pub const DEFAULT_ANIMATION_ENABLED: bool = false;
pub const DEFAULT_ANIMATION_STYLE: AnimationStyle = AnimationStyle::Linear;
pub const DEFAULT_ANIMATION_DURATION: u64 = 250;
pub const DEFAULT_ANIMATION_FPS: u64 = 60;

/// How many operations have asked for animations to be skipped for as long as they run
static ANIMATIONS_SUSPENDED: AtomicUsize = AtomicUsize::new(0);

/// Skips animations for as long as the guard is held
///
/// Some operations, such as stacking windows, rearrange a workspace in a way that animating would
/// only make look wrong. Unlike turning the global setting off and on again, this puts everything
/// back when the operation fails part of the way through, and it applies to per-animation settings
/// too.
pub struct SuspendedAnimations(());

impl Drop for SuspendedAnimations {
    fn drop(&mut self) {
        ANIMATIONS_SUSPENDED.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn suspend() -> SuspendedAnimations {
    ANIMATIONS_SUSPENDED.fetch_add(1, Ordering::SeqCst);
    SuspendedAnimations(())
}

/// How the windows for `prefix` should be moved, or `None` if they should simply be placed
pub fn motion_for(prefix: AnimationPrefix) -> Option<MotionSpec> {
    if ANIMATIONS_SUSPENDED.load(Ordering::SeqCst) > 0 {
        return None;
    }

    // one snapshot: enabled, duration and style are read from the same configuration
    let motion = crate::settings::settings()
        .snapshot()
        .shared
        .animation
        .motion_for(prefix)?;

    Some(MotionSpec {
        duration: Duration::from_millis(motion.duration_ms),
        style: motion.style,
    })
}

/// Whether switching workspaces should slide the windows out and in
pub fn workspace_transitions_enabled() -> bool {
    motion_for(AnimationPrefix::Workspace).is_some()
}

/// How long one frame takes, given the configured frames per second
pub fn frame_period() -> Duration {
    let fps = crate::settings::animation_fps().clamp(1, 240);

    Duration::from_secs_f64(1.0 / fps as f64)
}
