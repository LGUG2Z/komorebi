//! Turns a theme into border colors: a named theme or a palette of one's own, or the palette made
//! from a picture.
//!
//! [`send_notification`] puts a theme's six border colors into the statics of
//! [`border_manager`], which hands them to the border effect. The palette of a picture takes
//! seconds to make, so [`wallpaper_palette`] keeps each one it makes in komorebi's cache directory
//! ([`palette_file`]).

use crate::border_manager;
use crossbeam_utils::atomic::AtomicCell;
use komorebi_themes::Base16ColourPalette;
use komorebi_themes::Base16Wrapper;
use komorebi_themes::KomorebiTheme;
use komorebi_themes::KomorebiThemeBase16;
use komorebi_themes::KomorebiThemeCatppuccin;
use komorebi_themes::KomorebiThemeCustom;
use komorebi_themes::ThemeVariant;
use komorebi_themes::colour::Colour;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// Returns the directory the palettes made from wallpapers are kept in: `komorebi` in the user's
/// cache directory (`$XDG_CACHE_HOME`, else `~/.cache`), else in the log directory.
///
/// A palette takes seconds to make, so it should outlive a login, and the runtime directory, where
/// komorebi's socket is, does not.
fn palette_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| crate::LOG_DIR.clone())
        .join("komorebi")
}

/// Returns the file the palette of the picture at `path`, in this variant, is kept in.
///
/// The name alone is not enough to know a wallpaper by: two folders can each have a
/// `wallpaper.png`, and one wallpaper can be painted over. The file is named after the picture, a
/// hash of its path, size and time of last change, and the variant, so the palette is made again
/// when the path, the size or the time of the last change differ.
///
/// ```rust
/// use komorebi::theme_manager::palette_file;
/// use komorebi_themes::ThemeVariant;
/// use std::path::Path;
///
/// let file = palette_file(Path::new("/nonexistent/dunes.png"), ThemeVariant::Dark);
/// let name = file.file_name().unwrap().to_string_lossy().into_owned();
/// assert!(name.starts_with("dunes-") && name.ends_with(".base16.dark.json"), "{name}");
///
/// // a picture of the same name in another folder has a palette of its own
/// let elsewhere = palette_file(Path::new("/elsewhere/dunes.png"), ThemeVariant::Dark);
/// assert_ne!(file, elsewhere);
/// ```
pub fn palette_file(path: &Path, variant: ThemeVariant) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(&mut hasher);
        meta.modified().ok().hash(&mut hasher);
    }

    palette_dir().join(format!(
        "{}-{:016x}.base16.{variant}.json",
        path.file_stem().unwrap_or_default().to_string_lossy(),
        hasher.finish()
    ))
}

/// Returns the Base16 palette of a picture, in this variant: from the cache if it was made before,
/// else made now (which can take a few seconds for a large picture) and kept.
///
/// Returns `None`, and logs why, when no palette can be made from the picture: it cannot be read,
/// or it has fewer than the eight colors a palette needs. A palette that cannot be kept is made
/// again the next time.
pub fn wallpaper_palette(path: &Path, variant: ThemeVariant) -> Option<Base16ColourPalette> {
    let cached = palette_file(path, variant);

    if let Ok(text) = std::fs::read_to_string(&cached)
        && let Ok(palette) = serde_json::from_str::<Base16ColourPalette>(&text)
    {
        tracing::info!(
            "colour palette for wallpaper {} found in cache",
            cached.display()
        );
        return Some(palette);
    }

    let palette = match komorebi_themes::generate_base16_palette(path, variant) {
        Ok(palette) => palette,
        Err(error) => {
            tracing::error!(
                "could not make a colour palette from {}: {error}",
                path.display()
            );
            return None;
        }
    };

    // not being able to keep it only means making it again next time
    let kept = std::fs::create_dir_all(palette_dir()).and_then(|()| {
        std::fs::write(
            &cached,
            serde_json::to_string_pretty(&palette).map_err(std::io::Error::other)?,
        )
    });
    match kept {
        Ok(()) => tracing::info!("colour palette for wallpaper {} cached", cached.display()),
        Err(error) => tracing::warn!("could not keep {}: {error}", cached.display()),
    }

    Some(palette)
}

pub static CURRENT_THEME: AtomicCell<Option<KomorebiTheme>> = AtomicCell::new(None);

/// Makes `theme` the theme of the borders: puts its colors into the statics of [`border_manager`]
/// and the theme into [`CURRENT_THEME`], and asks the compositor for the new look, on the
/// caller's thread.
pub fn send_notification(theme: KomorebiTheme) {
    apply(&theme);
    CURRENT_THEME.store(Some(theme));
    border_manager::send_notification();
}

/// Puts the six border colors of a theme into the statics of [`border_manager`]: the color the
/// theme names for each kind of border, else the theme's default for that kind.
fn apply(theme: &KomorebiTheme) {
    let (
        single_border,
        stack_border,
        monocle_border,
        floating_border,
        unfocused_border,
        unfocused_locked_border,
    ) = match theme {
        KomorebiTheme::Catppuccin(KomorebiThemeCatppuccin {
            name,
            single_border,
            stack_border,
            monocle_border,
            floating_border,
            unfocused_border,
            unfocused_locked_border,
            ..
        }) => {
            let single_border = single_border
                .unwrap_or(komorebi_themes::CatppuccinValue::Blue)
                .color32(name.as_theme());

            let stack_border = stack_border
                .unwrap_or(komorebi_themes::CatppuccinValue::Green)
                .color32(name.as_theme());

            let monocle_border = monocle_border
                .unwrap_or(komorebi_themes::CatppuccinValue::Pink)
                .color32(name.as_theme());

            let floating_border = floating_border
                .unwrap_or(komorebi_themes::CatppuccinValue::Yellow)
                .color32(name.as_theme());

            let unfocused_border = unfocused_border
                .unwrap_or(komorebi_themes::CatppuccinValue::Base)
                .color32(name.as_theme());

            let unfocused_locked_border = unfocused_locked_border
                .unwrap_or(komorebi_themes::CatppuccinValue::Red)
                .color32(name.as_theme());

            (
                single_border,
                stack_border,
                monocle_border,
                floating_border,
                unfocused_border,
                unfocused_locked_border,
            )
        }
        KomorebiTheme::Base16(KomorebiThemeBase16 {
            name,
            single_border,
            stack_border,
            monocle_border,
            floating_border,
            unfocused_border,
            unfocused_locked_border,
            ..
        }) => {
            let single_border = single_border
                .unwrap_or(komorebi_themes::Base16Value::Base0D)
                .color32(Base16Wrapper::Base16(*name));

            let stack_border = stack_border
                .unwrap_or(komorebi_themes::Base16Value::Base0B)
                .color32(Base16Wrapper::Base16(*name));

            let monocle_border = monocle_border
                .unwrap_or(komorebi_themes::Base16Value::Base0F)
                .color32(Base16Wrapper::Base16(*name));

            let unfocused_border = unfocused_border
                .unwrap_or(komorebi_themes::Base16Value::Base01)
                .color32(Base16Wrapper::Base16(*name));

            let unfocused_locked_border = unfocused_locked_border
                .unwrap_or(komorebi_themes::Base16Value::Base08)
                .color32(Base16Wrapper::Base16(*name));

            let floating_border = floating_border
                .unwrap_or(komorebi_themes::Base16Value::Base09)
                .color32(Base16Wrapper::Base16(*name));

            (
                single_border,
                stack_border,
                monocle_border,
                floating_border,
                unfocused_border,
                unfocused_locked_border,
            )
        }
        KomorebiTheme::Custom(KomorebiThemeCustom {
            colours,
            single_border,
            stack_border,
            monocle_border,
            floating_border,
            unfocused_border,
            unfocused_locked_border,
            ..
        }) => {
            let single_border = single_border
                .unwrap_or(komorebi_themes::Base16Value::Base0D)
                .color32(Base16Wrapper::Custom(colours.clone()));

            let stack_border = stack_border
                .unwrap_or(komorebi_themes::Base16Value::Base0B)
                .color32(Base16Wrapper::Custom(colours.clone()));

            let monocle_border = monocle_border
                .unwrap_or(komorebi_themes::Base16Value::Base0F)
                .color32(Base16Wrapper::Custom(colours.clone()));

            let unfocused_border = unfocused_border
                .unwrap_or(komorebi_themes::Base16Value::Base01)
                .color32(Base16Wrapper::Custom(colours.clone()));

            let unfocused_locked_border = unfocused_locked_border
                .unwrap_or(komorebi_themes::Base16Value::Base08)
                .color32(Base16Wrapper::Custom(colours.clone()));

            let floating_border = floating_border
                .unwrap_or(komorebi_themes::Base16Value::Base09)
                .color32(Base16Wrapper::Custom(colours.clone()));

            (
                single_border,
                stack_border,
                monocle_border,
                floating_border,
                unfocused_border,
                unfocused_locked_border,
            )
        }
    };

    border_manager::FOCUSED.store(u32::from(Colour::from(single_border)), Ordering::SeqCst);
    border_manager::MONOCLE.store(u32::from(Colour::from(monocle_border)), Ordering::SeqCst);
    border_manager::STACK.store(u32::from(Colour::from(stack_border)), Ordering::SeqCst);
    border_manager::FLOATING.store(u32::from(Colour::from(floating_border)), Ordering::SeqCst);
    border_manager::UNFOCUSED.store(u32::from(Colour::from(unfocused_border)), Ordering::SeqCst);
    border_manager::UNFOCUSED_LOCKED.store(
        u32::from(Colour::from(unfocused_locked_border)),
        Ordering::SeqCst,
    );
}
