use crate::animation::animation_manager::AnimationManager;
use crate::core::animation::AnimationStyle;

use lazy_static::lazy_static;
use prefix::AnimationPrefix;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use parking_lot::Mutex;

pub use engine::AnimationEngine;
pub mod animation_manager;
pub mod engine;
pub mod ghost;
pub mod ghost_dispatcher;
pub mod lerp;
pub mod prefix;
pub mod render_dispatcher;
pub mod snapshot;
pub use render_dispatcher::RenderDispatcher;
pub mod style;
pub mod transition;

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
pub const DEFAULT_GHOST_MOVEMENT: bool = false;

lazy_static! {
    pub static ref ANIMATION_MANAGER: Arc<Mutex<AnimationManager>> =
        Arc::new(Mutex::new(AnimationManager::new()));
    pub static ref ANIMATION_STYLE_GLOBAL: Arc<Mutex<AnimationStyle>> =
        Arc::new(Mutex::new(DEFAULT_ANIMATION_STYLE));
    pub static ref ANIMATION_ENABLED_GLOBAL: Arc<AtomicBool> =
        Arc::new(AtomicBool::new(DEFAULT_ANIMATION_ENABLED));
    pub static ref ANIMATION_DURATION_GLOBAL: Arc<AtomicU64> =
        Arc::new(AtomicU64::new(DEFAULT_ANIMATION_DURATION));
    pub static ref ANIMATION_STYLE_PER_ANIMATION: Arc<Mutex<HashMap<AnimationPrefix, AnimationStyle>>> =
        Arc::new(Mutex::new(HashMap::new()));
    pub static ref ANIMATION_ENABLED_PER_ANIMATION: Arc<Mutex<HashMap<AnimationPrefix, bool>>> =
        Arc::new(Mutex::new(HashMap::new()));
    pub static ref ANIMATION_DURATION_PER_ANIMATION: Arc<Mutex<HashMap<AnimationPrefix, u64>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

/// Whether the windows of a workspace slide out of a monitor and those of the
/// workspace that is switched to slide in. Animations being enabled for all
/// kinds enables this one too, unless it is turned off for this kind in
/// particular. The windows are slid as ghosts, so ghosts are needed.
fn transitions_enabled_by(
    ghost_movement: bool,
    enabled_globally: bool,
    enabled: &HashMap<AnimationPrefix, bool>,
) -> bool {
    ghost_movement
        && enabled
            .get(&AnimationPrefix::Workspace)
            .copied()
            .unwrap_or(enabled_globally)
}

/// How long the animation of a kind takes, which is what it is set to or else
/// what animations take
fn duration_for(
    prefix: AnimationPrefix,
    per_animation: &HashMap<AnimationPrefix, u64>,
    global: u64,
) -> Duration {
    Duration::from_millis(per_animation.get(&prefix).copied().unwrap_or(global))
}

/// How the animation of a kind eases, which is what it is set to or else what
/// animations do
fn style_for(
    prefix: AnimationPrefix,
    per_animation: &HashMap<AnimationPrefix, AnimationStyle>,
    global: AnimationStyle,
) -> AnimationStyle {
    per_animation.get(&prefix).copied().unwrap_or(global)
}

/// Whether workspaces slide in and out of the monitor when they are switched
pub fn workspace_transitions_enabled() -> bool {
    transitions_enabled_by(
        GHOST_MOVEMENT_ENABLED.load(Ordering::Relaxed),
        ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst),
        &ANIMATION_ENABLED_PER_ANIMATION.lock(),
    )
}

/// How long a workspace takes to slide in and out of a monitor
pub fn workspace_transition_duration() -> Duration {
    duration_for(
        AnimationPrefix::Workspace,
        &ANIMATION_DURATION_PER_ANIMATION.lock(),
        ANIMATION_DURATION_GLOBAL.load(Ordering::SeqCst),
    )
}

/// How a workspace eases as it slides in and out of a monitor
pub fn workspace_transition_style() -> AnimationStyle {
    style_for(
        AnimationPrefix::Workspace,
        &ANIMATION_STYLE_PER_ANIMATION.lock(),
        *ANIMATION_STYLE_GLOBAL.lock(),
    )
}

pub static ANIMATION_FPS: AtomicU64 = AtomicU64::new(DEFAULT_ANIMATION_FPS);
pub static GHOST_MOVEMENT_ENABLED: AtomicBool = AtomicBool::new(DEFAULT_GHOST_MOVEMENT);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspaces_slide_when_animations_are_on_unless_turned_off_for_them() {
        let none = HashMap::new();
        let workspace = HashMap::from([(AnimationPrefix::Workspace, true)]);
        let movement = HashMap::from([(AnimationPrefix::Movement, true)]);
        let off = HashMap::from([(AnimationPrefix::Workspace, false)]);

        // all animations on is this one on
        assert!(transitions_enabled_by(true, true, &none));
        assert!(transitions_enabled_by(true, true, &movement));

        // unless it is turned off in particular
        assert!(!transitions_enabled_by(true, true, &off));

        // or on in particular, without the rest
        assert!(transitions_enabled_by(true, false, &workspace));
        assert!(!transitions_enabled_by(true, false, &none));
        assert!(!transitions_enabled_by(true, false, &movement));

        // and it needs ghosts
        assert!(!transitions_enabled_by(false, true, &none));
        assert!(!transitions_enabled_by(false, false, &workspace));
    }

    #[test]
    fn the_duration_and_style_are_the_ones_of_the_kind_or_else_the_global_ones() {
        let durations = HashMap::from([(AnimationPrefix::Workspace, 400)]);

        assert_eq!(
            duration_for(AnimationPrefix::Workspace, &durations, 250),
            Duration::from_millis(400)
        );
        assert_eq!(
            duration_for(AnimationPrefix::Movement, &durations, 250),
            Duration::from_millis(250)
        );
        assert_eq!(
            duration_for(AnimationPrefix::Workspace, &HashMap::new(), 250),
            Duration::from_millis(250)
        );

        let styles = HashMap::from([(AnimationPrefix::Workspace, AnimationStyle::EaseOutSine)]);

        assert_eq!(
            style_for(AnimationPrefix::Workspace, &styles, AnimationStyle::Linear),
            AnimationStyle::EaseOutSine
        );
        assert_eq!(
            style_for(AnimationPrefix::Movement, &styles, AnimationStyle::Linear),
            AnimationStyle::Linear
        );
    }

    #[test]
    fn the_workspace_kind_is_named_in_the_configuration() {
        assert_eq!(AnimationPrefix::Workspace.to_string(), "workspace");
        assert_eq!(
            "workspace".parse::<AnimationPrefix>().unwrap(),
            AnimationPrefix::Workspace
        );
        assert_eq!(
            serde_json::from_str::<AnimationPrefix>("\"workspace\"").unwrap(),
            AnimationPrefix::Workspace
        );
    }
}
