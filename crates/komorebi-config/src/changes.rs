//! What each platform's schema says differently from the union, as data.
//!
//! The union is written once (Windows' text, Windows' defaults); a view applies the changes of
//! its platform after pruning. They are of three kinds: descriptions that read differently on
//! a platform (spelling, or a sentence about that platform), default values that are a platform
//! fact (paddings, border width and offset), and properties the platform's file never had. Each
//! entry is a candidate for convergence: removing one is a description or default change of one
//! checked-in schema, which the test of the schemas shows.
//!
//! Generated from the difference between the union's view and the checked-in schema.json of
//! each tree (the helper is `tools/schema-changes` in the work notes); the test `tests/schemas.rs`
//! proves that view and file are byte-identical.

use komorebi_protocol::platforms::Platform;

/// One change of a view
#[derive(Clone, Copy, Debug)]
pub enum Change {
    /// Replace the value at the JSON pointer with this JSON text
    Set(&'static str),
    /// Remove the key at the JSON pointer
    Remove,
}

/// The changes a platform's view applies, in order
#[must_use]
pub const fn for_platform(platform: Platform) -> &'static [(&'static str, Change)] {
    match platform {
        Platform::Windows => WINDOWS,
        Platform::Kde => KDE,
        Platform::MacOs => MACOS,
    }
}

const WINDOWS: &[(&str, Change)] = &[(
    "/$defs/PlacementTarget",
    Change::Set(
        r####"{"description":"A target position for window placement, used as a key in `InitialWindowPlacementRules::Rules`.\n\nCan be either a `WindowPlacement` variant name (e.g. `\"Primary\"`, `\"Last\"`)\nor a 1-based container index (e.g. `\"1\"`, `\"3\"`, or the integer `3`).\n\nNOTE: Integer indices are 1-based in the config for user-friendliness.","anyOf":[{"$ref":"#/$defs/WindowPlacement"},{"description":"A 1-based container index as a string","type":"string","pattern":"^[0-9]+$"},{"description":"A 1-based container index","type":"integer","minimum":0}]}"####,
    ),
)];

const KDE: &[(&str, Change)] = &[
    (
        "/$defs/AnimationsConfig/description",
        Change::Set(r####""Animation configuration""####),
    ),
    (
        "/$defs/AppSpecificConfigurationPath/anyOf/0/description",
        Change::Set(r####""A single applications.json file""####),
    ),
    (
        "/$defs/AppSpecificConfigurationPath/anyOf/1/description",
        Change::Set(r####""Multiple applications.json files""####),
    ),
    (
        "/$defs/BorderColours/description",
        Change::Set(
            r####""Border colours for different container states\n\nThese options will have no effect if a `theme` is set""####,
        ),
    ),
    (
        "/$defs/MonitorConfig/properties/container_padding/description",
        Change::Set(r####""Container padding""####),
    ),
    (
        "/$defs/MonitorConfig/properties/window_hiding_position/description",
        Change::Set(
            r####""Accepted so that a configuration written for another platform loads, and ignored: there is\nno hiding position on KDE Plasma""####,
        ),
    ),
    (
        "/$defs/ThemeOptions/properties/floating_border/description",
        Change::Set(r####""Border color when the window is floating""####),
    ),
    (
        "/$defs/ThemeOptions/properties/monocle_border/description",
        Change::Set(r####""Border color when the container is in monocle mode""####),
    ),
    (
        "/$defs/ThemeOptions/properties/single_border/description",
        Change::Set(r####""Border color when the container contains a single window""####),
    ),
    (
        "/$defs/ThemeOptions/properties/stack_border/description",
        Change::Set(r####""Border color when the container contains multiple windows""####),
    ),
    (
        "/$defs/ThemeOptions/properties/unfocused_border/description",
        Change::Set(r####""Border color when the container is unfocused""####),
    ),
    (
        "/$defs/ThemeOptions/properties/unfocused_locked_border/description",
        Change::Set(r####""Border color when the container is unfocused and locked""####),
    ),
    (
        "/$defs/Wallpaper/properties/theme_options/description",
        Change::Set(r####""Specify theme options""####),
    ),
    (
        "/$defs/WorkspaceConfig/properties/window_container_behaviour_rules/description",
        Change::Set(
            r####""Window container behavior rules in the format of threshold => behavior""####,
        ),
    ),
    (
        "/description",
        Change::Set(
            r####""The `komorebi.json` static configuration file reference for komorebi for KDE Plasma""####,
        ),
    ),
    (
        "/properties/app_specific_configuration_path/description",
        Change::Set(
            r####""Path to an applications.json, or a list of them, with rules for the applications that\nneed them: which windows to ignore, to manage and to float""####,
        ),
    ),
    (
        "/properties/border_colours/description",
        Change::Set(
            r####""Window border colours for different container types (has no effect if a theme is defined)""####,
        ),
    ),
    (
        "/properties/border_offset/default",
        Change::Set(r####"0"####),
    ),
    (
        "/properties/border_width/default",
        Change::Set(r####"10"####),
    ),
    (
        "/properties/default_container_padding/default",
        Change::Set(r####"5"####),
    ),
    (
        "/properties/default_workspace_padding/default",
        Change::Set(r####"5"####),
    ),
    (
        "/properties/float_rule_placement/description",
        Change::Set(
            r####""Determines the `Placement` to be used when spawning a window that matches a\n'floating_applications' rule""####,
        ),
    ),
    (
        "/properties/focus_follows_mouse/description",
        Change::Set(
            r####""Focus the window under the pointer. On KDE this is KWin's own focus policy, whichever\nimplementation is named""####,
        ),
    ),
    (
        "/properties/tabbed_applications/description",
        Change::Set(
            r####""Accepted so that a configuration written for another platform loads, and ignored: a KWin\nwindow has no native tabs""####,
        ),
    ),
    (
        "/properties/titleless_applications/description",
        Change::Set(
            r####""Accepted so that a configuration written for another platform loads, and ignored: a KWin\nwindow is always a window""####,
        ),
    ),
    (
        "/properties/transparency/description",
        Change::Set(r####""Add transparency to unfocused windows (default: false)""####),
    ),
    (
        "/properties/transparency_alpha/description",
        Change::Set(
            r####""Alpha value for unfocused window transparency [[0-255]] (default: 200)""####,
        ),
    ),
    (
        "/$defs/KomorebiThemeBase16/properties/stackbar_background",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeBase16/properties/stackbar_focused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeBase16/properties/stackbar_unfocused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCatppuccin/properties/stackbar_background",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCatppuccin/properties/stackbar_focused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCatppuccin/properties/stackbar_unfocused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCustom/properties/stackbar_background",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCustom/properties/stackbar_focused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCustom/properties/stackbar_unfocused_text",
        Change::Remove,
    ),
    (
        "/$defs/Wallpaper/properties/theme_options/default",
        Change::Remove,
    ),
    ("/properties/focus_follows_mouse/deprecated", Change::Remove),
    ("/properties/transparency/default", Change::Remove),
    ("/properties/transparency_alpha/default", Change::Remove),
];

const MACOS: &[(&str, Change)] = &[
    (
        "/$defs/AnimationsConfig/description",
        Change::Set(r####""Animation configuration""####),
    ),
    (
        "/$defs/AppSpecificConfigurationPath/anyOf/0/description",
        Change::Set(r####""A single applications.json file""####),
    ),
    (
        "/$defs/AppSpecificConfigurationPath/anyOf/1/description",
        Change::Set(r####""Multiple applications.json files""####),
    ),
    (
        "/$defs/BorderColours/description",
        Change::Set(
            r####""Border colours for different container states\n\nThese options will have no effect if a `theme` is set""####,
        ),
    ),
    (
        "/$defs/MonitorConfig/properties/container_padding/description",
        Change::Set(r####""Container padding""####),
    ),
    (
        "/$defs/Wallpaper/properties/theme_options/description",
        Change::Set(r####""Specify theme options""####),
    ),
    (
        "/description",
        Change::Set(
            r####""The `komorebi.json` static configuration file reference for komorebi for Mac `v0.1.0`\n\n`\"$schema\": \"https://komorebi-for-mac.lgug2z.com/komorebi.0.1.0.schema.json\"`""####,
        ),
    ),
    (
        "/properties/border_colours/description",
        Change::Set(
            r####""Window border colours for different container types (has no effect if a theme is defined)""####,
        ),
    ),
    (
        "/properties/border_offset/default",
        Change::Set(r####"0"####),
    ),
    (
        "/properties/border_width/default",
        Change::Set(r####"10"####),
    ),
    (
        "/properties/default_container_padding/default",
        Change::Set(r####"5"####),
    ),
    (
        "/properties/default_workspace_padding/default",
        Change::Set(r####"5"####),
    ),
    (
        "/properties/float_rule_placement/description",
        Change::Set(
            r####""Determines the `Placement` to be used when spawning a window that matches a\n'floating_applications' rule""####,
        ),
    ),
    (
        "/properties/floating_window_aspect_ratio/description",
        Change::Set(
            r####""Identify applications which are slow to send initial event notifications\nAspect ratio to resize with when toggling floating mode for a window""####,
        ),
    ),
    (
        "/$defs/KomorebiThemeBase16/properties/stackbar_background",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeBase16/properties/stackbar_focused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeBase16/properties/stackbar_unfocused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCatppuccin/properties/stackbar_background",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCatppuccin/properties/stackbar_focused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCatppuccin/properties/stackbar_unfocused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCustom/properties/stackbar_background",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCustom/properties/stackbar_focused_text",
        Change::Remove,
    ),
    (
        "/$defs/KomorebiThemeCustom/properties/stackbar_unfocused_text",
        Change::Remove,
    ),
    (
        "/$defs/Wallpaper/properties/theme_options/default",
        Change::Remove,
    ),
];
