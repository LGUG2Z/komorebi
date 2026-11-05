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

pub use komorebi_config::animation::PerAnimationPrefixConfig;
pub use prefix::AnimationPrefix;

pub mod prefix;

pub const DEFAULT_ANIMATION_ENABLED: bool = false;
pub const DEFAULT_ANIMATION_STYLE: AnimationStyle = AnimationStyle::Linear;
pub const DEFAULT_ANIMATION_DURATION: u64 = 250;
pub const DEFAULT_ANIMATION_FPS: u64 = 60;

/// Returns how long window movement is animated for, in milliseconds, or `None` when it is not
/// animated.
///
/// The `movement` settings win over the global ones, for whether it is animated and for how long.
pub fn movement_animation() -> Option<u64> {
    crate::settings::settings()
        .snapshot()
        .shared
        .animation
        .motion_for(AnimationPrefix::Movement)
        .map(|motion| motion.duration_ms)
}

/// Returns the easing of window movement: the `movement` style, else the global one.
pub fn movement_style() -> AnimationStyle {
    let snapshot = crate::settings::settings().snapshot();
    let animation = &snapshot.shared.animation;

    animation
        .style_per_animation
        .get(&AnimationPrefix::Movement)
        .copied()
        .unwrap_or(animation.style)
}
