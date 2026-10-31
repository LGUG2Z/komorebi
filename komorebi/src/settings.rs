//! The settings of this daemon, as one snapshot (plan step 10, `docs/dev/settings.md`).
//!
//! Each migration step moves one group of the process-wide statics (writers and readers together)
//! into [`settings()`]. Moved so far: the two default paddings, the aspect ratio of a window toggled
//! to floating, `layout_defaults` and `display_index_preferences`. The groups that have not moved
//! are still statics and the pin test compares them with the starting settings.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use crate::animation::prefix::AnimationPrefix;
use crate::core::animation::AnimationStyle;

use komorebi_config::aspect_ratio::AspectRatio;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::LayoutDefaultEntry;

use komorebi_settings::Handle;
use komorebi_settings::Shared;
use komorebi_settings::shared::PlatformDefaults;

/// The part of the settings that only Windows has. Empty for now; the stackbar, hiding and handling behaviour and border style join it later.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowsSettings {}

pub type Settings = komorebi_settings::Settings<WindowsSettings>;

static SETTINGS: LazyLock<Handle<WindowsSettings>> = LazyLock::new(|| {
    Handle::new(Settings::new(
        Shared::defaults_for(PlatformDefaults::WINDOWS),
        WindowsSettings::default(),
    ))
});

/// The settings of the running daemon: take a [`Handle::snapshot`] once per unit of work.
pub fn settings() -> &'static Handle<WindowsSettings> {
    &SETTINGS
}

// Accessors for the groups that have moved over (S3). Each takes one snapshot, so a caller sees one
// configuration, and returns an owned value: the callers are configuration and monitor-change
// paths, not hot paths.

/// `default_container_padding`: the padding around each tile for a monitor that sets none.
pub fn container_padding() -> i32 {
    settings().snapshot().shared.defaults.container_padding
}

/// `default_workspace_padding`: the padding inside a work area for a monitor that sets none.
pub fn workspace_padding() -> i32 {
    settings().snapshot().shared.defaults.workspace_padding
}

/// `floating_window_aspect_ratio`: the shape of a window that is toggled to floating and centred.
pub fn floating_window_aspect_ratio() -> AspectRatio {
    settings().snapshot().shared.floating_window_aspect_ratio
}

/// `layout_defaults`: what each layout starts with when a workspace sets nothing.
pub fn layout_defaults() -> HashMap<DefaultLayout, LayoutDefaultEntry> {
    settings().snapshot().shared.layout_defaults.clone()
}

/// `display_index_preferences`: which monitor each configured index belongs to.
pub fn display_index_preferences() -> HashMap<usize, String> {
    settings()
        .snapshot()
        .shared
        .display_index_preferences
        .clone()
}

/// `transparency`: whether windows that are not focused are made see-through.
pub fn transparency_enabled() -> bool {
    settings().snapshot().shared.transparency.enabled
}

/// `transparency_alpha`: the opacity, 0 to 255, of a see-through window.
pub fn transparency_alpha() -> u8 {
    settings().snapshot().shared.transparency.alpha
}

// The animation settings. Windows decides whether a kind of animation is on as: the global flag, or
// a per-kind value of true (a per-kind false does NOT turn off a global true, unlike KDE and macOS).

/// Whether `prefix` is animated: the global flag, or a per-kind true.
pub fn animation_enabled_for(prefix: AnimationPrefix) -> bool {
    let snapshot = settings().snapshot();
    let animation = &snapshot.shared.animation;

    animation.enabled
        || animation
            .enabled_per_animation
            .get(&prefix)
            .is_some_and(|enabled| *enabled)
}

/// How `prefix` is animated, from one snapshot, or `None` if it is not (see [`animation_enabled_for`]).
pub fn animation_for(prefix: AnimationPrefix) -> Option<(Duration, AnimationStyle)> {
    let snapshot = settings().snapshot();
    let animation = &snapshot.shared.animation;

    let enabled = animation.enabled
        || animation
            .enabled_per_animation
            .get(&prefix)
            .is_some_and(|enabled| *enabled);

    enabled.then(|| {
        (
            Duration::from_millis(
                animation
                    .duration_per_animation
                    .get(&prefix)
                    .copied()
                    .unwrap_or(animation.duration),
            ),
            animation
                .style_per_animation
                .get(&prefix)
                .copied()
                .unwrap_or(animation.style),
        )
    })
}

/// `animation.fps`.
pub fn animation_fps() -> u64 {
    settings().snapshot().shared.animation.fps
}

/// The global `animation.duration`, in milliseconds.
pub fn animation_duration_global() -> u64 {
    settings().snapshot().shared.animation.duration
}

/// Turns every animation off for good (the global flag and the per-kind values): what shutdown does
/// before it puts the windows back.
pub fn animation_off_for_good() {
    settings().update(|settings| {
        settings.shared.animation.enabled_per_animation.clear();
        settings.shared.animation.enabled = false;
    });
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::border_manager;

    // The statics are still the source of truth. These pin that the starting values of the
    // settings are the starting values of the statics, so that moving a group over changes nothing.
    // (They read the statics without any test writing them first: no test here sets them.)
    #[test]
    fn the_starting_settings_are_the_starting_values_of_the_statics() {
        let shared = &settings().snapshot().shared;

        assert_eq!(shared.defaults.resize_delta, crate::DEFAULT_RESIZE_DELTA);
        assert_eq!(
            shared.defaults.mouse_follows_focus,
            crate::DEFAULT_MOUSE_FOLLOWS_FOCUS
        );
        assert_eq!(
            shared.border.enabled,
            border_manager::BORDER_ENABLED.load(Ordering::SeqCst)
        );
        assert_eq!(
            shared.border.width,
            border_manager::BORDER_WIDTH.load(Ordering::SeqCst)
        );
        assert_eq!(
            shared.border.offset,
            border_manager::BORDER_OFFSET.load(Ordering::SeqCst)
        );
        let colours = shared.border.colours;
        assert_eq!(
            colours.single,
            border_manager::FOCUSED.load(Ordering::SeqCst)
        );
        assert_eq!(colours.stack, border_manager::STACK.load(Ordering::SeqCst));
        assert_eq!(
            colours.monocle,
            border_manager::MONOCLE.load(Ordering::SeqCst)
        );
        assert_eq!(
            colours.floating,
            border_manager::FLOATING.load(Ordering::SeqCst)
        );
        assert_eq!(
            colours.unfocused,
            border_manager::UNFOCUSED.load(Ordering::SeqCst)
        );
        assert_eq!(
            colours.unfocused_locked,
            border_manager::UNFOCUSED_LOCKED.load(Ordering::SeqCst)
        );
    }
}
