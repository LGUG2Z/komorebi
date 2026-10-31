//! The settings of this daemon, as one snapshot (plan step 10, `docs/dev/settings.md`).
//!
//! Nothing reads this yet: the process-wide statics are still the source of truth, and each
//! migration step moves one group of them (writers and readers together) into
//! [`settings()`]. Until then the value is the starting values of those statics.

use std::sync::LazyLock;

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

        assert_eq!(
            shared.defaults.container_padding,
            crate::DEFAULT_CONTAINER_PADDING.load(Ordering::SeqCst)
        );
        assert_eq!(
            shared.defaults.workspace_padding,
            crate::DEFAULT_WORKSPACE_PADDING.load(Ordering::SeqCst)
        );
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
