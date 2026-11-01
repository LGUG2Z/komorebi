//! The part of the settings that is the same struct on every platform.
//!
//! Every field here has a counterpart static in each daemon (`docs/dev/settings.md`, section 2.1),
//! and [`SharedDelta::apply`] reproduces what `StaticConfig::apply_globals` does with it, including
//! which absent keys keep the old value and which are reset.

use std::collections::HashMap;

use komorebi_config::animation::PerAnimationPrefixConfig;
use komorebi_config::aspect_ratio::AspectRatio;
use komorebi_config::aspect_ratio::PredefinedAspectRatio;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::LayoutDefaultEntry;
use komorebi_protocol::animation::AnimationStyle;
use komorebi_protocol::animation::prefix::AnimationPrefix;
use komorebi_themes::colour::Colour;
use komorebi_themes::colour::Rgb;

/// What `ANIMATION_FPS` becomes when `animation` is configured without `fps`
/// (`DEFAULT_ANIMATION_FPS` in each daemon).
pub const DEFAULT_ANIMATION_FPS: u64 = 60;
/// `DEFAULT_ANIMATION_ENABLED`, `DEFAULT_ANIMATION_STYLE` and `DEFAULT_ANIMATION_DURATION`.
pub const DEFAULT_ANIMATION_ENABLED: bool = false;
pub const DEFAULT_ANIMATION_STYLE: AnimationStyle = AnimationStyle::Linear;
pub const DEFAULT_ANIMATION_DURATION: u64 = 250;
/// `DEFAULT_RESIZE_DELTA` and `DEFAULT_MOUSE_FOLLOWS_FOCUS` (equal on all three platforms).
pub const DEFAULT_RESIZE_DELTA: i32 = 50;
pub const DEFAULT_MOUSE_FOLLOWS_FOCUS: bool = true;

/// The defaults a daemon starts with. They differ per platform today (border width 8 / 10 / 10,
/// padding 10 / 5 / 5, ...) and this step does not change any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlatformDefaults {
    pub border_width: i32,
    pub border_offset: i32,
    pub border_radius: i32,
    pub container_padding: i32,
    pub workspace_padding: i32,
}

impl PlatformDefaults {
    /// `komorebi/src/border_manager/mod.rs` and `komorebi/src/lib.rs`. Windows has no border
    /// radius setting; the unused value is the one the other platforms use.
    pub const WINDOWS: Self = Self {
        border_width: 8,
        border_offset: -1,
        border_radius: 10,
        container_padding: 10,
        workspace_padding: 10,
    };
    /// `komorebi-linux/src/border_manager/mod.rs` and `.../lib.rs`
    pub const KDE: Self = Self {
        border_width: 10,
        border_offset: 0,
        border_radius: 10,
        container_padding: 5,
        workspace_padding: 5,
    };
    /// `komorebi-macos/src/border_manager/mod.rs` and `.../lib.rs`
    pub const MACOS: Self = Self {
        border_width: 10,
        border_offset: 0,
        border_radius: 10,
        container_padding: 5,
        workspace_padding: 5,
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

/// The animation settings: a global value for each of enabled, style and duration, and a map of
/// per-prefix values that wins over the global one.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub enabled: bool,
    pub style: AnimationStyle,
    pub duration: u64,
    pub fps: u64,
    pub enabled_per_animation: HashMap<AnimationPrefix, bool>,
    pub style_per_animation: HashMap<AnimationPrefix, AnimationStyle>,
    pub duration_per_animation: HashMap<AnimationPrefix, u64>,
}

/// How a window is moved for one kind of animation: the three values `motion_for` reads today as
/// three separate statics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub duration_ms: u64,
    pub style: AnimationStyle,
}

impl Animation {
    /// `None` if `prefix` is not animated, else how. The same rule as `motion_for` (without the
    /// suspension counter, which is runtime state and stays with the animation module), now read
    /// from one snapshot.
    pub fn motion_for(&self, prefix: AnimationPrefix) -> Option<Motion> {
        let enabled = self
            .enabled_per_animation
            .get(&prefix)
            .copied()
            .unwrap_or(self.enabled);

        enabled.then(|| Motion {
            duration_ms: self
                .duration_per_animation
                .get(&prefix)
                .copied()
                .unwrap_or(self.duration),
            style: self
                .style_per_animation
                .get(&prefix)
                .copied()
                .unwrap_or(self.style),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Defaults {
    pub container_padding: i32,
    pub workspace_padding: i32,
    pub resize_delta: i32,
    pub mouse_follows_focus: bool,
}

/// `transparency` and `transparency_alpha` (Windows and KDE; macOS never reads them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transparency {
    pub enabled: bool,
    pub alpha: u8,
}

/// Every setting that is not tied to one platform.
#[derive(Clone, Debug, PartialEq)]
pub struct Shared {
    pub border: Border,
    pub animation: Animation,
    pub defaults: Defaults,
    pub transparency: Transparency,
    pub floating_window_aspect_ratio: AspectRatio,
    pub layout_defaults: HashMap<DefaultLayout, LayoutDefaultEntry>,
    pub display_index_preferences: HashMap<usize, String>,
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
            animation: Animation {
                enabled: DEFAULT_ANIMATION_ENABLED,
                style: DEFAULT_ANIMATION_STYLE,
                duration: DEFAULT_ANIMATION_DURATION,
                fps: DEFAULT_ANIMATION_FPS,
                enabled_per_animation: HashMap::new(),
                style_per_animation: HashMap::new(),
                duration_per_animation: HashMap::new(),
            },
            defaults: Defaults {
                container_padding: defaults.container_padding,
                workspace_padding: defaults.workspace_padding,
                resize_delta: DEFAULT_RESIZE_DELTA,
                mouse_follows_focus: DEFAULT_MOUSE_FOLLOWS_FOCUS,
            },
            transparency: Transparency {
                enabled: false,
                alpha: 200,
            },
            // The static starts at Widescreen; a load without the key writes Standard
            // (`unwrap_or`), see `SharedDelta::floating_window_aspect_ratio`.
            floating_window_aspect_ratio: AspectRatio::Predefined(
                PredefinedAspectRatio::Widescreen,
            ),
            layout_defaults: HashMap::new(),
            display_index_preferences: HashMap::new(),
        }
    }
}

fn rgb(r: u32, g: u32, b: u32) -> u32 {
    u32::from(Colour::Rgb(Rgb::new(r, g, b)))
}

/// The colours a configuration file or theme names; a missing one keeps the current value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BorderColoursDelta {
    pub single: Option<u32>,
    pub stack: Option<u32>,
    pub monocle: Option<u32>,
    pub floating: Option<u32>,
    pub unfocused: Option<u32>,
    pub unfocused_locked: Option<u32>,
}

/// The animation block of a configuration file (`enabled` is required in the file, the rest is
/// optional).
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationDelta {
    pub enabled: PerAnimationPrefixConfig<bool>,
    pub style: Option<PerAnimationPrefixConfig<AnimationStyle>>,
    pub duration: Option<PerAnimationPrefixConfig<u64>>,
    pub fps: Option<u64>,
}

/// What one configuration load says about the [`Shared`] settings. `None` means "the key is not in
/// the file", and [`apply`](Self::apply) treats each `None` exactly as
/// `StaticConfig::apply_globals` does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SharedDelta {
    pub border: Option<bool>,
    pub border_width: Option<i32>,
    pub border_offset: Option<i32>,
    pub border_radius: Option<i32>,
    pub border_colours: Option<BorderColoursDelta>,
    pub animation: Option<AnimationDelta>,
    pub default_container_padding: Option<i32>,
    pub default_workspace_padding: Option<i32>,
    pub transparency: Option<bool>,
    pub transparency_alpha: Option<u8>,
    pub floating_window_aspect_ratio: Option<AspectRatio>,
    pub layout_defaults: Option<HashMap<DefaultLayout, LayoutDefaultEntry>>,
    pub display_index_preferences: Option<HashMap<usize, String>>,
}

impl SharedDelta {
    /// Merge this load into `shared`.
    ///
    /// * Keeps the old value when the key is absent: the border (enabled, width, offset, radius,
    ///   each colour), both paddings, transparency and its alpha, the display index preferences,
    ///   and the animation block as a whole.
    /// * Resets when the key is absent: `floating_window_aspect_ratio` (to Standard, not to the
    ///   initial Widescreen), `layout_defaults` (cleared), and, inside a present animation block,
    ///   `fps` (to 60).
    /// * A global animation value clears that setting's per-prefix map; a per-prefix map replaces
    ///   the whole map and leaves the global value alone.
    pub fn apply(&self, shared: &mut Shared) {
        shared.floating_window_aspect_ratio = self
            .floating_window_aspect_ratio
            .unwrap_or(AspectRatio::Predefined(PredefinedAspectRatio::Standard));

        if let Some(preferences) = &self.display_index_preferences {
            shared.display_index_preferences.clone_from(preferences);
        }

        if let Some(animation) = &self.animation {
            let target = &mut shared.animation;

            match &animation.enabled {
                PerAnimationPrefixConfig::Prefix(enabled) => {
                    target.enabled_per_animation.clone_from(enabled);
                }
                PerAnimationPrefixConfig::Global(enabled) => {
                    target.enabled = *enabled;
                    target.enabled_per_animation.clear();
                }
            }

            match &animation.style {
                Some(PerAnimationPrefixConfig::Prefix(style)) => {
                    target.style_per_animation.clone_from(style);
                }
                Some(PerAnimationPrefixConfig::Global(style)) => {
                    target.style = *style;
                    target.style_per_animation.clear();
                }
                None => {}
            }

            match &animation.duration {
                Some(PerAnimationPrefixConfig::Prefix(duration)) => {
                    target.duration_per_animation.clone_from(duration);
                }
                Some(PerAnimationPrefixConfig::Global(duration)) => {
                    target.duration = *duration;
                    target.duration_per_animation.clear();
                }
                None => {}
            }

            target.fps = animation.fps.unwrap_or(DEFAULT_ANIMATION_FPS);
        }

        if let Some(container) = self.default_container_padding {
            shared.defaults.container_padding = container;
        }

        if let Some(workspace) = self.default_workspace_padding {
            shared.defaults.workspace_padding = workspace;
        }

        match &self.layout_defaults {
            Some(defaults) => shared.layout_defaults.clone_from(defaults),
            None => shared.layout_defaults.clear(),
        }

        if let Some(width) = self.border_width {
            shared.border.width = width;
        }

        if let Some(offset) = self.border_offset {
            shared.border.offset = offset;
        }

        if let Some(radius) = self.border_radius {
            shared.border.radius = radius;
        }

        if let Some(enabled) = self.border {
            shared.border.enabled = enabled;
        }

        if let Some(colours) = &self.border_colours {
            let target = &mut shared.border.colours;
            if let Some(colour) = colours.single {
                target.single = colour;
            }
            if let Some(colour) = colours.stack {
                target.stack = colour;
            }
            if let Some(colour) = colours.monocle {
                target.monocle = colour;
            }
            if let Some(colour) = colours.floating {
                target.floating = colour;
            }
            if let Some(colour) = colours.unfocused {
                target.unfocused = colour;
            }
            if let Some(colour) = colours.unfocused_locked {
                target.unfocused_locked = colour;
            }
        }

        if let Some(enabled) = self.transparency {
            shared.transparency.enabled = enabled;
        }

        if let Some(alpha) = self.transparency_alpha {
            shared.transparency.alpha = alpha;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> Shared {
        Shared::defaults_for(PlatformDefaults::KDE)
    }

    #[test]
    fn the_platform_defaults_are_the_ones_the_statics_start_with() {
        let windows = Shared::defaults_for(PlatformDefaults::WINDOWS);
        assert_eq!(
            (windows.border.width, windows.border.offset),
            (8, -1),
            "komorebi/src/border_manager/mod.rs"
        );
        assert_eq!(windows.defaults.container_padding, 10);
        assert_eq!(windows.defaults.workspace_padding, 10);

        for platform in [PlatformDefaults::KDE, PlatformDefaults::MACOS] {
            let other = Shared::defaults_for(platform);
            assert_eq!((other.border.width, other.border.offset), (10, 0));
            assert_eq!(other.defaults.container_padding, 5);
            assert_eq!(other.defaults.workspace_padding, 5);
        }
    }

    #[test]
    fn the_default_colours_are_the_theme_conversion_of_the_rgb_values() {
        // Rgb packs as r | g << 8 | b << 16 (komorebi-themes), e.g. FOCUSED = (66, 165, 245)
        let colours = shared().border.colours;
        assert_eq!(colours.single, 66 | (165 << 8) | (245 << 16));
        assert_eq!(colours.unfocused_locked, 158 | (8 << 8) | (8 << 16));
    }

    #[test]
    fn an_empty_load_keeps_everything_but_the_two_reset_keys() {
        let mut shared = shared();
        shared.border.width = 33;
        shared.defaults.container_padding = 7;
        shared.transparency.alpha = 12;
        shared.animation.fps = 144;
        shared
            .display_index_preferences
            .insert(0, String::from("serial"));
        shared.layout_defaults.insert(
            DefaultLayout::BSP,
            LayoutDefaultEntry {
                layout_options: None,
                layout_options_rules: None,
            },
        );
        let before = shared.clone();

        SharedDelta::default().apply(&mut shared);

        // kept
        assert_eq!(shared.border, before.border);
        assert_eq!(shared.defaults, before.defaults);
        assert_eq!(shared.transparency, before.transparency);
        assert_eq!(shared.animation, before.animation);
        assert_eq!(
            shared.display_index_preferences,
            before.display_index_preferences
        );
        // reset
        assert!(shared.layout_defaults.is_empty());
        assert_eq!(
            shared.floating_window_aspect_ratio,
            AspectRatio::Predefined(PredefinedAspectRatio::Standard),
            "absent key writes Standard, not the initial Widescreen"
        );
    }

    #[test]
    fn present_keys_replace_and_absent_colours_stay() {
        let mut shared = shared();
        let before = shared.border.colours;

        SharedDelta {
            border: Some(false),
            border_width: Some(3),
            border_offset: Some(-2),
            border_radius: Some(0),
            border_colours: Some(BorderColoursDelta {
                single: Some(1),
                unfocused_locked: Some(2),
                ..BorderColoursDelta::default()
            }),
            default_workspace_padding: Some(1),
            transparency: Some(true),
            transparency_alpha: Some(9),
            ..SharedDelta::default()
        }
        .apply(&mut shared);

        assert!(!shared.border.enabled);
        assert_eq!(
            (
                shared.border.width,
                shared.border.offset,
                shared.border.radius
            ),
            (3, -2, 0)
        );
        assert_eq!(shared.border.colours.single, 1);
        assert_eq!(shared.border.colours.unfocused_locked, 2);
        assert_eq!(shared.border.colours.stack, before.stack);
        assert_eq!(shared.defaults.workspace_padding, 1);
        assert_eq!(shared.defaults.container_padding, 5, "absent: kept");
        assert_eq!(
            shared.transparency,
            Transparency {
                enabled: true,
                alpha: 9
            }
        );
    }

    fn block(
        enabled: PerAnimationPrefixConfig<bool>,
        style: Option<PerAnimationPrefixConfig<AnimationStyle>>,
        duration: Option<PerAnimationPrefixConfig<u64>>,
        fps: Option<u64>,
    ) -> SharedDelta {
        SharedDelta {
            animation: Some(AnimationDelta {
                enabled,
                style,
                duration,
                fps,
            }),
            ..SharedDelta::default()
        }
    }

    #[test]
    fn a_global_animation_value_clears_its_per_prefix_map_and_a_map_keeps_the_global() {
        let mut shared = shared();
        shared
            .animation
            .enabled_per_animation
            .insert(AnimationPrefix::Movement, true);
        shared
            .animation
            .duration_per_animation
            .insert(AnimationPrefix::Workspace, 50);

        // global enabled clears the enabled map; style absent keeps; duration map replaces the
        // map and leaves the global duration alone
        block(
            PerAnimationPrefixConfig::Global(true),
            None,
            Some(PerAnimationPrefixConfig::Prefix(HashMap::from([(
                AnimationPrefix::Movement,
                10,
            )]))),
            Some(30),
        )
        .apply(&mut shared);

        assert!(shared.animation.enabled);
        assert!(shared.animation.enabled_per_animation.is_empty());
        assert_eq!(shared.animation.style, DEFAULT_ANIMATION_STYLE);
        assert_eq!(shared.animation.duration, DEFAULT_ANIMATION_DURATION);
        assert_eq!(
            shared.animation.duration_per_animation,
            HashMap::from([(AnimationPrefix::Movement, 10)])
        );
        assert_eq!(shared.animation.fps, 30);
    }

    #[test]
    fn an_animation_block_without_fps_resets_fps_and_no_block_keeps_it() {
        let mut shared = shared();
        shared.animation.fps = 144;

        SharedDelta::default().apply(&mut shared);
        assert_eq!(shared.animation.fps, 144);

        block(PerAnimationPrefixConfig::Global(false), None, None, None).apply(&mut shared);
        assert_eq!(shared.animation.fps, DEFAULT_ANIMATION_FPS);
    }

    #[test]
    fn motion_for_reads_one_consistent_configuration() {
        let mut shared = shared();
        shared.animation.enabled = true;
        shared.animation.duration = 250;
        shared.animation.style = AnimationStyle::Linear;
        shared
            .animation
            .duration_per_animation
            .insert(AnimationPrefix::Workspace, 400);
        shared
            .animation
            .style_per_animation
            .insert(AnimationPrefix::Workspace, AnimationStyle::EaseInSine);
        shared
            .animation
            .enabled_per_animation
            .insert(AnimationPrefix::Transparency, false);

        assert_eq!(
            shared.animation.motion_for(AnimationPrefix::Movement),
            Some(Motion {
                duration_ms: 250,
                style: AnimationStyle::Linear
            })
        );
        assert_eq!(
            shared.animation.motion_for(AnimationPrefix::Workspace),
            Some(Motion {
                duration_ms: 400,
                style: AnimationStyle::EaseInSine
            })
        );
        assert_eq!(
            shared.animation.motion_for(AnimationPrefix::Transparency),
            None,
            "a per-prefix false wins over a global true"
        );
    }

    #[test]
    fn layout_defaults_are_replaced_when_given() {
        let mut shared = shared();
        let entry = LayoutDefaultEntry {
            layout_options: None,
            layout_options_rules: None,
        };

        SharedDelta {
            layout_defaults: Some(HashMap::from([(DefaultLayout::BSP, entry.clone())])),
            ..SharedDelta::default()
        }
        .apply(&mut shared);
        assert_eq!(
            shared.layout_defaults.get(&DefaultLayout::BSP),
            Some(&entry)
        );

        SharedDelta {
            layout_defaults: Some(HashMap::new()),
            ..SharedDelta::default()
        }
        .apply(&mut shared);
        assert!(shared.layout_defaults.is_empty());
    }

    #[test]
    fn display_index_preferences_are_replaced_not_merged() {
        let mut shared = shared();
        shared
            .display_index_preferences
            .insert(0, String::from("a"));
        shared
            .display_index_preferences
            .insert(1, String::from("b"));

        SharedDelta {
            display_index_preferences: Some(HashMap::from([(2, String::from("c"))])),
            ..SharedDelta::default()
        }
        .apply(&mut shared);

        assert_eq!(
            shared.display_index_preferences,
            HashMap::from([(2, String::from("c"))])
        );
    }
}
