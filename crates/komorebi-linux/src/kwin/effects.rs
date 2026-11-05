//! The two KWin effects that draw komorebi's borders and animate its windows.
//!
//! komorebi draws no border and animates nothing itself. Two compositor effects do, and this
//! module configures them:
//!
//! - **KDE Rounded Corners** ([`ROUNDED_CORNERS`]) draws the border: its outer outline, outside the
//!   window, in the gap komorebi leaves between tiles. [`border_settings`] has the settings.
//! - **Geometry Change** ([`GEOMETRY_CHANGE`]) animates a window going to its place.
//!   [`animation_settings`] has the settings.
//!
//! [`EffectsState::apply`] writes an effect's settings into `kwinrc` through [`Kwinrc`], loads the
//! effect if it is not loaded, and tells it to read its settings again. It always does the last
//! step: KWin keeps the copy of `kwinrc` it read when it started, and an effect reads its settings
//! from that copy until it is told to reconfigure, even one loaded a moment ago.
//!
//! Neither effect comes with KWin, and KWin discovers effects only when it starts: an effect
//! installed later cannot be loaded until the next login. An effect that is not installed is
//! reported once in the log, and komorebi carries on without it.
//!
//! komorebi's builds of the two effects (`nix/kde-rounded-corners.nix`,
//! `nix/kwin-effect-geometry-change.nix`) add what the stock ones lack: a border color for each
//! window, set over D-Bus with [`EffectsState::set_window_colours`], and a choice of easing curve
//! (the `Curve` setting).
//!
//! # What is guaranteed, and by what
//!
//! | Rule | Enforced by | Proof |
//! |---|---|---|
//! | No outline is drawn inside the window, over its title bar | [`border_settings`] sets every inner outline to 0 | `kwin::effects::tests::the_border_is_the_outer_outline_and_nothing_is_drawn_inside_the_window` |
//! | The border is never thicker than the effect can draw | [`border_settings`] clamps the width to `0..=20` | `kwin::effects::tests::a_border_thicker_than_the_effect_can_draw_is_the_thickest_it_can` |
//! | The same look asked for twice sends nothing to KWin | [`EffectsState::apply`] compares with the last look applied | no test |
//! | An effect komorebi loaded is unloaded when komorebi exits | [`EffectsState::finish`], called by the KWin backend when it shuts down | the `vm-look` VM suite |
//! | An effect the user had loaded stays loaded, with the user's settings | [`EffectsState::finish`] unloads only the effects komorebi loaded, and puts the settings back | no test |
//! | The user's settings of an effect are put back when komorebi stops using it or exits, even after a crash | [`Kwinrc`], which keeps the first value of every key it sets in a backup file | `kwin::kwinrc::tests::a_backup_left_by_a_crash_keeps_the_values_from_before_the_first_run` |
//!
//! KDE Rounded Corners 0.10.0 draws the border about 24 pixels too low on KWin 6.8, and
//! `nix/kde-rounded-corners.nix` builds 0.11.0, which does not. At time of writing (2026-10-28)
//! nixpkgs has 0.11.0 as well.

#![deny(missing_docs)]

use super::kwinrc::Kwinrc;
use crate::backend::AnimationLook;
use crate::backend::BorderLook;
use crate::backend::Look;
use color_eyre::eyre;
use zbus::Connection;
use zbus::proxy::Proxy;

/// The plugin id of KDE Rounded Corners, the effect that draws the border.
pub const ROUNDED_CORNERS: &str = "kwin4_effect_shapecorners";
/// The plugin id of Geometry Change, the effect that animates windows.
pub const GEOMETRY_CHANGE: &str = "kwin4_effect_geometry_change";

/// The `kwinrc` group KDE Rounded Corners reads, as its `options.kcfg` names it.
const ROUNDED_CORNERS_GROUP: &str = "Round-Corners";
/// The `kwinrc` group Geometry Change reads.
const GEOMETRY_CHANGE_GROUP: &str = "Effect-kwin4_effect_geometry_change";

/// The D-Bus object path at which komorebi's build of the border effect takes a color for each
/// window, on KWin's bus name.
const COLOURS_PATH: &str = "/org/komorebi/BorderColours";
/// The D-Bus interface of [`COLOURS_PATH`].
const COLOURS_INTERFACE: &str = "org.komorebi.BorderColours1";

/// The thickest outline, in pixels, that the border effect can draw.
///
/// The effect draws the outer outline only where the window has a shadow (its shader returns early
/// where the texture is transparent), which caps the thickness at 20 pixels. This was read from
/// screenshots of a KWin 6.8.1 session, at time of writing (2026-10-27).
const MAX_OUTLINE: i32 = 20;

/// The state komorebi keeps of the two effects: the look it last applied, and what it has to undo
/// when it exits.
///
/// The KWin backend keeps one for the life of the connection to KWin. The default is the state of
/// a komorebi that has told the effects nothing.
#[derive(Default)]
pub struct EffectsState {
    /// The look last applied, so that asking for it again does nothing.
    last: Option<Look>,
    /// Whether borders were being drawn after [`EffectsState::last`] was applied.
    drawn: bool,
    /// The effects komorebi loaded, which it unloads when it is done.
    loaded_by_us: Vec<&'static str>,
    /// The effects komorebi found missing, so that each is reported once.
    missing: Vec<&'static str>,
    /// The border color of each window that the border effect was last given.
    colours: Option<Vec<(String, u32)>>,
    /// Whether the border effect takes a color for each window, once known: only komorebi's build
    /// of it does.
    colours_supported: Option<bool>,
}

/// Returns the settings of KDE Rounded Corners, as `kwinrc` keys and values, for this border.
///
/// Every outline is set, not only the one komorebi uses: the effect's defaults draw a one pixel
/// black outline *inside* the window, over its title bar and contents, which is what an
/// unconfigured install looks like. The border is the outer outline, outside the window, in the gap
/// komorebi leaves between tiles. The width is clamped to `0..=20`, the thickest the effect can
/// draw, and the colors are written as `r,g,b`.
///
/// Covered by `kwin::effects::tests::the_border_is_the_outer_outline_and_nothing_is_drawn_inside_the_window`
/// and `kwin::effects::tests::a_border_thicker_than_the_effect_can_draw_is_the_thickest_it_can`.
pub fn border_settings(border: &BorderLook) -> Vec<(&'static str, String)> {
    let width = border.width.clamp(0, MAX_OUTLINE).to_string();
    let colour = |(r, g, b): (u8, u8, u8)| format!("{r},{g},{b}");
    let off = || "0".to_string();
    let t = || "true".to_string();
    let f = || "false".to_string();

    vec![
        ("Size", border.radius.to_string()),
        ("InactiveCornerRadius", border.radius.to_string()),
        // nothing inside the window
        ("OutlineThickness", off()),
        ("InactiveOutlineThickness", off()),
        ("SecondOutlineThickness", off()),
        ("InactiveSecondOutlineThickness", off()),
        // the border, outside it
        ("OuterOutlineThickness", width.clone()),
        ("InactiveOuterOutlineThickness", width),
        ("OuterOutlineColor", colour(border.focused)),
        ("InactiveOuterOutlineColor", colour(border.unfocused)),
        ("ActiveOuterOutlineAlpha", "255".to_string()),
        ("InactiveOuterOutlineAlpha", "255".to_string()),
        ("ActiveOuterOutlineUseCustom", t()),
        ("InactiveOuterOutlineUseCustom", t()),
        ("ActiveOuterOutlineUsePalette", f()),
        ("InactiveOuterOutlineUsePalette", f()),
        ("OuterOutlineIsGradient", f()),
        ("InactiveOuterOutlineIsGradient", f()),
    ]
}

/// Returns the settings of Geometry Change, as `kwinrc` keys and values, for this animation.
///
/// Cross-fading is turned off. Cross-fading the old contents into the new while a window resizes is
/// the effect's default and costs a redraw of both; a window going to its place is a move and a
/// resize, not a change of what it shows.
///
/// `Curve` is not a setting of the stock effect, which always eases out exponentially. komorebi's
/// build of it (`nix/kwin-effect-geometry-change.nix`) reads it, and the stock effect ignores the
/// key.
///
/// Covered by `kwin::effects::tests::animation_is_a_duration_without_cross_fading`.
pub fn animation_settings(animation: &AnimationLook) -> Vec<(&'static str, String)> {
    vec![
        ("Duration", animation.duration_ms.to_string()),
        ("CrossFade", "false".to_string()),
        ("Curve", animation.curve.to_string()),
    ]
}

/// Returns a proxy for KWin's `/Effects` object, which loads, unloads and reconfigures effects.
async fn effects(connection: &Connection) -> eyre::Result<Proxy<'static>> {
    Ok(Proxy::new(
        connection,
        "org.kde.KWin",
        "/Effects",
        "org.kde.kwin.Effects",
    )
    .await?)
}

/// Returns true if and only if KWin has the effect installed (it found it when it started). A
/// failed call counts as not installed.
async fn supported(proxy: &Proxy<'_>, name: &str) -> bool {
    proxy
        .call::<_, _, bool>("isEffectSupported", &(name,))
        .await
        .unwrap_or(false)
}

/// Returns true if and only if the effect is loaded. A failed call counts as not loaded.
async fn loaded(proxy: &Proxy<'_>, name: &str) -> bool {
    proxy
        .call::<_, _, bool>("isEffectLoaded", &(name,))
        .await
        .unwrap_or(false)
}

impl EffectsState {
    /// Brings the effects in line with `look`, and returns true if and only if borders are being
    /// drawn afterwards.
    ///
    /// A part of the look that is `None` stops the use of its effect (see
    /// [`EffectsState::disable`]); a part that is set writes the effect's settings and has it
    /// running (see [`EffectsState::enable`]). The same look as the last one does nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if and only if there is no proxy for KWin's `/Effects` object. Everything
    /// that goes wrong with one effect (not installed, settings not written, not loaded) is logged,
    /// and the function still returns whether borders are drawn.
    pub async fn apply(
        &mut self,
        connection: &Connection,
        kwinrc: &Kwinrc,
        look: &Look,
    ) -> eyre::Result<bool> {
        if self.last.as_ref() == Some(look) {
            return Ok(self.drawn);
        }

        let proxy = effects(connection).await?;

        self.drawn = match &look.border {
            Some(border) => {
                self.enable(
                    &proxy,
                    kwinrc,
                    ROUNDED_CORNERS,
                    ROUNDED_CORNERS_GROUP,
                    &border_settings(border),
                )
                .await
            }
            None => {
                self.disable(&proxy, kwinrc, ROUNDED_CORNERS, ROUNDED_CORNERS_GROUP)
                    .await;
                false
            }
        };

        match &look.animation {
            Some(animation) => {
                self.enable(
                    &proxy,
                    kwinrc,
                    GEOMETRY_CHANGE,
                    GEOMETRY_CHANGE_GROUP,
                    &animation_settings(animation),
                )
                .await;
            }
            None => {
                self.disable(&proxy, kwinrc, GEOMETRY_CHANGE, GEOMETRY_CHANGE_GROUP)
                    .await;
            }
        }

        self.last = Some(look.clone());
        Ok(self.drawn)
    }

    /// Tells the border effect the border color of each window, given as KWin's UUID of the window
    /// and `0xRRGGBB`, and returns true if and only if the effect has them.
    ///
    /// It returns false when the effect cannot be told: the borders are not being drawn, there is
    /// no proxy for the effect's object, or the effect is the stock one, which has a color for the
    /// active window and one for the others, from its settings, and nothing else. The first call
    /// that finds the stock effect logs it once; later calls return false without asking. The same
    /// colors as the last call send nothing.
    pub async fn set_window_colours(
        &mut self,
        connection: &Connection,
        colours: &[(String, u32)],
    ) -> bool {
        if !self.drawn {
            // unloaded, and the next time it is loaded it knows nothing
            self.colours = None;
            return false;
        }

        if self.colours_supported == Some(false) {
            return false;
        }

        if self.colours.as_deref() == Some(colours) {
            return true;
        }

        let proxy =
            match Proxy::new(connection, "org.kde.KWin", COLOURS_PATH, COLOURS_INTERFACE).await {
                Ok(proxy) => proxy,
                Err(error) => {
                    tracing::debug!("no proxy for the border colours: {error}");
                    return false;
                }
            };

        let (names, rgbs): (Vec<String>, Vec<u32>) = colours.iter().cloned().unzip();
        match proxy.call_method("SetColours", &(names, rgbs)).await {
            Ok(_) => {
                self.colours_supported = Some(true);
                self.colours = Some(colours.to_vec());
                true
            }
            Err(error) => {
                // a stock build of the effect has no such object; this is the answer, not a fault
                self.colours_supported = Some(false);
                tracing::info!(
                    "the border effect does not take a colour for each window ({error}): borders \
                     are the focused and the unfocused colour only. komorebi's build of KDE \
                     Rounded Corners (nix/kde-rounded-corners.nix) does."
                );
                false
            }
        }
    }

    /// Gives the effect these settings and has it running, and returns true if and only if it is
    /// running with them.
    ///
    /// Returns false, and logs why, when the effect is not installed (reported once), when its
    /// settings cannot be written, or when KWin does not load it.
    async fn enable(
        &mut self,
        proxy: &Proxy<'_>,
        kwinrc: &Kwinrc,
        effect: &'static str,
        group: &str,
        settings: &[(&'static str, String)],
    ) -> bool {
        if !supported(proxy, effect).await {
            if !self.missing.contains(&effect) {
                self.missing.push(effect);
                tracing::info!(
                    "the {effect} effect is not installed (KWin finds effects when it starts), \
                     so it is not used"
                );
            }
            return false;
        }

        if let Err(error) = kwinrc.set(group, settings) {
            tracing::warn!("could not write the settings of {effect}: {error}");
            return false;
        }

        if !loaded(proxy, effect).await {
            match proxy.call::<_, _, bool>("loadEffect", &(effect,)).await {
                Ok(true) => {
                    self.loaded_by_us.push(effect);
                    tracing::info!("loaded the {effect} effect");
                }
                other => {
                    tracing::warn!("KWin would not load the {effect} effect: {other:?}");
                    return false;
                }
            }
        }

        // KWin's own copy of kwinrc is as it was when it started, and what the effect reads when
        // it loads comes from there: the settings only reach it by being told to read them again,
        // even for an effect loaded a moment ago
        let _ = proxy.call_method("reconfigureEffect", &(effect,)).await;

        true
    }

    /// Stops using the effect: unloads it if komorebi loaded it, and puts the user's settings for
    /// it back, telling it to read them again if it is still loaded.
    async fn disable(
        &mut self,
        proxy: &Proxy<'_>,
        kwinrc: &Kwinrc,
        effect: &'static str,
        group: &str,
    ) {
        if let Some(at) = self.loaded_by_us.iter().position(|e| *e == effect) {
            self.loaded_by_us.remove(at);
            let _ = proxy.call_method("unloadEffect", &(effect,)).await;
        }

        if kwinrc.restore_group(group).unwrap_or(false) && loaded(proxy, effect).await {
            let _ = proxy.call_method("reconfigureEffect", &(effect,)).await;
        }
    }

    /// Undoes what komorebi did to the effects, when it exits: unloads the effects it loaded, and
    /// puts back the user's settings of both.
    ///
    /// An effect the user had loaded keeps running, with the settings it had. Nothing is done when
    /// there is no proxy for KWin's `/Effects` object.
    pub async fn finish(&mut self, connection: &Connection, kwinrc: &Kwinrc) {
        let Ok(proxy) = effects(connection).await else {
            return;
        };

        for effect in std::mem::take(&mut self.loaded_by_us) {
            let _ = proxy.call_method("unloadEffect", &(effect,)).await;
        }

        // an effect the user had running keeps running, with the settings it had
        for (effect, group) in [
            (ROUNDED_CORNERS, ROUNDED_CORNERS_GROUP),
            (GEOMETRY_CHANGE, GEOMETRY_CHANGE_GROUP),
        ] {
            if kwinrc.restore_group(group).unwrap_or(false) && loaded(&proxy, effect).await {
                let _ = proxy.call_method("reconfigureEffect", &(effect,)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(border: &BorderLook) -> std::collections::HashMap<&'static str, String> {
        border_settings(border).into_iter().collect()
    }

    #[test]
    fn the_border_is_the_outer_outline_and_nothing_is_drawn_inside_the_window() {
        let s = settings(&BorderLook {
            width: 10,
            radius: 12,
            focused: (66, 165, 245),
            unfocused: (128, 128, 128),
        });

        assert_eq!(s["OuterOutlineThickness"], "10");
        assert_eq!(s["InactiveOuterOutlineThickness"], "10");
        assert_eq!(s["OuterOutlineColor"], "66,165,245");
        assert_eq!(s["InactiveOuterOutlineColor"], "128,128,128");
        assert_eq!(s["Size"], "12");
        assert_eq!(s["InactiveCornerRadius"], "12");

        // the effect's defaults put a black outline over the title bar: every inner one is off
        for key in [
            "OutlineThickness",
            "InactiveOutlineThickness",
            "SecondOutlineThickness",
            "InactiveSecondOutlineThickness",
        ] {
            assert_eq!(s[key], "0", "{key}");
        }
        assert_eq!(s["ActiveOuterOutlineUseCustom"], "true");
        assert_eq!(s["ActiveOuterOutlineUsePalette"], "false");
    }

    #[test]
    fn a_border_thicker_than_the_effect_can_draw_is_the_thickest_it_can() {
        let s = settings(&BorderLook {
            width: 400,
            radius: 0,
            focused: (0, 0, 0),
            unfocused: (0, 0, 0),
        });
        assert_eq!(s["OuterOutlineThickness"], MAX_OUTLINE.to_string());

        let s = settings(&BorderLook {
            width: -3,
            radius: 0,
            focused: (0, 0, 0),
            unfocused: (0, 0, 0),
        });
        assert_eq!(s["OuterOutlineThickness"], "0");
    }

    #[test]
    fn animation_is_a_duration_without_cross_fading() {
        let s: std::collections::HashMap<_, _> = animation_settings(&AnimationLook {
            duration_ms: 250,
            curve: "InOutSine",
        })
        .into_iter()
        .collect();
        assert_eq!(s["Duration"], "250");
        assert_eq!(s["CrossFade"], "false");
        assert_eq!(s["Curve"], "InOutSine");
    }
}
