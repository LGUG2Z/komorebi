//! The part of the settings that is the same struct on every platform.

use komorebi_themes::colour::Colour;
use komorebi_themes::colour::Rgb;

/// The defaults a daemon starts with. They differ per platform today (border width 8 / 10 / 10,
/// padding 10 / 5 / 5, ...) and this step does not change any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlatformDefaults {
    pub border_width: i32,
    pub border_offset: i32,
    pub border_radius: i32,
}

impl PlatformDefaults {
    /// `komorebi/src/border_manager/mod.rs`; Windows has no border radius setting (the unused
    /// value is the one the other platforms use).
    pub const WINDOWS: Self = Self {
        border_width: 8,
        border_offset: -1,
        border_radius: 10,
    };
    /// `komorebi-for-kde/komorebi/src/border_manager/mod.rs`
    pub const KDE: Self = Self {
        border_width: 10,
        border_offset: 0,
        border_radius: 10,
    };
    /// `komorebi-for-mac/komorebi/src/border_manager/mod.rs`
    pub const MACOS: Self = Self {
        border_width: 10,
        border_offset: 0,
        border_radius: 10,
    };
}

/// Colours are `u32` exactly as the border statics hold them (`u32::from(Colour)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BorderColours {
    pub single: u32,
    pub stack: u32,
    pub monocle: u32,
    pub floating: u32,
    pub unfocused: u32,
    pub unfocused_locked: u32,
}

/// Whether and how the borders are drawn; the colours are the same on all three platforms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Border {
    pub enabled: bool,
    pub width: i32,
    pub offset: i32,
    /// Used by KDE and macOS; Windows keeps it but never reads it.
    pub radius: i32,
    pub colours: BorderColours,
}

/// Every setting that is not tied to one platform.
#[derive(Clone, Debug, PartialEq)]
pub struct Shared {
    pub border: Border,
}

impl Shared {
    /// The values the statics hold before any configuration is read.
    pub fn defaults_for(defaults: PlatformDefaults) -> Self {
        Self {
            border: Border {
                enabled: true,
                width: defaults.border_width,
                offset: defaults.border_offset,
                radius: defaults.border_radius,
                colours: BorderColours {
                    single: rgb(66, 165, 245),
                    stack: rgb(0, 165, 66),
                    monocle: rgb(255, 51, 153),
                    floating: rgb(245, 245, 165),
                    unfocused: rgb(128, 128, 128),
                    unfocused_locked: rgb(158, 8, 8),
                },
            },
        }
    }
}

fn rgb(r: u32, g: u32, b: u32) -> u32 {
    u32::from(Colour::Rgb(Rgb::new(r, g, b)))
}
