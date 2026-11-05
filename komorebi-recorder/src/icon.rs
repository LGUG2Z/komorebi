//! The icon of every window the program opens: a slice of watermelon.
//!
//! The picture is `assets/watermelon.svg`. `assets/generate.py` (`just icon`) renders it into `assets/watermelon.png`,
//! which this module embeds, and `assets/watermelon.ico`, which Windows shows for the executables (`build.rs` compiles
//! it into them) and for the installer. What each host does with the icon is in `docs/PLATFORM.md`.

use eframe::egui;
use std::io::Cursor;

/// The watermelon, 256 by 256 pixels of RGBA.
const PNG: &[u8] = include_bytes!("../assets/watermelon.png");

/// Returns the icon of the program's windows: a slice of watermelon on a transparent background, 256 by 256 pixels.
///
/// The recorder window and the viewer pass it to [`egui::ViewportBuilder::with_icon`]. eframe sets it as the window and
/// taskbar icon on Windows and as the Dock icon on macOS.
///
/// The picture is part of the binary, and a test holds that it decodes. If it ever did not, this returns an empty
/// [`egui::IconData`], which the window system takes to mean no icon: the window opens with the system's default.
///
/// # Examples
///
/// ```rust
/// let icon = komorebi_recorder::icon::window_icon();
///
/// assert_eq!((icon.width, icon.height), (256, 256));
/// assert_eq!(icon.rgba.len(), 256 * 256 * 4);
///
/// // the corner of the picture is transparent and its middle is not
/// assert_eq!(icon.rgba[3], 0);
/// assert_eq!(icon.rgba[(128 * 256 + 128) * 4 + 3], 255);
/// ```
pub fn window_icon() -> egui::IconData {
    decode().unwrap_or_default()
}

/// The picture as RGBA, or nothing if it is not an RGBA PNG with eight bits a channel, which is what `generate.py`
/// writes.
fn decode() -> Option<egui::IconData> {
    let mut reader = png::Decoder::new(Cursor::new(PNG)).read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut rgba).ok()?;

    (info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight).then(|| {
        rgba.truncate(info.buffer_size());
        egui::IconData {
            rgba,
            width: info.width,
            height: info.height,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICO: &[u8] = include_bytes!("../assets/watermelon.ico");

    #[test]
    fn the_embedded_picture_decodes() {
        let icon = decode().expect("watermelon.png is an RGBA PNG with eight bits a channel");

        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        assert_eq!(window_icon(), icon);
    }

    #[test]
    fn the_windows_icon_holds_the_same_picture_at_256_pixels() {
        assert_eq!(ICO[..4], [0, 0, 1, 0], "an icon file starts with its type");
        let count = usize::from(u16::from_le_bytes([ICO[4], ICO[5]]));
        assert!(
            count > 1,
            "one size is not enough for a taskbar and a window"
        );

        let mut widths = Vec::new();
        for entry in ICO[6..].as_chunks::<16>().0.iter().take(count) {
            // a width of 0 means 256
            let width = entry[0];
            let pixels = if width == 0 { 256 } else { u32::from(width) };
            let length = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
            let offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
            let image = &ICO[offset..offset + length];

            assert!(
                image.starts_with(b"\x89PNG"),
                "the {pixels} pixel entry is not a PNG"
            );
            if width == 0 {
                assert_eq!(image, PNG, "the 256 pixel entry is not watermelon.png");
            }
            widths.push(width);
        }

        assert!(widths.contains(&0), "no 256 pixel entry in {widths:?}");
        assert!(widths.contains(&16), "no 16 pixel entry in {widths:?}");
    }
}
