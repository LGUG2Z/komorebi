use crate::core::animation::AnimationStyle;

use parking_lot::Mutex;
pub use prefix::AnimationPrefix;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
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

use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Animation configuration
///
/// This can be either global:
/// ```json
/// {
///     "enabled": true,
///     "style": "EaseInSine",
///     "fps": 60,
///     "duration": 250
/// }
/// ```
///
/// Or scoped by an animation kind prefix:
/// ```json
/// {
///     "movement": {
///         "enabled": true,
///         "style": "EaseInSine",
///         "fps": 60,
///         "duration": 250
///     }
/// }
/// ```
pub enum PerAnimationPrefixConfig<T> {
    /// Animation configuration prefixed for a specific animation kind
    Prefix(HashMap<AnimationPrefix, T>),
    /// Animation configuration for all animation kinds
    Global(T),
}

pub const DEFAULT_ANIMATION_ENABLED: bool = false;
pub const DEFAULT_ANIMATION_STYLE: AnimationStyle = AnimationStyle::Linear;
pub const DEFAULT_ANIMATION_DURATION: u64 = 250;
pub const DEFAULT_ANIMATION_FPS: u64 = 60;

/// How many operations have asked for animations to be skipped for as long as they run
static ANIMATIONS_SUSPENDED: AtomicUsize = AtomicUsize::new(0);

pub static ANIMATION_STYLE_GLOBAL: LazyLock<Arc<Mutex<AnimationStyle>>> =
    LazyLock::new(|| Arc::new(Mutex::new(DEFAULT_ANIMATION_STYLE)));

pub static ANIMATION_ENABLED_GLOBAL: LazyLock<Arc<AtomicBool>> =
    LazyLock::new(|| Arc::new(AtomicBool::new(DEFAULT_ANIMATION_ENABLED)));

pub static ANIMATION_DURATION_GLOBAL: LazyLock<Arc<AtomicU64>> =
    LazyLock::new(|| Arc::new(AtomicU64::new(DEFAULT_ANIMATION_DURATION)));

pub static ANIMATION_STYLE_PER_ANIMATION: LazyLock<
    Arc<Mutex<HashMap<AnimationPrefix, AnimationStyle>>>,
> = LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

pub static ANIMATION_ENABLED_PER_ANIMATION: LazyLock<Arc<Mutex<HashMap<AnimationPrefix, bool>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

pub static ANIMATION_DURATION_PER_ANIMATION: LazyLock<Arc<Mutex<HashMap<AnimationPrefix, u64>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

pub static ANIMATION_FPS: AtomicU64 = AtomicU64::new(DEFAULT_ANIMATION_FPS);

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

    let enabled = ANIMATION_ENABLED_PER_ANIMATION
        .lock()
        .get(&prefix)
        .copied()
        .unwrap_or_else(|| ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst));

    if !enabled {
        return None;
    }

    Some(MotionSpec {
        duration: Duration::from_millis(
            ANIMATION_DURATION_PER_ANIMATION
                .lock()
                .get(&prefix)
                .copied()
                .unwrap_or_else(|| ANIMATION_DURATION_GLOBAL.load(Ordering::SeqCst)),
        ),
        style: ANIMATION_STYLE_PER_ANIMATION
            .lock()
            .get(&prefix)
            .copied()
            .unwrap_or_else(|| *ANIMATION_STYLE_GLOBAL.lock()),
    })
}

/// Whether switching workspaces should slide the windows out and in
pub fn workspace_transitions_enabled() -> bool {
    motion_for(AnimationPrefix::Workspace).is_some()
}

/// How long one frame takes, given the configured frames per second
pub fn frame_period() -> Duration {
    let fps = ANIMATION_FPS.load(Ordering::Relaxed).clamp(1, 240);

    Duration::from_secs_f64(1.0 / fps as f64)
}
