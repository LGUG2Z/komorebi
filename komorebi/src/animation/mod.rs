//! The animation settings (the `komorebic animation*` commands and the `animation` key of
//! `komorebi.json`).
//!
//! komorebi does not animate windows itself: KWin's Geometry Change effect animates a window going
//! to its place. [`movement_animation`] and [`movement_style`] are what the effect is told, in the
//! [`Look`](crate::backend::Look) the border manager asks the backend for. The stock effect has
//! one curve, an exponential ease out; komorebi's build of it takes every style. The `fps`
//! setting is stored and has nothing to map to: the compositor animates at the refresh rate of the
//! screen.

use crate::core::animation::AnimationStyle;

use parking_lot::Mutex;
pub use prefix::AnimationPrefix;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;

pub mod prefix;

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

/// Returns how long window movement is animated for, in milliseconds, or `None` when it is not
/// animated.
///
/// The `movement` settings win over the global ones, for whether it is animated and for how long.
pub fn movement_animation() -> Option<u64> {
    let enabled = ANIMATION_ENABLED_PER_ANIMATION
        .lock()
        .get(&AnimationPrefix::Movement)
        .copied()
        .unwrap_or_else(|| ANIMATION_ENABLED_GLOBAL.load(std::sync::atomic::Ordering::SeqCst));

    enabled.then(|| {
        ANIMATION_DURATION_PER_ANIMATION
            .lock()
            .get(&AnimationPrefix::Movement)
            .copied()
            .unwrap_or_else(|| ANIMATION_DURATION_GLOBAL.load(std::sync::atomic::Ordering::SeqCst))
    })
}

/// Returns the easing of window movement: the `movement` style, else the global one.
pub fn movement_style() -> AnimationStyle {
    ANIMATION_STYLE_PER_ANIMATION
        .lock()
        .get(&AnimationPrefix::Movement)
        .copied()
        .unwrap_or_else(|| *ANIMATION_STYLE_GLOBAL.lock())
}
