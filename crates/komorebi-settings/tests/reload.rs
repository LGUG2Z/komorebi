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
