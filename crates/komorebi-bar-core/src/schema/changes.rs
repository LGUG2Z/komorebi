//! What the macOS bar's schema says differently from the union, as data.
//!
//! The union is written once: it is the Windows text of `schema.bar.json` (every widget, the
//! mouse options, Windows' spelling of the documentation). The macOS view removes what the macOS
//! bar does not have (the keyboard, media and systray widgets, the mouse options and their
//! definitions, which pruning of the unreachable definitions finishes) and sets the text and the
//! defaults that read differently. Each entry is a convergence candidate: dropping one changes
//! one line of `komorebi-for-mac/schema.bar.json` and of `nix/komorebi-bar-options.nix`.
//!
//! Generated from the difference between the two checked-in files; `tests/schema_oracle.rs`
//! proves that the view of the Windows file is byte-identical to the macOS file.

/// One change of a view
#[derive(Clone, Copy, Debug)]
pub enum Change {
    /// Replace the value at the JSON pointer with this JSON text
    Set(&'static str),
    /// Remove the key at the JSON pointer
    Remove,
    /// Remove the elements of the array at the JSON pointer whose `title` is this
    RemoveTitled(&'static str),
}

/// The changes of the macOS view, in order.
pub const MACOS: &[(&str, Change)] = &[
    (
        "/description",
        Change::Set(
            r####""The `komorebi.bar.json` static configuration file reference for komorebi for Mac `v0.1.0`\n\n`\"$schema\": \"https://komorebi-for-mac.lgug2z.com/komorebi.bar.0.1.0.schema.json\"`""####,
        ),
    ),
    (
        "/properties/height",
        Change::Set(
            r####"{"description":"Bar height","type":["number","null"],"format":"float","default":50.0}"####,
        ),
    ),
    (
        "/properties/margin",
        Change::Set(
            r####"{"description":"Bar margin. Use one value for all sides or use a grouped margin for horizontal and/or\nvertical definition which can each take a single value for a symmetric margin or two\nvalues for each side, i.e.:\n```json\n\"margin\": {\n    \"horizontal\": 10.0\n}\n```\nor:\n```json\n\"margin\": {\n    \"vertical\": [top, bottom]\n}\n```\nYou can also set individual margin on each side like this:\n```json\n\"margin\": {\n    \"top\": 10.0,\n    \"bottom\": 10.0,\n    \"left\": 10.0,\n    \"right\": 10.0\n}\n```","anyOf":[{"$ref":"#/$defs/SpacingKind"},{"type":"null"}]}"####,
        ),
    ),
    (
        "/properties/padding",
        Change::Set(
            r####"{"description":"Bar padding. Use one value for all sides or use a grouped padding for horizontal and/or\nvertical definition which can each take a single value for a symmetric padding or two\nvalues for each side, i.e.:\n```json\n\"padding\": {\n    \"horizontal\": 10.0\n}\n```\nor:\n```json\n\"padding\": {\n    \"horizontal\": [left, right]\n}\n```\nYou can also set individual padding on each side like this:\n```json\n\"padding\": {\n    \"top\": 10.0,\n    \"bottom\": 10.0,\n    \"left\": 10.0,\n    \"right\": 10.0\n}\n```","anyOf":[{"$ref":"#/$defs/SpacingKind"},{"type":"null"}]}"####,
        ),
    ),
    ("/properties/mouse", Change::Remove),
    (
        "/$defs/KomorebiFocusedContainerConfig/properties/show_icon",
        Change::Set(
            r####"{"description":"DEPRECATED: use 'display' instead (Show the icon of the currently focused container)","type":["boolean","null"]}"####,
        ),
    ),
    (
        "/$defs/KomorebiLayout",
        Change::Set(
            r####"{"description":"Komorebi layout kind","oneOf":[{"title":"Default","description":"Predefined layout","type":"object","properties":{"Default":{"$ref":"#/$defs/DefaultLayout"}},"additionalProperties":false,"required":["Default"]},{"description":"Monocle mode","type":"string","const":"Monocle"},{"description":"Floating layer","type":"string","const":"Floating"},{"description":"Paused","type":"string","const":"Paused"},{"description":"Custom layout","type":"string","const":"Custom"}]}"####,
        ),
    ),
    (
        "/$defs/NetworkConfig/properties/default_interface_options",
        Change::Remove,
    ),
    (
        "/$defs/StorageDisplayName/oneOf/0/description",
        Change::Set(r####""Display label as mount point eg. /""####),
    ),
    (
        "/$defs/StorageDisplayName/oneOf/1/description",
        Change::Set(r####""Display label as name eg. Macintosh HD""####),
    ),
    (
        "/$defs/StorageDisplayName/oneOf/2/description",
        Change::Set(r####""Display label as mount then name eg. / Macintosh HD""####),
    ),
    (
        "/$defs/StorageDisplayName/oneOf/3/description",
        Change::Set(r####""Display label as name then mount eg. Macintosh HD /""####),
    ),
    (
        "/$defs/TimeConfig/properties/changing_icon",
        Change::Set(
            r####"{"description":"Change the icon depending on the time. The default icon is used between 8:30 and 12:00","type":["boolean","null"],"default":false}"####,
        ),
    ),
    (
        "/$defs/TimeFormat/oneOf/6/title",
        Change::Set(r####""Default""####),
    ),
    (
        "/$defs/WidgetConfig/oneOf",
        Change::RemoveTitled("Keyboard"),
    ),
    ("/$defs/WidgetConfig/oneOf", Change::RemoveTitled("Media")),
    ("/$defs/WidgetConfig/oneOf", Change::RemoveTitled("Systray")),
];
