//! The KWin effect that draws komorebi's borders.
//!
//! komorebi draws no border itself. A compositor effect does, and this module configures it:
//! **KDE Rounded Corners** ([`ROUNDED_CORNERS`]) draws the border, its outer outline, outside the
//! window, in the gap komorebi leaves between tiles. [`border_settings`] has the settings.
//!
//! [`EffectsState::apply`] writes the effect's settings into `kwinrc` through [`Kwinrc`], loads the
//! effect if it is not loaded, and tells it to read its settings again. It always does the last
//! step: KWin keeps the copy of `kwinrc` it read when it started, and an effect reads its settings
//! from that copy until it is told to reconfigure, even one loaded a moment ago.
//!
//! The effect does not come with KWin, and KWin discovers effects only when it starts: an effect
//! installed later cannot be loaded until the next login. An effect that is not installed is
//! reported once in the log, and komorebi carries on without it.
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
//! At time of writing (2026-10-27) the stock KDE Rounded Corners in nixpkgs is 0.10.0, which draws
//! the border about 24 pixels too low on KWin 6.8; `nix/kde-rounded-corners.nix` builds 0.11.0.

#![deny(missing_docs)]

use super::kwinrc::Kwinrc;
use crate::backend::BorderLook;
use crate::backend::Look;
use color_eyre::eyre;
use zbus::Connection;
use zbus::proxy::Proxy;

/// The plugin id of KDE Rounded Corners, the effect that draws the border.
pub const ROUNDED_CORNERS: &str = "kwin4_effect_shapecorners";

/// The `kwinrc` group KDE Rounded Corners reads, as its `options.kcfg` names it.
const ROUNDED_CORNERS_GROUP: &str = "Round-Corners";

/// The thickest outline, in pixels, that the border effect can draw.
///
/// The effect draws the outer outline only where the window has a shadow (its shader returns early
/// where the texture is transparent), which caps the thickness at 20 pixels. This was read from
/// screenshots of a KWin 6.8.1 session, at time of writing (2026-10-27).
const MAX_OUTLINE: i32 = 20;

/// The state komorebi keeps of the effect: the look it last applied, and what it has to undo when
/// it exits.
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
    /// Brings the effect in line with `look`, and returns true if and only if borders are being
    /// drawn afterwards.
    ///
    /// A look with no border stops the use of the effect (see [`EffectsState::disable`]); one with a
    /// border writes the effect's settings and has it running (see [`EffectsState::enable`]). The
    /// same look as the last one does nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if and only if there is no proxy for KWin's `/Effects` object. Everything
    /// that goes wrong with the effect (not installed, settings not written, not loaded) is logged,
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

        self.last = Some(look.clone());
        Ok(self.drawn)
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

    /// Undoes what komorebi did to the effect, when it exits: unloads it if komorebi loaded it, and
    /// puts back the user's settings.
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
        if kwinrc.restore_group(ROUNDED_CORNERS_GROUP).unwrap_or(false)
            && loaded(&proxy, ROUNDED_CORNERS).await
        {
            let _ = proxy
                .call_method("reconfigureEffect", &(ROUNDED_CORNERS,))
                .await;
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
}
