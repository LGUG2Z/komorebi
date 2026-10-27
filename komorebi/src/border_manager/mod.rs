//! The borders around the windows, which the compositor draws.
//!
//! komorebi draws no border itself. KWin's KDE Rounded Corners effect draws an outline outside
//! each window, in the gap komorebi leaves between tiles, and this module decides what it looks
//! like:
//!
//! - [`send_notification`] asks the backend for the [`Look`] of the settings in force
//!   ([`current_look`]): the border's width, radius and two colors, one for the focused window and
//!   one for the others, and the animation of windows going to their places. The backend answers
//!   whether the borders are drawn, which is kept in [`BORDER_DRAWN`]: a drawn border takes room
//!   around each window, which the layout leaves.
//! - [`update_colours`] tells the backend the color of each window's border, by what the window
//!   is to the layout ([`window_kinds`]). Only komorebi's build of the effect takes them; the
//!   stock one draws the look's two colors, one for the active window and one for the others.
//!
//! The settings are the statics of this module, which the `komorebic border*` commands set and
//! the `border*` keys of `komorebi.json` ([`crate::static_config::StaticConfig`]) give their
//! first values.

use crate::backend::AnimationLook;
use crate::backend::BorderLook;
use crate::backend::Look;
use crate::backend::backend;
use crate::core::WindowKind;
use crate::core::animation::AnimationStyle;
use crate::window_manager::WindowManager;
use color_eyre::eyre;
use lazy_static::lazy_static;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

/// The thickness of the border, in pixels.
///
/// It is also the room the layout leaves for the border on each side of a window while the borders
/// are drawn ([`BORDER_DRAWN`]). The effect draws at most 20 pixels; the layout leaves the whole
/// width even when it is more.
pub static BORDER_WIDTH: AtomicI32 = AtomicI32::new(10);
/// More room the layout leaves on each side of a window while the borders are drawn, on top of
/// [`BORDER_WIDTH`]. The effect draws nothing in it.
pub static BORDER_OFFSET: AtomicI32 = AtomicI32::new(0);
/// The radius of the window's corners, in pixels.
pub static BORDER_RADIUS: AtomicI32 = AtomicI32::new(10);
/// Whether borders are asked for at all.
pub static BORDER_ENABLED: AtomicBool = AtomicBool::new(true);

lazy_static! {
    /// The color of the focused window's border, as [`pack`] stores it.
    pub static ref FOCUSED: AtomicU32 = AtomicU32::new(pack(66, 165, 245));
    /// The color of the border of every other window, as [`pack`] stores it.
    pub static ref UNFOCUSED: AtomicU32 = AtomicU32::new(pack(128, 128, 128));
    /// The color of the border of a locked container that does not have the focus.
    pub static ref UNFOCUSED_LOCKED: AtomicU32 = AtomicU32::new(pack(158, 8, 8));
    /// The color of the border of a monocled window.
    pub static ref MONOCLE: AtomicU32 = AtomicU32::new(pack(255, 51, 153));
    /// The color of the border of a stack that has the focus.
    pub static ref STACK: AtomicU32 = AtomicU32::new(pack(0, 165, 66));
    /// The color of the border of a floating window that has the focus.
    pub static ref FLOATING: AtomicU32 = AtomicU32::new(pack(245, 245, 165));
}

/// Returns the kind of border the stock border effect gives a window of this kind.
///
/// The stock effect has one color for the active window and one for the others, so `Stack`,
/// `Monocle` and `Floating` are shown as `Single`, and `UnfocusedLocked` as `Unfocused`.
///
/// ```rust
/// use komorebi::border_manager::outline_kind;
/// use komorebi::core::WindowKind;
///
/// assert_eq!(outline_kind(WindowKind::Stack), WindowKind::Single);
/// assert_eq!(outline_kind(WindowKind::UnfocusedLocked), WindowKind::Unfocused);
/// ```
pub fn outline_kind(kind: WindowKind) -> WindowKind {
    match kind {
        WindowKind::Single | WindowKind::Stack | WindowKind::Monocle | WindowKind::Floating => {
            WindowKind::Single
        }
        WindowKind::Unfocused | WindowKind::UnfocusedLocked => WindowKind::Unfocused,
    }
}

/// Returns the color the settings give a kind of window, as the statics store it (red, green and
/// blue in one number).
fn kind_colour(kind: WindowKind) -> u32 {
    match kind {
        WindowKind::Single => FOCUSED.load(Ordering::SeqCst),
        WindowKind::Stack => STACK.load(Ordering::SeqCst),
        WindowKind::Monocle => MONOCLE.load(Ordering::SeqCst),
        WindowKind::Floating => FLOATING.load(Ordering::SeqCst),
        WindowKind::Unfocused => UNFOCUSED.load(Ordering::SeqCst),
        WindowKind::UnfocusedLocked => UNFOCUSED_LOCKED.load(Ordering::SeqCst),
    }
}

/// Returns what the border of each window is to the layout, which decides its color.
///
/// Only the workspace each monitor shows is looked at. On it, the focused window of the focused
/// container on the focused monitor is `Single`, or `Stack` if its container has more than one
/// window; a monocled window is `Monocle` on the focused monitor; the focused window of any other
/// container is `Unfocused`, or `UnfocusedLocked` if its container is locked; a floating window is
/// `Floating` if it has the focus and `Unfocused` if not. A workspace with a monocle lists the
/// monocled window, and its floating windows only when the floating layer is in front.
pub fn window_kinds(wm: &WindowManager) -> Vec<(u32, WindowKind)> {
    let mut out = vec![];
    let focused_monitor_idx = wm.focused_monitor_idx();
    let foreground = backend().foreground_window();

    let floating = |workspace: &crate::workspace::Workspace, out: &mut Vec<(u32, WindowKind)>| {
        for window in workspace.floating_windows() {
            let kind = if foreground == Some(window.id) {
                WindowKind::Floating
            } else {
                WindowKind::Unfocused
            };
            out.push((window.id, kind));
        }
    };

    for (monitor_idx, monitor) in wm.monitors().iter().enumerate() {
        let Some(workspace) = monitor.focused_workspace() else {
            continue;
        };

        if let Some(monocle) = &workspace.monocle_container {
            if let Some(window) = monocle.focused_window() {
                let kind = if monitor_idx == focused_monitor_idx {
                    WindowKind::Monocle
                } else {
                    WindowKind::Unfocused
                };
                out.push((window.id, kind));
            }

            if matches!(workspace.layer, crate::workspace::WorkspaceLayer::Floating) {
                floating(workspace, &mut out);
            }

            continue;
        }

        for (idx, container) in workspace.containers().iter().enumerate() {
            let kind =
                if idx != workspace.focused_container_idx() || monitor_idx != focused_monitor_idx {
                    if container.locked {
                        WindowKind::UnfocusedLocked
                    } else {
                        WindowKind::Unfocused
                    }
                } else if container.windows().len() > 1 {
                    WindowKind::Stack
                } else {
                    WindowKind::Single
                };

            if let Some(window) = container.focused_window() {
                out.push((window.id, kind));
            }
        }

        floating(workspace, &mut out);
    }

    out
}

/// Returns the border color of each window [`window_kinds`] lists, as `0xRRGGBB`.
pub fn window_colours(wm: &WindowManager) -> Vec<(u32, u32)> {
    window_kinds(wm)
        .into_iter()
        .map(|(id, kind)| {
            let (r, g, b) = unpack(kind_colour(kind));
            (
                id,
                (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
            )
        })
        .collect()
}

/// Tells the backend the color of every window's border, or that no window has one when the
/// borders are off.
///
/// Called whenever anything about the windows could have changed, after [`send_notification`]
/// (the compositor has to be drawing the borders first). The backend sends nothing it sent last
/// time, and a failure is logged.
pub fn update_colours(wm: &WindowManager) {
    let colours = if BORDER_ENABLED.load(Ordering::SeqCst) {
        window_colours(wm)
    } else {
        vec![]
    };

    if let Err(error) = backend().set_border_colours(&colours) {
        tracing::warn!("could not set the colour of the borders: {error}");
    }
}

/// Returns a color as the statics store it: red in the low byte, then green, then blue.
///
/// ```rust
/// use komorebi::border_manager::pack;
/// use komorebi::border_manager::unpack;
///
/// assert_eq!(pack(1, 2, 3), 0x030201);
/// assert_eq!(unpack(pack(66, 165, 245)), (66, 165, 245));
/// ```
pub const fn pack(r: u32, g: u32, b: u32) -> u32 {
    r | (g << 8) | (b << 16)
}

/// Returns the red, green and blue of a color as the statics store it. The bits above the third
/// byte are dropped.
pub const fn unpack(colour: u32) -> (u8, u8, u8) {
    (
        (colour & 0xff) as u8,
        ((colour >> 8) & 0xff) as u8,
        ((colour >> 16) & 0xff) as u8,
    )
}

/// Whether the compositor is drawing the borders, as the backend last answered
/// [`send_notification`].
///
/// When it is, they take room around each window ([`BORDER_WIDTH`] and [`BORDER_OFFSET`]), which
/// the layout leaves; when it is not (no effect installed, borders off) they take none.
pub static BORDER_DRAWN: AtomicBool = AtomicBool::new(false);

/// Returns the [`Look`] of these settings.
///
/// The border is there when `enabled`, with its width and radius at least 0; the colors are as the
/// statics store them, red, green and blue in one number. The animation is a duration in
/// milliseconds and a style, when there is one.
///
/// ```rust
/// use komorebi::border_manager::look;
/// use komorebi::border_manager::pack;
/// use komorebi::core::animation::AnimationStyle;
///
/// let blue = pack(66, 165, 245);
/// let grey = pack(128, 128, 128);
///
/// let on = look(
///     true,
///     8,
///     -2,
///     blue,
///     grey,
///     Some((250, AnimationStyle::EaseInOutSine)),
/// );
/// let border = on.border.expect("the borders are on");
/// assert_eq!((border.width, border.radius), (8, 0));
/// assert_eq!(border.focused, (66, 165, 245));
/// assert_eq!(border.unfocused, (128, 128, 128));
/// assert_eq!(on.animation.expect("an animation").curve, "InOutSine");
///
/// assert_eq!(look(false, 8, 10, blue, grey, None).border, None);
/// ```
pub fn look(
    enabled: bool,
    width: i32,
    radius: i32,
    focused: u32,
    unfocused: u32,
    animation: Option<(u64, AnimationStyle)>,
) -> Look {
    Look {
        border: enabled.then(|| BorderLook {
            width: width.max(0),
            radius: radius.max(0),
            focused: unpack(focused),
            unfocused: unpack(unfocused),
        }),
        animation: animation.map(|(duration_ms, style)| AnimationLook {
            duration_ms,
            curve: style.qt_curve(),
        }),
    }
}

/// Returns the [`Look`] of the settings in force: the border statics of this module, and the
/// movement animation of [`crate::animation`].
pub fn current_look() -> Look {
    look(
        BORDER_ENABLED.load(Ordering::SeqCst),
        BORDER_WIDTH.load(Ordering::SeqCst),
        BORDER_RADIUS.load(Ordering::SeqCst),
        FOCUSED.load(Ordering::SeqCst),
        UNFOCUSED.load(Ordering::SeqCst),
        crate::animation::movement_animation()
            .map(|duration| (duration, crate::animation::movement_style())),
    )
}

/// Asks the backend for the [`Look`] of the settings in force, and keeps its answer, whether the
/// borders are drawn, in [`BORDER_DRAWN`].
///
/// Called whenever the window manager's state changes in a way that could change the look, and
/// whenever a setting does. The backend does nothing when the look is the one it applied last, and
/// a failure is logged.
pub fn send_notification() {
    match backend().apply_look(&current_look()) {
        Ok(drawn) => BORDER_DRAWN.store(drawn, Ordering::SeqCst),
        Err(error) => tracing::warn!("could not set how windows look: {error}"),
    }
}

/// Asks the backend for the look again, as [`send_notification`] does. Called when a border
/// setting changed.
///
/// There are no border windows to destroy: the compositor draws the borders. The name is the one
/// its callers share with komorebi on the other platforms.
///
/// # Errors
///
/// Never returns an error: a failure is logged.
pub fn destroy_all_borders() -> eyre::Result<()> {
    send_notification();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stored_colours_become_red_green_and_blue() {
        let blue = pack(66, 165, 245);
        let grey = pack(128, 128, 128);

        let look = look(true, 10, 12, blue, grey, None);
        let border = look.border.unwrap();

        assert_eq!(border.focused, (66, 165, 245));
        assert_eq!(border.unfocused, (128, 128, 128));
        assert_eq!((border.width, border.radius), (10, 12));
        assert_eq!(look.animation, None);
    }

    #[test]
    fn borders_off_and_animation_on_are_independent() {
        let look = look(
            false,
            10,
            12,
            0,
            0,
            Some((250, AnimationStyle::EaseOutCubic)),
        );
        assert_eq!(look.border, None);
        assert_eq!(
            look.animation,
            Some(AnimationLook {
                duration_ms: 250,
                curve: "OutCubic"
            })
        );
    }

    #[test]
    fn every_style_is_a_curve_qt_has() {
        // the names QEasingCurve has: komorebi's styles are those without the "Ease"
        for style in <AnimationStyle as clap::ValueEnum>::value_variants() {
            let name = format!("{style}");
            let expected = name.strip_prefix("Ease").unwrap_or(&name);
            assert_eq!(style.qt_curve(), expected, "{name}");
        }
        assert_eq!(
            AnimationStyle::CubicBezier(0.1, 0.2, 0.3, 0.4).qt_curve(),
            "OutCubic"
        );
    }

    #[test]
    fn every_kind_of_window_has_a_color_of_its_own_and_the_stock_effect_has_two() {
        let colours: Vec<u32> = [
            WindowKind::Single,
            WindowKind::Stack,
            WindowKind::Monocle,
            WindowKind::Floating,
            WindowKind::Unfocused,
            WindowKind::UnfocusedLocked,
        ]
        .into_iter()
        .map(kind_colour)
        .collect();
        let mut distinct = colours.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), colours.len());

        assert_eq!(outline_kind(WindowKind::Floating), WindowKind::Single);
        assert_eq!(outline_kind(WindowKind::Unfocused), WindowKind::Unfocused);
    }

    #[test]
    fn a_negative_width_or_radius_is_none() {
        let look = look(true, -4, -1, 0, 0, None);
        let border = look.border.unwrap();
        assert_eq!((border.width, border.radius), (0, 0));
    }
}
