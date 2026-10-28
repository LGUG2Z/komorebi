//! Pixel arithmetic shared by the commands that look at frames: brightness, cropping to a region
//! of a display, and comparing two pictures.
//!
//! Nothing here decodes video, so it can be tested on synthetic frames. The commands that use it
//! are `changes`, `motion`, `framediff` and `blank` ([`super::changes`], [`super::motion`],
//! [`super::framediff`], [`super::blank`]).
//!
//! # Regions
//!
//! A [`Crop`] is a rectangle in *display points*: the same global coordinates as
//! [`ChangeSpan::bbox_points`](super::changes::ChangeSpan::bbox_points) and a window's rect in
//! komorebi's state. It is turned into fractions of a display's frame ([`Crop::fractions`]) and
//! then into pixels of whatever size a frame was decoded at ([`pixel_rect`]), so the same crop
//! works at any decode size. A caller that has a window's rect passes it to [`Crop::from_rect`].
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::analysis::pixels::{Crop, pixel_rect};
//!
//! // the right half of a display whose frame is 1000 x 500 points at (100, 0)
//! let crop = Crop::parse("600,0,500,500")?;
//! let fractions = crop.fractions([100.0, 0.0, 1000.0, 500.0])?;
//! assert_eq!(fractions, [0.5, 0.0, 0.5, 1.0]);
//!
//! // in a frame decoded at 240 x 120 pixels
//! let rect = pixel_rect(fractions, 240, 120);
//! assert_eq!((rect.x, rect.y, rect.width, rect.height), (120, 0, 120, 120));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::envelope;
use color_eyre::eyre;
use schemars::JsonSchema;
use serde::Serialize;

/// A rectangle in global display points: `x`, `y`, `width`, `height`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct Crop {
    /// Left edge, in points.
    pub x: f64,
    /// Top edge, in points.
    pub y: f64,
    /// Width, in points; above zero.
    pub width: f64,
    /// Height, in points; above zero.
    pub height: f64,
}

impl Crop {
    /// Returns the crop for a rect of `[x, y, width, height]`, such as a window's rect.
    #[must_use]
    pub fn from_rect(rect: [f64; 4]) -> Self {
        Self {
            x: rect[0],
            y: rect[1],
            width: rect[2],
            height: rect[3],
        }
    }

    /// Returns the rect as `[x, y, width, height]`.
    #[must_use]
    pub fn rect(&self) -> [f64; 4] {
        [self.x, self.y, self.width, self.height]
    }

    /// Parses `x,y,w,h` (numbers, in points).
    ///
    /// # Errors
    ///
    /// Returns a usage error if the text is not four numbers or the size is not above zero.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::pixels::Crop;
    ///
    /// assert_eq!(Crop::parse("10, 20,300,40")?.rect(), [10.0, 20.0, 300.0, 40.0]);
    /// assert!(Crop::parse("1,2,3").is_err());
    /// assert!(Crop::parse("1,2,0,4").is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn parse(text: &str) -> eyre::Result<Self> {
        let numbers: Vec<f64> = text
            .split(',')
            .map(|part| part.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| {
                envelope::usage(format!("--region wants x,y,w,h in points, not {text:?}"))
            })?;
        let [x, y, width, height] = numbers[..] else {
            return Err(envelope::usage(format!(
                "--region wants four numbers, x,y,w,h, not {text:?}"
            )));
        };
        if !(width > 0.0 && height > 0.0 && x.is_finite() && y.is_finite()) {
            return Err(envelope::usage(
                "--region needs a width and height above zero",
            ));
        }

        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    /// Returns the crop as fractions of a display whose frame is `frame` (`[x, y, width, height]`
    /// in points): `[x, y, width, height]`, each from 0 to 1. The part of the crop outside the
    /// display is cut off.
    ///
    /// # Errors
    ///
    /// Returns a usage error if the frame has no size or the crop does not overlap the display.
    pub fn fractions(&self, frame: [f64; 4]) -> eyre::Result<[f64; 4]> {
        if frame[2] <= 0.0 || frame[3] <= 0.0 {
            return Err(envelope::usage("the display's frame has no size"));
        }

        let left = ((self.x - frame[0]) / frame[2]).clamp(0.0, 1.0);
        let top = ((self.y - frame[1]) / frame[3]).clamp(0.0, 1.0);
        let right = ((self.x + self.width - frame[0]) / frame[2]).clamp(0.0, 1.0);
        let bottom = ((self.y + self.height - frame[1]) / frame[3]).clamp(0.0, 1.0);
        if right <= left || bottom <= top {
            return Err(envelope::usage(format!(
                "the region {},{},{},{} is outside the display ({},{} {}x{} points)",
                self.x, self.y, self.width, self.height, frame[0], frame[1], frame[2], frame[3]
            )));
        }

        Ok([left, top, right - left, bottom - top])
    }
}

/// A rectangle of pixels in a decoded frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width, at least 1.
    pub width: u32,
    /// Height, at least 1.
    pub height: u32,
}

/// Converts fractions of a frame to the pixels of a `width` x `height` frame. The rect is at
/// least one pixel and never leaves the frame.
#[must_use]
pub fn pixel_rect(fractions: [f64; 4], width: u32, height: u32) -> PixelRect {
    let (w, h) = (width.max(1), height.max(1));
    let x = ((fractions[0] * f64::from(w)).floor() as u32).min(w - 1);
    let y = ((fractions[1] * f64::from(h)).floor() as u32).min(h - 1);
    let right = (((fractions[0] + fractions[2]) * f64::from(w)).ceil() as u32).clamp(x + 1, w);
    let bottom = (((fractions[1] + fractions[3]) * f64::from(h)).ceil() as u32).clamp(y + 1, h);
    PixelRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

/// The brightness (Rec. 601 weights, integers) of every pixel of a BGRA picture.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::pixels::luma;
///
/// // one white pixel, one black, one pure green (BGRA order)
/// assert_eq!(luma(&[255, 255, 255, 255, 0, 0, 0, 255, 0, 255, 0, 255]), [255, 0, 149]);
/// ```
#[must_use]
pub fn luma(bgra: &[u8]) -> Vec<u8> {
    bgra.as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| {
            ((u32::from(pixel[2]) * 299 + u32::from(pixel[1]) * 587 + u32::from(pixel[0]) * 114)
                / 1000) as u8
        })
        .collect()
}

/// The brightness of `rect` of a picture whose rows are `width` pixels, row by row.
#[must_use]
pub fn crop_luma(luma: &[u8], width: u32, rect: PixelRect) -> Vec<u8> {
    let mut out = Vec::with_capacity(rect.width as usize * rect.height as usize);
    for row in rect.y..rect.y + rect.height {
        let start = row as usize * width as usize + rect.x as usize;
        if let Some(line) = luma.get(start..start + rect.width as usize) {
            out.extend_from_slice(line);
        }
    }
    out
}

/// The BGRA bytes of `rect` of a BGRA picture whose rows are `width` pixels.
#[must_use]
pub fn crop_bgra(bgra: &[u8], width: u32, rect: PixelRect) -> Vec<u8> {
    let mut out = Vec::with_capacity(rect.width as usize * rect.height as usize * 4);
    for row in rect.y..rect.y + rect.height {
        let start = (row as usize * width as usize + rect.x as usize) * 4;
        if let Some(line) = bgra.get(start..start + rect.width as usize * 4) {
            out.extend_from_slice(line);
        }
    }
    out
}

/// How two same-sized brightness pictures differ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Delta {
    /// How many pixels moved by more than the threshold.
    pub changed: usize,
    /// How many pixels were compared.
    pub total: usize,
    /// The mean of the absolute brightness difference over all pixels, 0 to 255.
    pub mean: f64,
    /// The largest absolute brightness difference of any pixel.
    pub peak: u8,
    /// The box around the changed pixels, `(min_x, min_y, max_x, max_y)` in pixels of the
    /// pictures, or `None` if nothing changed.
    pub bounds: Option<(u32, u32, u32, u32)>,
}

impl Delta {
    /// The share of pixels that changed, 0 to 1.
    #[must_use]
    pub fn share(&self) -> f64 {
        self.changed as f64 / self.total.max(1) as f64
    }
}

/// Compares two brightness pictures `width` pixels wide. A pixel has changed if its brightness
/// moved by more than `threshold`. Pictures of different lengths are compared as far as the
/// shorter goes.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::pixels::delta;
///
/// // a 2 x 2 picture where the bottom right pixel went from black to white
/// let found = delta(&[0, 0, 0, 0], &[0, 0, 0, 255], 2, 24);
/// assert_eq!(found.changed, 1);
/// assert_eq!(found.share(), 0.25);
/// assert_eq!(found.peak, 255);
/// assert_eq!(found.bounds, Some((1, 1, 1, 1)));
/// ```
#[must_use]
pub fn delta(before: &[u8], after: &[u8], width: u32, threshold: i32) -> Delta {
    let (mut changed, mut sum, mut peak) = (0_usize, 0_u64, 0_u8);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0_u32, 0_u32);
    let width = width.max(1);
    for (index, (a, b)) in before.iter().zip(after).enumerate() {
        let difference = (i32::from(*a) - i32::from(*b)).unsigned_abs() as u8;
        sum += u64::from(difference);
        peak = peak.max(difference);
        if i32::from(difference) > threshold {
            let (x, y) = (index as u32 % width, index as u32 / width);
            changed += 1;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }

    let total = before.len().min(after.len());
    Delta {
        changed,
        total,
        mean: sum as f64 / total.max(1) as f64,
        peak,
        bounds: (changed > 0).then_some((min_x, min_y, max_x, max_y)),
    }
}

/// How even a patch of the screen is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Evenness {
    /// The variance of brightness over the patch.
    pub variance: f64,
    /// The mean colour, `[red, green, blue]`.
    pub rgb: [u8; 3],
}

/// Measures a BGRA patch: the variance of its brightness and its mean colour.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::pixels::evenness;
///
/// let white = [255_u8; 16]; // four white pixels
/// let found = evenness(&white);
/// assert_eq!(found.variance, 0.0);
/// assert_eq!(found.rgb, [255, 255, 255]);
/// ```
#[must_use]
pub fn evenness(bgra: &[u8]) -> Evenness {
    let pixels = bgra.as_chunks::<4>().0;
    let count = pixels.len().max(1) as f64;
    let (mut r, mut g, mut b) = (0_u64, 0_u64, 0_u64);
    let (mut sum, mut square) = (0_f64, 0_f64);
    for pixel in pixels {
        r += u64::from(pixel[2]);
        g += u64::from(pixel[1]);
        b += u64::from(pixel[0]);
        let l = f64::from(
            (u32::from(pixel[2]) * 299 + u32::from(pixel[1]) * 587 + u32::from(pixel[0]) * 114)
                / 1000,
        );
        sum += l;
        square += l * l;
    }

    let mean = sum / count;
    Evenness {
        variance: (square / count - mean * mean).max(0.0),
        rgb: [
            (r as f64 / count).round() as u8,
            (g as f64 / count).round() as u8,
            (b as f64 / count).round() as u8,
        ],
    }
}

/// Names a colour a person would: `white`, `black` or `gray` when the channels agree, otherwise
/// the hex code (`#1a2b3c`).
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::analysis::pixels::colour_name;
///
/// assert_eq!(colour_name([250, 252, 255]), "white");
/// assert_eq!(colour_name([3, 3, 3]), "black");
/// assert_eq!(colour_name([128, 128, 128]), "gray");
/// assert_eq!(colour_name([200, 0, 16]), "#c80010");
/// ```
#[must_use]
pub fn colour_name(rgb: [u8; 3]) -> String {
    let (low, high) = (
        rgb.iter().copied().min().unwrap_or(0),
        rgb.iter().copied().max().unwrap_or(0),
    );
    if high - low <= 12 {
        let mean = (u32::from(rgb[0]) + u32::from(rgb[1]) + u32::from(rgb[2])) / 3;
        return match mean {
            224.. => "white",
            ..32 => "black",
            _ => "gray",
        }
        .to_string();
    }

    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crop_is_cut_off_at_the_display_and_refused_outside_it() {
        let frame = [0.0, 0.0, 100.0, 100.0];
        let crop = Crop::from_rect([50.0, -20.0, 100.0, 40.0]);
        assert_eq!(
            crop.fractions(frame).expect("overlaps"),
            [0.5, 0.0, 0.5, 0.2]
        );
        assert!(
            Crop::from_rect([200.0, 0.0, 10.0, 10.0])
                .fractions(frame)
                .is_err()
        );
    }

    #[test]
    fn a_pixel_rect_is_at_least_one_pixel_and_inside_the_frame() {
        let rect = pixel_rect([0.999, 0.999, 0.0005, 0.0005], 10, 10);
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (9, 9, 1, 1));
        let whole = pixel_rect([0.0, 0.0, 1.0, 1.0], 7, 5);
        assert_eq!((whole.width, whole.height), (7, 5));
    }

    #[test]
    fn cropping_takes_the_rows_of_the_rect() {
        let picture: Vec<u8> = (0..16).collect(); // 4 x 4
        let rect = PixelRect {
            x: 1,
            y: 2,
            width: 2,
            height: 2,
        };
        assert_eq!(crop_luma(&picture, 4, rect), [9, 10, 13, 14]);
        let bgra: Vec<u8> = (0..64).collect();
        assert_eq!(
            crop_bgra(&bgra, 4, rect)[..8],
            [36, 37, 38, 39, 40, 41, 42, 43]
        );
    }

    #[test]
    fn a_small_change_under_the_threshold_is_not_a_changed_pixel_but_is_in_the_mean() {
        let found = delta(&[100, 100], &[110, 100], 2, 24);
        assert_eq!(found.changed, 0);
        assert_eq!(found.bounds, None);
        assert_eq!(found.mean, 5.0);
        assert_eq!(found.peak, 10);
    }

    #[test]
    fn the_box_of_change_covers_every_changed_pixel() {
        let before = vec![0; 16];
        let mut after = vec![0; 16];
        after[1] = 255; // (1, 0)
        after[14] = 255; // (2, 3)
        let found = delta(&before, &after, 4, 24);
        assert_eq!(found.bounds, Some((1, 0, 2, 3)));
    }

    #[test]
    fn uniform_patches_have_no_variance_and_noisy_ones_do() {
        assert_eq!(
            evenness(&[10, 20, 30, 255, 10, 20, 30, 255]).rgb,
            [30, 20, 10]
        );
        assert!(evenness(&[0, 0, 0, 255, 255, 255, 255, 255]).variance > 10_000.0);
    }
}
