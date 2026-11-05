//! The settings of this daemon, as one snapshot (plan step 10, `docs/dev/settings.md`).
//!
//! Each migration step moves one group of the process-wide statics (writers and readers together)
//! into [`settings()`]. Moved so far: the two default paddings, the aspect ratio of a window toggled
//! to floating, `layout_defaults` and `display_index_preferences`. The groups that have not moved
//! are still statics and the pin test compares them with the starting settings.

use std::collections::HashMap;
use std::sync::LazyLock;

use komorebi_config::aspect_ratio::AspectRatio;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::LayoutDefaultEntry;

use komorebi_settings::Handle;
use komorebi_settings::Shared;
use komorebi_settings::shared::PlatformDefaults;

/// The part of the settings that only this platform has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MacosSettings {}

pub type Settings = komorebi_settings::Settings<MacosSettings>;

static SETTINGS: LazyLock<Handle<MacosSettings>> = LazyLock::new(|| {
    Handle::new(Settings::new(
        Shared::defaults_for(PlatformDefaults::MACOS),
        MacosSettings::default(),
    ))
});

/// The settings of the running daemon: take a [`Handle::snapshot`] once per unit of work.
pub fn settings() -> &'static Handle<MacosSettings> {
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

/// `animation.fps`.
pub fn animation_fps() -> u64 {
    settings().snapshot().shared.animation.fps
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
        assert_eq!(
            shared.border.radius,
            border_manager::BORDER_RADIUS.load(Ordering::SeqCst)
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
