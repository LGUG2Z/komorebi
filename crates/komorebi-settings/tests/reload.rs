//! A configuration load through a `Handle`, the way the daemons apply `apply_globals` for the groups
//! that have moved (paddings, aspect ratio, layout defaults, display index preferences): a reload
//! merges, a few keys reset. Section 4 of `docs/dev/settings.md`.

use std::collections::HashMap;

use komorebi_config::aspect_ratio::AspectRatio;
use komorebi_config::aspect_ratio::PredefinedAspectRatio;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::LayoutDefaultEntry;
use komorebi_settings::Handle;
use komorebi_settings::Settings;
use komorebi_settings::Shared;
use komorebi_settings::shared::PlatformDefaults;
use komorebi_settings::shared::SharedDelta;

fn handle() -> Handle<()> {
    Handle::new(Settings::new(
        Shared::defaults_for(PlatformDefaults::WINDOWS),
        (),
    ))
}

fn load(handle: &Handle<()>, delta: SharedDelta) {
    handle.update(|settings| delta.apply(&mut settings.shared));
}

fn entry() -> LayoutDefaultEntry {
    LayoutDefaultEntry {
        layout_options: None,
        layout_options_rules: None,
    }
}

#[test]
fn the_first_load_without_the_keys_changes_widescreen_to_standard() {
    let handle = handle();
    assert_eq!(
        handle.snapshot().shared.floating_window_aspect_ratio,
        AspectRatio::Predefined(PredefinedAspectRatio::Widescreen)
    );

    load(&handle, SharedDelta::default());

    assert_eq!(
        handle.snapshot().shared.floating_window_aspect_ratio,
        AspectRatio::Predefined(PredefinedAspectRatio::Standard)
    );
}

#[test]
fn a_reload_merges_what_the_file_gives_and_resets_what_it_resets() {
    let handle = handle();

    // load 1 gives everything
    load(
        &handle,
        SharedDelta {
            default_container_padding: Some(7),
            default_workspace_padding: Some(8),
            floating_window_aspect_ratio: Some(AspectRatio::Custom(3, 2)),
            layout_defaults: Some(HashMap::from([(DefaultLayout::BSP, entry())])),
            display_index_preferences: Some(HashMap::from([(0, String::from("a"))])),
            ..SharedDelta::default()
        },
    );
    let first = handle.snapshot();
    assert_eq!(first.shared.defaults.container_padding, 7);
    assert_eq!(first.shared.defaults.workspace_padding, 8);
    assert_eq!(
        first.shared.floating_window_aspect_ratio,
        AspectRatio::Custom(3, 2)
    );
    assert_eq!(first.shared.layout_defaults.len(), 1);

    // load 2 gives only a workspace padding
    load(
        &handle,
        SharedDelta {
            default_workspace_padding: Some(1),
            ..SharedDelta::default()
        },
    );
    let second = handle.snapshot();
    // kept: container padding, display index preferences
    assert_eq!(second.shared.defaults.container_padding, 7);
    assert_eq!(
        second.shared.display_index_preferences,
        HashMap::from([(0, String::from("a"))])
    );
    // replaced
    assert_eq!(second.shared.defaults.workspace_padding, 1);
    // reset: layout defaults cleared, aspect ratio Standard
    assert!(second.shared.layout_defaults.is_empty());
    assert_eq!(
        second.shared.floating_window_aspect_ratio,
        AspectRatio::Predefined(PredefinedAspectRatio::Standard)
    );

    // a reader that held the first snapshot still sees the first load, whole
    assert_eq!(first.shared.defaults.workspace_padding, 8);
    assert_eq!(first.shared.layout_defaults.len(), 1);
}

#[test]
fn a_load_never_touches_the_groups_it_does_not_name() {
    let handle = handle();
    handle.update(|settings| {
        settings.shared.border.width = 42;
        settings.shared.animation.fps = 144;
        settings.shared.transparency.alpha = 9;
    });
    let before = handle.snapshot();

    load(
        &handle,
        SharedDelta {
            default_container_padding: Some(3),
            ..SharedDelta::default()
        },
    );

    let after = handle.snapshot();
    assert_eq!(after.shared.border, before.shared.border);
    assert_eq!(after.shared.animation, before.shared.animation);
    assert_eq!(after.shared.transparency, before.shared.transparency);
}

#[test]
fn transparency_is_kept_by_a_load_without_the_key_unless_the_daemon_resets_it() {
    // Windows passes the keys as they are: absent keeps the value
    let windows = handle();
    load(
        &windows,
        SharedDelta {
            transparency: Some(true),
            transparency_alpha: Some(90),
            ..SharedDelta::default()
        },
    );
    load(&windows, SharedDelta::default());
    let kept = windows.snapshot().shared.transparency;
    assert!(kept.enabled);
    assert_eq!(kept.alpha, 90);

    // KDE passes `unwrap_or(false)` and `unwrap_or(200)`: absent resets to the defaults
    let kde = handle();
    load(
        &kde,
        SharedDelta {
            transparency: Some(true),
            transparency_alpha: Some(90),
            ..SharedDelta::default()
        },
    );
    let (transparency, alpha): (Option<bool>, Option<u8>) = (None, None);
    load(
        &kde,
        SharedDelta {
            transparency: Some(transparency.unwrap_or(false)),
            transparency_alpha: Some(alpha.unwrap_or(200)),
            ..SharedDelta::default()
        },
    );
    let reset = kde.snapshot().shared.transparency;
    assert!(!reset.enabled);
    assert_eq!(reset.alpha, 200);
}

#[test]
fn the_animation_block_follows_the_load_rules() {
    use komorebi_config::animation::PerAnimationPrefixConfig;
    use komorebi_protocol::animation::AnimationStyle;
    use komorebi_protocol::animation::prefix::AnimationPrefix;
    use komorebi_settings::shared::AnimationDelta;

    let handle = handle();
    let block = |enabled, fps| SharedDelta {
        animation: Some(AnimationDelta {
            enabled,
            style: None,
            duration: Some(PerAnimationPrefixConfig::Global(120)),
            fps,
        }),
        ..SharedDelta::default()
    };

    // a per-kind map replaces the map, the global flag is left alone; fps given
    load(
        &handle,
        block(
            PerAnimationPrefixConfig::Prefix(HashMap::from([(AnimationPrefix::Movement, true)])),
            Some(30),
        ),
    );
    let first = handle.snapshot().shared.animation.clone();
    assert!(!first.enabled);
    assert_eq!(first.enabled_per_animation.len(), 1);
    assert_eq!(first.duration, 120);
    assert_eq!(first.fps, 30);
    assert_eq!(first.style, AnimationStyle::Linear);

    // a load with no animation block keeps all of it
    load(&handle, SharedDelta::default());
    assert_eq!(handle.snapshot().shared.animation, first);

    // a global flag clears the map, and a block without fps resets fps
    load(&handle, block(PerAnimationPrefixConfig::Global(true), None));
    let second = handle.snapshot().shared.animation.clone();
    assert!(second.enabled);
    assert!(second.enabled_per_animation.is_empty());
    assert_eq!(second.fps, 60);
}
