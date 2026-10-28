//! A contact sheet: frames of the recording laid out on one image, with the times, the events and
//! the bug regions marked.
//!
//! It is for someone (or an agent that can look at an image) to see what the screen did without
//! playing a video.
//!
//! Each display is its own block of frames in time order. Every frame says when it is; where
//! komorebi events fall around it they are named; frames inside a bug region the user marked have
//! a red border. Frames are decoded at the width they are drawn, so a sheet of a long recording
//! costs little.
//!
//! [`Analysis::sheet`] draws one from [`SheetOptions`] and a `SessionVideo` (the decoder of the
//! session's video, in `analysis::video`) and returns a [`SheetData`] saying where each frame is on
//! the image, so that a reader of the answer can find a frame without looking at the picture. The
//! image is a PNG written to [`SheetOptions::out`]. Decoding needs a host that can decode the video
//! (macOS and Windows; see `crate::av::supported`).
//!
//! # Layout
//!
//! From the top: a title with the session's directory name and the first and last of the moments;
//! under it, if a bug region overlaps those moments, a red line with the first such region's id and
//! its description (cut to 150 characters); then, for each display with a frame to show, a header
//! naming it and its frames in rows of [`SheetOptions::columns`]. Under each frame is its timecode
//! (red, with `BUG`, if it is inside a region) and the first two events received near it, as `#<seq>
//! <name>`, with `+<n>` for any more. An event is near a frame if it was received within half the
//! average spacing of the moments (half a second for a single moment) of the frame's moment, but
//! never less than a tenth of a second.
//!
//! The drawn width of a frame is [`SheetOptions::cell_width`], limited to between 64 and 1920
//! pixels, and the number of columns is limited to the number of moments. The image may be at most
//! 8192 pixels wide and 16384 tall; a sheet that would be larger is refused (see
//! [`Analysis::sheet`]).
//!
//! # Example
//!
//! Needs `ffmpeg` to make a test video, and a host that can decode it, so it is only compiled here;
//! the tests `a_sheet_shows_the_frame_that_was_on_screen_at_each_moment` and
//! `a_sheet_marks_the_bug_region_and_names_the_events_around_a_frame` in `tests/video.rs` run the
//! same call through the `sheet` command.
//!
//! ```rust,no_run
//! use komorebi_recorder::analysis::Analysis;
//! use komorebi_recorder::analysis::sheet::SheetOptions;
//! use komorebi_recorder::analysis::video::SessionVideo;
//! use komorebi_recorder::testkit::SessionBuilder;
//!
//! // a recording of one display, 300 frames at 30 frames per second
//! let (session, _) = SessionBuilder::new("doc-sheet").video(1, 0.0, 300, 30).build();
//! let analysis = Analysis::open(&session.dir)?;
//! let video = SessionVideo::new(&analysis.manifest, analysis.guard());
//!
//! let sheet = analysis.sheet(&video, &SheetOptions {
//!     times: vec![1.0, 5.0, 9.0],
//!     displays: video.displays(),
//!     cell_width: 240,
//!     columns: 3,
//!     regions: Vec::new(),
//!     out: session.path("sheets/example.png"),
//! })?;
//!
//! assert_eq!(sheet.frames.len(), 3);
//! assert!(sheet.skipped.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::Analysis;
use super::envelope;
use super::video::SessionVideo;
use crate::av::Scale;
use crate::timecode;
use ab_glyph::Font;
use ab_glyph::FontRef;
use ab_glyph::PxScale;
use ab_glyph::ScaleFont;
use ab_glyph::point;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use schemars::JsonSchema;
use serde::Serialize;
use std::io::BufWriter;
use std::path::Path;
use std::path::PathBuf;

/// The `schema` of the `sheet` command's output: the name that [`SheetData`] has in an envelope.
pub const SCHEMA_SHEET: &str = "sheet/1";

const BACKGROUND: [u8; 3] = [24, 24, 28];
const TEXT: [u8; 3] = [225, 225, 230];
const DIM: [u8; 3] = [140, 140, 150];
const EVENT: [u8; 3] = [240, 190, 70];
const BUG: [u8; 3] = [230, 70, 60];
const MARGIN: u32 = 10;
const LABEL_HEIGHT: u32 = 44;
const HEADER_HEIGHT: u32 = 26;
const TITLE_HEIGHT: u32 = 44;
const MAX_WIDTH: u32 = 8192;
const MAX_HEIGHT: u32 = 16384;

/// A bug region to mark on the sheet.
#[derive(Debug, Clone)]
pub struct Region {
    /// The annotation's id, written on the sheet and in [`SheetFrame::bugs`].
    pub id: String,
    /// Where the region starts, in seconds of video time.
    pub start_s: f64,
    /// Where the region ends, in seconds of video time. The `sheet` command gives an open-ended
    /// bug the end of the recording.
    pub end_s: f64,
    /// What the user said is wrong, written under the title of the sheet.
    pub text: String,
}

/// What to put on a sheet, and where to write it.
#[derive(Debug, Clone)]
pub struct SheetOptions {
    /// The moments to show, in session video time. They are drawn in this order, so a caller that
    /// wants them in time order sorts them. Must not be empty.
    pub times: Vec<f64>,
    /// The displays to show, each as a block of its own. Must not be empty; a display listed that
    /// has no video at any of the moments is left out.
    pub displays: Vec<u32>,
    /// How wide each frame is drawn, in pixels; limited to between 64 and 1920.
    pub cell_width: u32,
    /// How many frames to a row; limited to between 1 and the number of moments.
    pub columns: usize,
    /// The bug regions to mark: a frame whose moment is inside one has a red border.
    pub regions: Vec<Region>,
    /// Where to write the PNG. Its parent directories are created, and a file already there is
    /// replaced.
    pub out: PathBuf,
}

/// One frame of a sheet and where it is on the image.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SheetFrame {
    /// The display the frame is of.
    pub display: u32,
    /// The moment that was asked for, in seconds of video time.
    pub requested_video_s: f64,
    /// When the frame shown was actually put on screen, in seconds of video time. It is the picture
    /// that was on screen at the requested moment, so it is never later than it, and can be
    /// earlier: a still screen produces no new frames.
    pub shown_video_s: f64,
    /// Where it is on the image: pixels from the left edge to its left edge.
    pub x: u32,
    /// Where it is on the image: pixels from the top edge to its top edge.
    pub y: u32,
    /// How wide it is drawn, in pixels.
    pub width: u32,
    /// How tall it is drawn, in pixels.
    pub height: u32,
    /// The sequence numbers of the events received near it.
    pub events: Vec<u64>,
    /// The ids of the bug regions it is inside.
    pub bugs: Vec<String>,
}

/// A moment of a display that the sheet has no frame for.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Skipped {
    /// The display that was asked for.
    pub display: u32,
    /// The moment that was asked for, in seconds of video time.
    pub video_s: f64,
    /// Why there is no frame.
    pub reason: String,
}

/// The answer to `sheet`: the image that was written and where each frame is on it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SheetData {
    /// Where the PNG was written.
    pub path: String,
    /// The width of the image, in pixels.
    pub width: u32,
    /// The height of the image, in pixels.
    pub height: u32,
    /// The frames on the image, a display at a time and in the order of the moments within it.
    pub frames: Vec<SheetFrame>,
    /// Moments a display had no video.
    pub skipped: Vec<Skipped>,
}

pub(super) struct Canvas {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Canvas {
    pub(super) fn new(width: u32, height: u32) -> Self {
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..width as usize * height as usize {
            rgba.extend_from_slice(&[BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], 255]);
        }
        Self {
            width,
            height,
            rgba,
        }
    }

    pub(super) fn put(&mut self, x: i64, y: i64, color: [u8; 3], alpha: f32) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }

        let at = (y as usize * self.width as usize + x as usize) * 4;
        for (channel, wanted) in color.iter().enumerate() {
            let below = f32::from(self.rgba[at + channel]);
            self.rgba[at + channel] = (below + (f32::from(*wanted) - below) * alpha).round() as u8;
        }
    }

    fn rectangle(&mut self, x: u32, y: u32, width: u32, height: u32, color: [u8; 3]) {
        for row in y..y + height {
            for column in x..x + width {
                self.put(i64::from(column), i64::from(row), color, 1.0);
            }
        }
    }

    pub(super) fn border(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        thickness: u32,
        color: [u8; 3],
    ) {
        self.rectangle(x, y, width, thickness, color);
        self.rectangle(x, y + height - thickness, width, thickness, color);
        self.rectangle(x, y, thickness, height, color);
        self.rectangle(x + width - thickness, y, thickness, height, color);
    }

    /// Copy a BGRA frame in at `x`, `y`.
    fn blit(&mut self, x: u32, y: u32, width: u32, height: u32, bgra: &[u8]) {
        for row in 0..height {
            for column in 0..width {
                let from = (row as usize * width as usize + column as usize) * 4;
                if from + 2 >= bgra.len() {
                    return;
                }
                self.put(
                    i64::from(x + column),
                    i64::from(y + row),
                    [bgra[from + 2], bgra[from + 1], bgra[from]],
                    1.0,
                );
            }
        }
    }

    fn text(&mut self, font: &FontRef, size: f32, x: u32, y: u32, text: &str, color: [u8; 3]) {
        let scaled = font.as_scaled(PxScale::from(size));
        let baseline = y as f32 + scaled.ascent();
        let mut caret = x as f32;
        for character in text.chars() {
            let id = scaled.glyph_id(character);
            let glyph = id.with_scale_and_position(PxScale::from(size), point(caret, baseline));
            caret += scaled.h_advance(id);
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, coverage| {
                    self.put(
                        i64::from(bounds.min.x as i32) + i64::from(gx),
                        i64::from(bounds.min.y as i32) + i64::from(gy),
                        color,
                        coverage,
                    );
                });
            }
        }
    }

    pub(super) fn save(&self, path: &Path) -> eyre::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err_with(|| format!("could not create {}", parent.display()))?;
        }

        let file = std::fs::File::create(path)
            .wrap_err_with(|| format!("could not create {}", path.display()))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)?;
        writer.finish()?;
        Ok(())
    }
}

fn truncate(text: &str, limit: usize) -> String {
    let single: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.chars().count() <= limit {
        single
    } else {
        let mut cut: String = single.chars().take(limit.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

impl Analysis {
    /// Draws a contact sheet of the moments in `options` and writes it as a PNG.
    ///
    /// Each frame is the one that was on screen at its moment, decoded at the drawn width. A moment
    /// at which a display had no video (it was unplugged, asleep, or not yet recorded) is skipped
    /// and listed in [`SheetData::skipped`] rather than failing the sheet. See the module docs for
    /// the layout.
    ///
    /// # Errors
    ///
    /// - a usage error if `options.times` is empty, or if the sheet would be wider than 8192 or
    ///   taller than 16384 pixels (the message says how big it would be; show fewer frames or draw
    ///   them narrower);
    /// - an unusable error if `options.displays` is empty, which is what the commands pass for a
    ///   session that recorded no video;
    /// - a not-found error if none of the moments has video on any of the displays;
    /// - an error if a video file cannot be opened or decoded (including on a host that cannot
    ///   decode video), if the output directory or file cannot be created, or if the PNG cannot be
    ///   written.
    ///
    /// # Example
    ///
    /// See the [module docs](self).
    pub fn sheet(&self, video: &SessionVideo, options: &SheetOptions) -> eyre::Result<SheetData> {
        if options.times.is_empty() {
            return Err(envelope::usage("there are no moments to show"));
        }
        if options.displays.is_empty() {
            return Err(envelope::unusable("the session recorded no video"));
        }

        let font = FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR)
            .map_err(|_| eyre::Report::msg("the built-in font could not be read"))?;
        let columns = options.columns.clamp(1, options.times.len());
        let cell = options.cell_width.clamp(64, 1920);

        // decode every frame first, so the layout knows each one's height
        struct Cell {
            requested: f64,
            shown: f64,
            width: u32,
            height: u32,
            bgra: Vec<u8>,
        }
        let mut blocks: Vec<(u32, Vec<Cell>)> = Vec::new();
        let mut skipped = Vec::new();
        for display in &options.displays {
            let mut cells = Vec::new();
            for time in &options.times {
                match video.frame_at(*display, *time, Scale::Width(cell))? {
                    Some(decoded) => cells.push(Cell {
                        requested: *time,
                        shown: decoded.shown_s,
                        width: decoded.frame.width,
                        height: decoded.frame.height,
                        bgra: decoded.frame.bgra,
                    }),
                    None => skipped.push(Skipped {
                        display: *display,
                        video_s: *time,
                        reason:
                            "the display had no video then (unplugged, asleep, or not yet recorded)"
                                .to_string(),
                    }),
                }
            }
            if !cells.is_empty() {
                blocks.push((*display, cells));
            }
        }
        if blocks.is_empty() {
            return Err(envelope::not_found(
                "none of those moments has video on any display",
            ));
        }

        let width = MARGIN + columns as u32 * (cell + MARGIN);
        let mut height = TITLE_HEIGHT + MARGIN;
        for (_, cells) in &blocks {
            height += HEADER_HEIGHT;
            for row in cells.chunks(columns) {
                height +=
                    row.iter().map(|cell| cell.height).max().unwrap_or(0) + LABEL_HEIGHT + MARGIN;
            }
        }
        if width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(envelope::usage(format!(
                "a sheet of {} frames at width {cell} would be {width}x{height}: show fewer frames \
                 or draw them narrower",
                options.times.len()
            )));
        }

        let mut canvas = Canvas::new(width, height);
        let first = options.times.iter().copied().fold(f64::INFINITY, f64::min);
        let last = options
            .times
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let session_name = self
            .session
            .dir
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        canvas.text(
            &font,
            15.0,
            MARGIN,
            6,
            &format!(
                "{session_name}   {} - {}",
                timecode::format(first),
                timecode::format(last)
            ),
            TEXT,
        );
        if let Some(region) = options
            .regions
            .iter()
            .find(|region| region.end_s >= first && region.start_s <= last)
        {
            canvas.text(
                &font,
                12.0,
                MARGIN,
                26,
                &format!("BUG {}: {}", region.id, truncate(&region.text, 150)),
                BUG,
            );
        }

        // spread of the moments, to know how near an event has to be to belong to a frame
        let step = if options.times.len() > 1 {
            (last - first) / (options.times.len() - 1) as f64
        } else {
            1.0
        };
        let reach = (step / 2.0).max(0.1);

        let mut frames = Vec::new();
        let mut y = TITLE_HEIGHT + MARGIN;
        for (display, cells) in &blocks {
            canvas.text(
                &font,
                14.0,
                MARGIN,
                y + 4,
                &format!("display {display}"),
                DIM,
            );
            y += HEADER_HEIGHT;

            for row in cells.chunks(columns) {
                let row_height = row.iter().map(|cell| cell.height).max().unwrap_or(0);
                for (column, cell_frame) in row.iter().enumerate() {
                    let x = MARGIN + column as u32 * (cell + MARGIN);
                    canvas.blit(x, y, cell_frame.width, cell_frame.height, &cell_frame.bgra);

                    let bugs: Vec<String> = options
                        .regions
                        .iter()
                        .filter(|region| {
                            cell_frame.requested >= region.start_s - 1e-6
                                && cell_frame.requested <= region.end_s + 1e-6
                        })
                        .map(|region| region.id.clone())
                        .collect();
                    if !bugs.is_empty() {
                        canvas.border(x, y, cell_frame.width, cell_frame.height, 3, BUG);
                    }

                    let events: Vec<u64> = self
                        .events()
                        .iter()
                        .filter(|event| {
                            (self.video_s(event.recv_mach_ns) - cell_frame.requested).abs() <= reach
                        })
                        .map(|event| event.seq)
                        .collect();

                    let label_y = y + row_height + 3;
                    canvas.text(
                        &font,
                        13.0,
                        x,
                        label_y,
                        &timecode::format(cell_frame.requested),
                        if bugs.is_empty() { TEXT } else { BUG },
                    );
                    if !bugs.is_empty() {
                        canvas.text(&font, 12.0, x + 92, label_y, "BUG", BUG);
                    }
                    let named: Vec<String> = events
                        .iter()
                        .take(2)
                        .filter_map(|seq| {
                            let index = self.event_index(*seq)?;
                            let event = &self.events()[index];
                            Some(format!("#{seq} {}", event.info.name))
                        })
                        .collect();
                    if !named.is_empty() {
                        let more = events.len().saturating_sub(2);
                        canvas.text(
                            &font,
                            11.0,
                            x,
                            label_y + 16,
                            &truncate(
                                &format!(
                                    "{}{}",
                                    named.join("  "),
                                    if more > 0 {
                                        format!("  +{more}")
                                    } else {
                                        String::new()
                                    }
                                ),
                                (cell / 7) as usize,
                            ),
                            EVENT,
                        );
                    }

                    frames.push(SheetFrame {
                        display: *display,
                        requested_video_s: cell_frame.requested,
                        shown_video_s: cell_frame.shown,
                        x,
                        y,
                        width: cell_frame.width,
                        height: cell_frame.height,
                        events,
                        bugs,
                    });
                }
                y += row_height + LABEL_HEIGHT + MARGIN;
            }
        }

        canvas.save(&options.out)?;
        Ok(SheetData {
            path: options.out.display().to_string(),
            width,
            height,
            frames,
            skipped,
        })
    }
}
