//! The sync self-test: prove that "video second N" really is "clock time N".
//!
//! A child process (so the recorder does not exclude it from the capture along with its own
//! windows) draws a large live clock plus a strip of 40 black/white cells that binary-encodes the
//! host clock in milliseconds at the moment each frame was painted. The parent records the screen
//! headlessly, then for several video timestamps decodes the strip out of the extracted frame and
//! compares it with the time the session's manifest predicts for that video second. Nothing is
//! read by eye or OCR.
//!
//! What the comparison includes beyond the recorder's own accuracy: the clock is painted a
//! little before the compositor shows it and SCK frames are rate-limited, so the visible clock
//! trails the video time by one to a few frame intervals. That is why the default tolerance is
//! 100 ms and the mean bias is reported separately.
//!
//! # The pieces
//!
//! | Item | Is |
//! |---|---|
//! | [`run`] | the `selftest` command: records, decodes, compares, and stores the verdict in the manifest |
//! | [`run_clock`] | the child process: the window that paints the clock and the strip (the hidden `selftest-clock` command) |
//! | [`encode_cells`], [`decode_cells`] | the strip: a clock value to cell colors, and the average luminance of each cell back to a clock value |
//! | [`strip_filter`], [`strip_units`] | where the strip is in a display's video, as an `ffmpeg` crop |
//! | [`wrapped_difference_ms`], [`evaluate`] | the comparison, and the verdict ([`Reading`] in, [`Calibration`] out) |
//!
//! The pure parts (the strip, the geometry, the comparison) are the same on both hosts and are
//! tested without a screen. [`run`] and [`run_clock`] need a screen, a window system and `ffmpeg`,
//! so no automated test covers them: the `selftest` command is the check, and
//! `docs/dev/windows-checkpoints.md` (W2) says what to expect of it on Windows.
//!
//! # The strip
//!
//! The strip is 40 cells, one for each bit of the master clock in milliseconds, most significant
//! first, white for 1 and black for 0, of the value taken modulo 2^40 (a little under 35 years of
//! milliseconds, so it does not wrap in practice). It is painted at the same place in the clock
//! window every frame, and [`strip_filter`] says where that is in a display's video. The window
//! also shows the master clock and the wall clock as text, for a person looking at the video.
//!
//! # Hosts
//!
//! The recording itself is the host's capture. The geometry differs: on macOS the window's
//! position and the display's frame are both in points, and the video is a number of pixels to the
//! point; on Windows the display's frame is physical pixels and the window's position is still in
//! points. [`strip_units`] brings them to common units.
//!
//! # Example
//!
//! The strip carries a clock reading, and the difference between what the video shows and what the
//! manifest predicts is the error:
//!
//! ```rust
//! use komorebi_recorder::selftest::{decode_cells, encode_cells, evaluate, Reading};
//!
//! // the clock at the moment a frame was painted, in ms: encoded as cells, then read back from
//! // the average luminance of each cell in the video
//! let painted_ms = 10_000;
//! let luminance: Vec<u8> = encode_cells(painted_ms)
//!     .iter()
//!     .map(|white| if *white { 255 } else { 0 })
//!     .collect();
//! let observed_ms = decode_cells(&luminance).expect("a legible strip");
//! assert_eq!(observed_ms, painted_ms);
//!
//! // the manifest predicted the video second to be at 10 030 ms: the video is 30 ms behind
//! let verdict = evaluate(
//!     &[Reading { video_s: 1.0, predicted_ms: 10_030, observed_ms }],
//!     100.0,
//! );
//! assert!(verdict.passed);
//! assert_eq!(verdict.samples[0].error_ms, -30.0);
//! ```

use crate::analysis::envelope::Envelope;
use crate::cli::SelftestArgs;
use crate::cli::outputs::SCHEMA_SELFTEST;
use crate::cli::outputs::SelftestData;
use crate::cli::outputs::SelftestSample;
use crate::clock;
use crate::frames;
use crate::platform::capture;
use crate::recorder;
use crate::recorder::Live;
use crate::recorder::OsLogOptions;
use crate::recorder::RecordOptions;
use crate::session::Calibration;
use crate::session::CalibrationSample;
use crate::session::Manifest;
use crate::session::Session;
use crate::timecode;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use eframe::egui;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

/// Number of cells in the strip, i.e. bits of the encoded millisecond clock.
const CELLS: usize = 40;
const CELL_WIDTH: f32 = 24.0;
const CELL_HEIGHT: f32 = 60.0;
/// Where the strip sits inside the clock window, in points.
const STRIP_X: f32 = 20.0;
const STRIP_Y: f32 = 110.0;
const WINDOW_SIZE: [f32; 2] = [1000.0, 260.0];
const WINDOW_POSITION: [f32; 2] = [80.0, 80.0];

const WRAP_MS: u64 = 1 << CELLS;

/// Returns the cell colors (true = white) for a clock value in milliseconds, most significant
/// first.
///
/// There are 40 cells, so the value is taken modulo 2^40: bits above the lowest 40 are not shown.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::selftest::encode_cells;
///
/// // 5 is 101 in binary: the last three cells, with the rest black
/// let cells = encode_cells(5);
/// assert_eq!(cells.len(), 40);
/// assert!(cells[..37].iter().all(|white| !white));
/// assert_eq!(cells[37..], [true, false, true]);
/// ```
#[must_use]
pub fn encode_cells(ms: u64) -> [bool; CELLS] {
    let mut cells = [false; CELLS];
    for (index, cell) in cells.iter_mut().enumerate() {
        *cell = (ms >> (CELLS - 1 - index)) & 1 == 1;
    }
    cells
}

/// Recovers the clock value from the average luminance of each cell, 0 (black) to 255 (white).
///
/// A cell at 128 or above is white. Returns `None` if there are not exactly 40 values, or if more
/// than 3 of them are neither near black nor near white (from 64 up to, but not including, 192),
/// which is what the average of a crop that missed the strip looks like. Cells that are soft but
/// still clearly one or the other, as anti-aliasing and video compression make them, decode.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::selftest::{decode_cells, encode_cells};
///
/// // a clock value survives encoding and reading back from the cells' luminance
/// let ms = 1_210_934_273;
/// let sharp: Vec<u8> = encode_cells(ms).iter().map(|white| if *white { 255 } else { 0 }).collect();
/// assert_eq!(decode_cells(&sharp), Some(ms));
///
/// // soft edges still decode
/// let soft: Vec<u8> = sharp.iter().map(|level| if *level == 255 { 230 } else { 12 }).collect();
/// assert_eq!(decode_cells(&soft), Some(ms));
///
/// // flat gray is a crop that missed the strip, and a short crop is not a strip
/// assert_eq!(decode_cells(&[128; 40]), None);
/// assert_eq!(decode_cells(&[0; 10]), None);
/// ```
#[must_use]
pub fn decode_cells(luminance: &[u8]) -> Option<u64> {
    if luminance.len() != CELLS {
        return None;
    }

    // a legible strip is overwhelmingly near-black or near-white; a mid-grey average means the
    // crop missed the strip
    let ambiguous = luminance
        .iter()
        .filter(|value| (64..192).contains(*value))
        .count();
    if ambiguous > 3 {
        return None;
    }

    Some(luminance.iter().fold(0_u64, |value, level| {
        (value << 1) | u64::from(*level >= 128)
    }))
}

/// Returns `observed - predicted` in milliseconds, accounting for the strip's wrap-around.
///
/// The strip holds a clock value modulo 2^40 milliseconds, so two values on either side of a wrap
/// are close, however far apart they are as numbers. The result is the shorter way round: within
/// half the wrap, in either direction.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::selftest::wrapped_difference_ms;
///
/// assert_eq!(wrapped_difference_ms(1_050, 1_000), 50);
/// assert_eq!(wrapped_difference_ms(950, 1_000), -50);
///
/// // the strip wrapped between the prediction and the observation
/// let wrap = 1_u64 << 40;
/// assert_eq!(wrapped_difference_ms(5, wrap - 5), 10);
/// assert_eq!(wrapped_difference_ms(wrap - 5, 5), -10);
/// ```
#[must_use]
pub fn wrapped_difference_ms(observed: u64, predicted: u64) -> i64 {
    let wrapped = (observed.wrapping_sub(predicted)) % WRAP_MS;
    if wrapped >= WRAP_MS / 2 {
        wrapped as i64 - WRAP_MS as i64
    } else {
        wrapped as i64
    }
}

/// Returns the ffmpeg `-vf` expression that crops the strip out of a display's video and averages
/// each cell down to one gray pixel.
///
/// `window_origin` is the clock window's top left corner and `display_origin` the display's, in
/// the same units, and `scale` is how many pixels of video one of those units is. The crop is the
/// strip's position in the window (20, 110) and its size (40 cells of 24 by 60) in those units,
/// multiplied by `scale`, and cut down to the middle half of the cells' height so that the edges
/// of the strip do not blur into the average. The cells are then scaled to 40 by 1 pixels with area
/// averaging.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::selftest::strip_filter;
///
/// // the window at (80, 80) on a display at the origin, one pixel to the unit
/// assert_eq!(
///     strip_filter((80.0, 80.0), (0.0, 0.0), 1.0),
///     "crop=960:30:100:205,scale=40:1:flags=area"
/// );
///
/// // the same window on a display to the left, two pixels to the unit
/// assert_eq!(
///     strip_filter((-1720.0, 80.0), (-1800.0, 0.0), 2.0),
///     "crop=1920:60:200:410,scale=40:1:flags=area"
/// );
/// ```
#[must_use]
pub fn strip_filter(window_origin: (f64, f64), display_origin: (f64, f64), scale: f64) -> String {
    let x = ((window_origin.0 - display_origin.0) + f64::from(STRIP_X)) * scale;
    let y = ((window_origin.1 - display_origin.1) + f64::from(STRIP_Y)) * scale;
    let width = f64::from(CELL_WIDTH) * CELLS as f64 * scale;
    // sample the middle of the cells vertically
    let height = f64::from(CELL_HEIGHT) * 0.5 * scale;
    let y = y + f64::from(CELL_HEIGHT) * 0.25 * scale;

    format!(
        "crop={}:{}:{}:{},scale={CELLS}:1:flags=area",
        width.round() as i64,
        height.round().max(1.0) as i64,
        x.round() as i64,
        y.round() as i64,
    )
}

/// Where the strip is in a display's video, as `strip_filter` wants it: the window's and the
/// display's origins in the same units, and how many pixels of video one of those units is.
///
/// On a Mac the window (egui's points) and the display (CoreGraphics' points) share units and the
/// video is `video_scale` pixels to the point. On Windows the video is the display's physical
/// pixels and the window is still in points, `pixels_per_point` of which make a pixel, so the
/// display's origin is brought into points and the unit is the pixels-per-point.
///
/// `display_frame_is_pixels` says which: true where the display's frame is in physical pixels
/// (Windows). A `pixels_per_point` below 0.1 is taken as 0.1.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::selftest::{strip_filter, strip_units};
///
/// // a Mac: both in points, the video 2 pixels to the point
/// assert_eq!(strip_units((100.0, 50.0), 2.0, 2.0, false), ((100.0, 50.0), 2.0));
///
/// // Windows at 150 %: the display at 1920 px is at 1280 points, and a point is 1.5 pixels
/// let ((x, y), unit) = strip_units((1920.0, 0.0), 1.0, 1.5, true);
/// assert_eq!((x, y, unit), (1280.0, 0.0, 1.5));
///
/// // so a window 80 points into that display puts its strip 150 pixels in
/// let filter = strip_filter((1280.0 + 80.0, 80.0), (x, y), unit);
/// assert!(filter.starts_with("crop=1440:45:150:"));
/// ```
#[must_use]
pub fn strip_units(
    display_frame_origin: (f64, f64),
    video_scale: f64,
    pixels_per_point: f64,
    display_frame_is_pixels: bool,
) -> ((f64, f64), f64) {
    if display_frame_is_pixels {
        let ppp = pixels_per_point.max(0.1);
        (
            (display_frame_origin.0 / ppp, display_frame_origin.1 / ppp),
            ppp,
        )
    } else {
        (display_frame_origin, video_scale)
    }
}

/// The child process: a borderless, untitled (so komorebi leaves it alone) window that paints
/// the clock as fast as it can.
///
/// The window is 1000 by 260 points, always on top, at (80, 80). It paints the master clock and the
/// wall clock as text, and the strip (see the [module docs](self#the-strip)) from a clock reading
/// taken as each frame's content is decided. It prints a line `RECT <x> <y> <width> <height>
/// <pixels per point>` on its standard output whenever its inner rectangle changes, which is how
/// [`run`] finds it.
///
/// This blocks until the window is closed, or the parent kills it. It exits the process after 120
/// seconds, in case the parent died without killing it.
///
/// # Errors
///
/// Returns an error if the window system cannot create the window (`the clock window failed`).
///
/// This opens a window, so it has no example and no automated test; it runs when [`run`] starts it
/// as the hidden `selftest-clock` command.
pub fn run_clock() -> eyre::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("")
            .with_decorations(false)
            .with_position(WINDOW_POSITION)
            .with_inner_size(WINDOW_SIZE)
            .with_always_on_top()
            .with_active(false),
        ..Default::default()
    };

    eframe::run_native(
        "komorebi-recorder-clock",
        options,
        Box::new(|_| Ok(Box::new(ClockApp::default()))),
    )
    .map_err(|error| eyre::Report::msg(format!("the clock window failed: {error}")))
}

#[derive(Default)]
struct ClockApp {
    last_rect: Option<egui::Rect>,
    started: Option<Instant>,
}

impl eframe::App for ClockApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        let started = *self.started.get_or_insert_with(Instant::now);
        // never linger if the parent dies without killing us
        if started.elapsed() > Duration::from_secs(120) {
            std::process::exit(0);
        }

        // read the clock first: this is the moment the frame's content is decided
        let mach_ns = clock::mono_now_ns();
        let wall_us = clock::wall_now_us();
        let ms = (mach_ns / 1_000_000) % WRAP_MS;

        if let Some(rect) = context.input(|input| input.viewport().inner_rect)
            && self.last_rect != Some(rect)
        {
            self.last_rect = Some(rect);
            println!(
                "RECT {} {} {} {} {}",
                rect.min.x,
                rect.min.y,
                rect.width(),
                rect.height(),
                context.pixels_per_point()
            );
            let _ = std::io::stdout().flush();
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_gray(40)))
            .show(context, |ui| {
                let painter = ui.painter();
                painter.text(
                    egui::pos2(20.0, 14.0),
                    egui::Align2::LEFT_TOP,
                    format!("mach {mach_ns} ns"),
                    egui::FontId::monospace(30.0),
                    egui::Color32::WHITE,
                );
                painter.text(
                    egui::pos2(20.0, 56.0),
                    egui::Align2::LEFT_TOP,
                    crate::timeline::format_wall_us(wall_us).unwrap_or_default(),
                    egui::FontId::monospace(30.0),
                    egui::Color32::LIGHT_GREEN,
                );

                for (index, white) in encode_cells(ms).into_iter().enumerate() {
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(STRIP_X + index as f32 * CELL_WIDTH, STRIP_Y),
                        egui::vec2(CELL_WIDTH, CELL_HEIGHT),
                    );
                    painter.rect_filled(
                        cell,
                        0.0,
                        if white {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::BLACK
                        },
                    );
                }
            });

        context.request_repaint();
    }
}

/// One decoded sample, before it is turned into a [`CalibrationSample`].
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// The video time the frame was taken at, in seconds.
    pub video_s: f64,
    /// The clock value the manifest predicts for that video time, in milliseconds on the master
    /// clock modulo 2^40.
    pub predicted_ms: u64,
    /// The clock value read from the strip in the frame, in the same units.
    pub observed_ms: u64,
}

/// Turns readings into a calibration verdict.
///
/// Each reading's error is the observed clock minus the predicted one, in milliseconds
/// ([`wrapped_difference_ms`]), so a negative error means the video shows a clock that is behind
/// what the manifest predicts. The verdict passes if there is at least one reading and the largest
/// error, in either direction, is no more than `tolerance_ms`. The verdict stores the readings as
/// samples, with the times in nanoseconds on the master clock.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::selftest::{evaluate, Reading};
///
/// let readings = vec![
///     Reading { video_s: 1.0, predicted_ms: 10_000, observed_ms: 9_970 },
///     Reading { video_s: 1.5, predicted_ms: 10_500, observed_ms: 10_460 },
/// ];
///
/// let verdict = evaluate(&readings, 100.0);
/// assert!(verdict.passed);
/// assert_eq!(verdict.max_abs_error_ms, 40.0);
/// assert_eq!(verdict.samples[0].error_ms, -30.0);
///
/// // the same readings against a tighter tolerance
/// assert!(!evaluate(&readings, 35.0).passed);
///
/// // no readings is not a pass
/// assert!(!evaluate(&[], 100.0).passed);
/// ```
#[must_use]
pub fn evaluate(readings: &[Reading], tolerance_ms: f64) -> Calibration {
    let samples: Vec<CalibrationSample> = readings
        .iter()
        .map(|reading| {
            let error_ms = wrapped_difference_ms(reading.observed_ms, reading.predicted_ms) as f64;
            CalibrationSample {
                video_s: reading.video_s,
                predicted_mach_ns: reading.predicted_ms * 1_000_000,
                observed_mach_ns: reading.observed_ms * 1_000_000,
                error_ms,
            }
        })
        .collect();

    let max_abs_error_ms = samples
        .iter()
        .map(|sample| sample.error_ms.abs())
        .fold(0.0, f64::max);

    Calibration {
        method: "on-screen binary clock strip decoded from extracted video frames".to_string(),
        passed: !samples.is_empty() && max_abs_error_ms <= tolerance_ms,
        max_abs_error_ms,
        tolerance_ms,
        samples,
    }
}

/// Decode the strip from one frame of one display file.
fn read_strip(
    session: &Session,
    file: &str,
    file_s: f64,
    filter: &str,
) -> eyre::Result<Option<u64>> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-ss", &format!("{file_s:.3}"), "-i"])
        .arg(session.path(file))
        .args(["-frames:v", "1", "-vf", filter])
        .args(["-pix_fmt", "gray", "-f", "rawvideo", "-"])
        .output()
        .wrap_err("could not run ffmpeg")?;

    if !output.status.success() {
        eyre::bail!(
            "ffmpeg failed at {}: {}",
            timecode::format(file_s),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(decode_cells(&output.stdout))
}

/// `[x, y, width, height, pixels_per_point]`, the window in points.
type WindowReport = [f64; 5];

fn spawn_clock() -> eyre::Result<(
    std::process::Child,
    crossbeam_channel::Receiver<WindowReport>,
)> {
    let mut child = Command::new(std::env::current_exe()?)
        .arg("selftest-clock")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err("could not start the clock window")?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre::Report::msg("the clock window has no stdout"))?;

    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let numbers: Vec<f64> = line
                .strip_prefix("RECT ")
                .map(|rest| {
                    rest.split_whitespace()
                        .filter_map(|n| n.parse().ok())
                        .collect()
                })
                .unwrap_or_default();

            match numbers[..] {
                [x, y, w, h, ppp] => {
                    let _ = tx.send([x, y, w, h, ppp]);
                }
                // an older clock window did not say
                [x, y, w, h] => {
                    let _ = tx.send([x, y, w, h, 1.0]);
                }
                _ => {}
            }
        }
    });

    Ok((child, rx))
}

/// Runs the self-test: records the screen, reads the clock strip out of the video, and compares
/// it with what the session's manifest predicts.
///
/// It starts the clock window as a child process ([`run_clock`], through the hidden
/// `selftest-clock` command of this same executable) on the main display, waits for it to say where
/// it is, and records that display alone for `args.seconds` seconds with no komorebi and no OS
/// trace. Then, for every half second of video from 1 s to half a second before the end, it asks
/// `ffmpeg` for that frame, reads the clock out of the strip, and compares it with the clock the
/// manifest predicts for that video second. A moment at which the strip cannot be read is not a
/// reading, and is noted in the manifest's warnings. The verdict ([`evaluate`]) is stored in the
/// manifest as `calibration`, a table of the errors is printed, and the session directory is
/// printed last.
///
/// The recording is kept: `args.out` is where it goes, and by default it is next to the other
/// recordings. What is printed is more than a pass or a fail: the error of each sample, and their
/// mean, the bias, which is reported apart from the worst error because the clock is painted
/// slightly before it is shown.
///
/// # Errors
///
/// Returns an error if
///
/// * `ffmpeg` is not on `PATH`;
/// * the OS has not let this process capture the screen (on macOS, the Screen Recording
///   permission), or the main display cannot be found;
/// * the clock window cannot be started, or does not report its position within 20 seconds;
/// * the recording fails, for any reason `record` would fail;
/// * no video was recorded for the main display;
/// * the clock cannot be found in any frame (is the clock window covered, or moved?): a frame is
///   saved under the session's `frames/` directory for a look;
/// * the manifest cannot be written;
/// * the test **fails**: the video disagrees with the clocks by more than `args.tolerance_ms`. The
///   verdict is in the manifest by then.
///
/// # Example
///
/// ```rust,no_run
/// use komorebi_recorder::cli::SelftestArgs;
/// use komorebi_recorder::selftest::run;
///
/// // record for 8 seconds, and allow the video to disagree with the clocks by up to 100 ms
/// run(&SelftestArgs { seconds: 8.0, tolerance_ms: 100.0, out: None, json: false })?;
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
///
/// This needs a screen, a window system and `ffmpeg`, so it is not run, and no automated test
/// covers it: `komorebi-recorder selftest` is the check.
pub fn run(args: &SelftestArgs) -> eyre::Result<()> {
    if !frames::ffmpeg_available() {
        eyre::bail!("the self-test decodes frames with ffmpeg, which was not found on PATH");
    }

    capture::ensure_permission()?;

    let display = capture::main_display()?;
    let main_id = display.id;

    eprintln!("starting the clock window on display {main_id}...");
    let (mut child, rect_rx) = spawn_clock()?;

    let first = rect_rx
        .recv_timeout(Duration::from_secs(20))
        .map_err(|_| eyre::Report::msg("the clock window never reported its position"))?;

    // let the window settle (and be shown) before trusting its position
    std::thread::sleep(Duration::from_millis(1500));
    let mut rect = first;
    while let Ok(latest) = rect_rx.try_recv() {
        rect = latest;
    }

    let options = RecordOptions {
        out: args.out.clone(),
        displays: vec![main_id],
        oslog: OsLogOptions {
            enabled: false,
            ..OsLogOptions::default()
        },
        duration: Some(Duration::from_secs_f64(args.seconds)),
        no_komorebi: true,
        ..RecordOptions::default()
    };

    eprintln!("recording {:.0}s...", args.seconds);
    let live = Live::new();
    let recorded = recorder::record(&options, &live);
    let _ = child.kill();
    let _ = child.wait();
    let dir = recorded?;

    let session = Session::open(&dir)?;
    let mut manifest = session.manifest()?;
    let entry = manifest
        .displays
        .iter()
        .find(|entry| entry.result.display.id == main_id && entry.result.frames > 0)
        .cloned()
        .ok_or_else(|| eyre::Report::msg("no video was recorded for the main display"))?;

    let (display_origin, video_per_unit) = strip_units(
        (display.frame[0], display.frame[1]),
        entry.result.scale,
        rect[4],
        cfg!(windows),
    );
    let filter = strip_filter((rect[0], rect[1]), display_origin, video_per_unit);

    let (_, duration) = manifest.video_span();
    let mut readings = Vec::new();
    let mut undecodable = Vec::new();
    let mut video_s = 1.0;
    while video_s <= duration - 0.5 {
        let file_s = video_s - entry.offset_s;
        let predicted_ms = manifest.mach_at_video_seconds(video_s) / 1_000_000 % WRAP_MS;
        match read_strip(&session, &entry.result.file, file_s, &filter)? {
            Some(observed_ms) => readings.push(Reading {
                video_s,
                predicted_ms,
                observed_ms,
            }),
            None => undecodable.push(video_s),
        }
        video_s += 0.5;
    }

    if readings.is_empty() {
        // leave a frame behind so the problem can be looked at
        let _ = frames::extract(&session, &manifest, &[1.0], Some(main_id));
        eyre::bail!(
            "could not find the clock in the video (is the clock window covered, or was it moved? \
             window reported at {rect:?}); a frame was saved under {}",
            session.path("frames").display()
        );
    }

    let calibration = evaluate(&readings, args.tolerance_ms);
    report(&calibration, &undecodable, &mut manifest, args.json);
    manifest.calibration = Some(calibration.clone());
    session.write_manifest(&manifest)?;
    if !args.json {
        println!("session: {}", dir.display());
    }

    if calibration.passed {
        if args.json {
            let samples = &calibration.samples;
            return crate::cli::print_json(
                &Envelope::ok(
                    SCHEMA_SELFTEST,
                    SelftestData {
                        session: dir.display().to_string(),
                        passed: true,
                        max_abs_error_ms: calibration.max_abs_error_ms,
                        mean_bias_ms: samples.iter().map(|sample| sample.error_ms).sum::<f64>()
                            / samples.len().max(1) as f64,
                        tolerance_ms: calibration.tolerance_ms,
                        samples: samples
                            .iter()
                            .map(|sample| SelftestSample {
                                video_s: sample.video_s,
                                error_ms: sample.error_ms,
                            })
                            .collect(),
                    },
                )
                .warnings(manifest.warnings.clone()),
            );
        }
        Ok(())
    } else {
        Err(eyre::Report::msg(format!(
            "sync self-test FAILED: the video disagrees with the clocks by up to {:.0} ms (tolerance {:.0} ms)",
            calibration.max_abs_error_ms, calibration.tolerance_ms
        )))
    }
}

fn report(calibration: &Calibration, undecodable: &[f64], manifest: &mut Manifest, json: bool) {
    if json {
        // the readings are in the envelope; only the manifest's note is kept
        if !undecodable.is_empty() {
            manifest.warnings.push(format!(
                "self-test could not read the clock at {} of {} sampled timestamps",
                undecodable.len(),
                undecodable.len() + calibration.samples.len()
            ));
        }
        return;
    }

    println!("video second | clock shown in video vs predicted");
    for sample in &calibration.samples {
        println!(
            "  {:>6} | {:+.0} ms",
            timecode::format(sample.video_s),
            sample.error_ms
        );
    }

    let mean = calibration
        .samples
        .iter()
        .map(|sample| sample.error_ms)
        .sum::<f64>()
        / calibration.samples.len().max(1) as f64;
    println!(
        "{} samples, mean bias {mean:+.1} ms (the clock is painted slightly before it is shown), \
         worst {:.0} ms, tolerance {:.0} ms: {}",
        calibration.samples.len(),
        calibration.max_abs_error_ms,
        calibration.tolerance_ms,
        if calibration.passed { "PASS" } else { "FAIL" }
    );

    if !undecodable.is_empty() {
        let note = format!(
            "self-test could not read the clock at {} of {} sampled timestamps",
            undecodable.len(),
            undecodable.len() + calibration.samples.len()
        );
        println!("note: {note}");
        manifest.warnings.push(note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(ms: u64) -> Vec<u8> {
        encode_cells(ms)
            .iter()
            .map(|white| if *white { 255 } else { 0 })
            .collect()
    }

    #[test]
    fn a_mac_window_and_display_share_units_and_a_windows_window_does_not() {
        // a Mac: both in points, the video 2 pixels to the point
        assert_eq!(
            strip_units((100.0, 50.0), 2.0, 2.0, false),
            ((100.0, 50.0), 2.0)
        );

        // Windows at 150 %: the display at 1920 px is 1280 points, and a point is 1.5 pixels
        let ((x, y), unit) = strip_units((1920.0, 0.0), 1.0, 1.5, true);
        assert_eq!((x, y, unit), (1280.0, 0.0, 1.5));

        // so a window 80 points into that display puts its strip 120 pixels + 20 points in
        let filter = strip_filter((1280.0 + 80.0, 80.0), (x, y), unit);
        assert!(filter.starts_with("crop=1440:45:150:"), "{filter}");
    }

    #[test]
    fn the_strip_roundtrips_any_clock_value() {
        for ms in [
            0,
            1,
            0xA5A5A5A5A5 & (WRAP_MS - 1),
            1_210_934_273,
            WRAP_MS - 1,
        ] {
            assert_eq!(decode_cells(&luminance(ms)), Some(ms), "{ms}");
        }
    }

    #[test]
    fn decoding_tolerates_antialiasing_but_not_a_missed_crop() {
        // slightly soft edges still decode
        let mut soft = luminance(0b1011_0010);
        for (index, level) in soft.iter_mut().enumerate() {
            *level = if *level == 255 {
                230 + (index % 5) as u8
            } else {
                10 + (index % 7) as u8
            };
        }
        assert_eq!(decode_cells(&soft), Some(0b1011_0010));

        // flat grey is a crop that missed the strip
        assert_eq!(decode_cells(&[128; CELLS]), None);
        assert_eq!(decode_cells(&[0; 10]), None, "wrong length");
    }

    #[test]
    fn wrap_around_differences_are_small() {
        assert_eq!(wrapped_difference_ms(1_050, 1_000), 50);
        assert_eq!(wrapped_difference_ms(950, 1_000), -50);
        // the strip wrapped between the prediction and the observation
        assert_eq!(wrapped_difference_ms(5, WRAP_MS - 5), 10);
        assert_eq!(wrapped_difference_ms(WRAP_MS - 5, 5), -10);
    }

    #[test]
    fn the_crop_filter_targets_the_strip_in_display_pixels() {
        // window at (80, 80) on a display at the origin, 1x
        assert_eq!(
            strip_filter((80.0, 80.0), (0.0, 0.0), 1.0),
            "crop=960:30:100:205,scale=40:1:flags=area"
        );
        // the same window on a display to the left, at 2x
        assert_eq!(
            strip_filter((-1720.0, 80.0), (-1800.0, 0.0), 2.0),
            "crop=1920:60:200:410,scale=40:1:flags=area"
        );
    }

    #[test]
    fn evaluation_passes_within_tolerance_and_reports_the_worst_error() {
        let readings = vec![
            Reading {
                video_s: 1.0,
                predicted_ms: 10_000,
                observed_ms: 9_970,
            },
            Reading {
                video_s: 1.5,
                predicted_ms: 10_500,
                observed_ms: 10_460,
            },
        ];
        let calibration = evaluate(&readings, 100.0);
        assert!(calibration.passed);
        assert!((calibration.max_abs_error_ms - 40.0).abs() < 1e-9);
        assert_eq!(calibration.samples[0].error_ms, -30.0);

        assert!(!evaluate(&readings, 35.0).passed);
        assert!(!evaluate(&[], 100.0).passed, "no readings is not a pass");
    }
}
