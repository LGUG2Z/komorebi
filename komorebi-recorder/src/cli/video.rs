//! `changes` and `sheet`: what the screen did.
//!
//! | Command | Asks | Answer, with `--json` |
//! |---|---|---|
//! | `changes` | [`Analysis::changes`](crate::analysis::Analysis::changes) | `changes/1` |
//! | `sheet` | [`Analysis::sheet`] | `sheet/1` |
//! | `motion` | [`Analysis::motion`] | `motion/1` |
//! | `framediff` | [`framediff`](crate::analysis::framediff::framediff) | `framediff/1` |
//! | `blank` | [`Analysis::blank`] | `blank/1` |
//! | `measure` | [`Analysis::measure`] | `measure/1` |
//! | `compare` | [`Analysis::compare`] | `compare/1` |
//!
//! Both work on a range of video time, given as `--from` and `--to` (any time `state --at` takes;
//! the default is the whole video) or, instead of those, as `--bug <id>`: a bug region from three
//! seconds before it starts to its end. Both work on the displays that have video (`--display`, repeatable, to pick),
//! and both refuse a session that recorded no video. `sheet` also takes `--at` for exact moments, or
//! `--count` moments evenly spaced across the range, and writes a PNG (by default to
//! `<session>/sheets/<range>.png`); see [`crate::analysis::sheet`] for the layout.
//!
//! Needs a host that can decode the video (macOS and Windows; see [`crate::av::supported`]).
//! Covered by `tests/video.rs`, which runs the commands through the real binary on a session with a
//! test video (it skips with a message when `ffmpeg` is not there to make one).

use super::print_json;
use crate::analysis::Analysis;
use crate::analysis::blank::DEFAULT_MIN_DURATION_S;
use crate::analysis::blank::DEFAULT_VARIANCE;
use crate::analysis::blank::SCHEMA_BLANK;
use crate::analysis::changes::SCHEMA_CHANGES;
use crate::analysis::compare::SCHEMA_COMPARE;
use crate::analysis::envelope;
use crate::analysis::envelope::Envelope;
use crate::analysis::framediff::DEFAULT_WIDTH;
use crate::analysis::framediff::FrameAt;
use crate::analysis::framediff::FrameDiffOptions;
use crate::analysis::framediff::SCHEMA_FRAMEDIFF;
use crate::analysis::measure::DEFAULT_HZ;
use crate::analysis::measure::MeasureOptions;
use crate::analysis::measure::SCHEMA_MEASURE;
use crate::analysis::measure::Statistic;
use crate::analysis::motion::SCHEMA_MOTION;
use crate::analysis::motion::ScanOptions;
use crate::analysis::pixels::Crop;
use crate::analysis::sheet::Region;
use crate::analysis::sheet::SCHEMA_SHEET;
use crate::analysis::sheet::SheetOptions;
use crate::analysis::video::SessionVideo;
use crate::annotations;
use crate::report;
use clap::Args;
use color_eyre::eyre;
use std::path::PathBuf;

/// Seconds shown before a bug region when a sheet is made of one.
const BUG_PRE_ROLL_S: f64 = 3.0;

/// The arguments of `changes`.
#[derive(Args, Debug)]
pub struct ChangesArgs {
    /// Session directory
    pub session: PathBuf,
    /// From this moment [default: the start of the video]
    #[arg(long, value_name = "TIME")]
    pub from: Option<String>,
    /// To this moment [default: the end of the video]
    #[arg(long, value_name = "TIME")]
    pub to: Option<String>,
    /// Instead: a bug region, from a few seconds before it to its end
    #[arg(long, value_name = "ID", conflicts_with_all = ["from", "to"])]
    pub bug: Option<String>,
    /// Only this display (repeatable) [default: every display with video]
    #[arg(long = "display", value_name = "ID")]
    pub displays: Vec<u32>,
    /// Above 1 notices smaller changes, below 1 only larger ones
    #[arg(long, default_value_t = 1.0)]
    pub sensitivity: f64,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `sheet`.
#[derive(Args, Debug)]
pub struct SheetArgs {
    /// Session directory
    pub session: PathBuf,
    /// From this moment [default: the start of the video]
    #[arg(long, value_name = "TIME")]
    pub from: Option<String>,
    /// To this moment [default: the end of the video]
    #[arg(long, value_name = "TIME")]
    pub to: Option<String>,
    /// Instead: a bug region, from a few seconds before it to its end
    #[arg(long, value_name = "ID", conflicts_with_all = ["from", "to"])]
    pub bug: Option<String>,
    /// Show exactly these moments instead of evenly spaced ones (repeatable)
    #[arg(long = "at", value_name = "TIME")]
    pub at: Vec<String>,
    /// How many frames, evenly spaced
    #[arg(long, default_value_t = 12)]
    pub count: usize,
    /// Only this display (repeatable) [default: every display with video]
    #[arg(long = "display", value_name = "ID")]
    pub displays: Vec<u32>,
    /// How wide each frame is drawn, in pixels
    #[arg(long, default_value_t = 480)]
    pub width: u32,
    /// Frames per row
    #[arg(long, default_value_t = 4)]
    pub columns: usize,
    /// Draw each window's rect on every frame, from the window track (`record --window-track`);
    /// a session without one says so and gets a plain sheet
    #[arg(long)]
    pub windows: bool,
    /// Where to write the PNG [default: in the session's sheets/ directory, named by the range]
    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `motion`.
#[derive(Args, Debug)]
pub struct MotionArgs {
    /// Session directory
    pub session: PathBuf,
    /// Only this display (repeatable) [default: every display with video]
    #[arg(long = "display", value_name = "ID")]
    pub displays: Vec<u32>,
    /// Only this part of the display, in display points: x,y,w,h
    #[arg(long, value_name = "X,Y,W,H")]
    pub region: Option<String>,
    /// From this moment [default: the start of the video]
    #[arg(long, value_name = "TIME")]
    pub from: Option<String>,
    /// To this moment [default: the end of the video]
    #[arg(long, value_name = "TIME")]
    pub to: Option<String>,
    /// Above 1 notices smaller changes, below 1 only larger ones
    #[arg(long, default_value_t = 1.0)]
    pub sensitivity: f64,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `framediff`.
#[derive(Args, Debug)]
pub struct FramediffArgs {
    /// Session directory
    pub session: PathBuf,
    /// The first moment
    #[arg(value_name = "TIME_A")]
    pub time_a: String,
    /// The second moment [default with --against: the same as TIME_A]
    #[arg(value_name = "TIME_B")]
    pub time_b: Option<String>,
    /// Compare with the frame of this other recording at TIME_B (resolved in that recording)
    #[arg(long, value_name = "SESSION")]
    pub against: Option<PathBuf>,
    /// The display [default: the first display with video]
    #[arg(long = "display", value_name = "ID")]
    pub display: Option<u32>,
    /// Only this part of the display, in display points: x,y,w,h
    #[arg(long, value_name = "X,Y,W,H")]
    pub region: Option<String>,
    /// Write a PNG of the compared area with the changed pixels in red
    #[arg(long, value_name = "FILE")]
    pub png: Option<PathBuf>,
    /// The width frames are decoded at, in pixels
    #[arg(long, default_value_t = DEFAULT_WIDTH)]
    pub width: u32,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `blank`.
#[derive(Args, Debug)]
pub struct BlankArgs {
    /// Session directory
    pub session: PathBuf,
    /// The region to watch, in display points: x,y,w,h
    #[arg(long, value_name = "X,Y,W,H")]
    pub region: String,
    /// Only this display (repeatable) [default: every display with video]
    #[arg(long = "display", value_name = "ID")]
    pub displays: Vec<u32>,
    /// From this moment [default: the start of the video]
    #[arg(long, value_name = "TIME")]
    pub from: Option<String>,
    /// To this moment [default: the end of the video]
    #[arg(long, value_name = "TIME")]
    pub to: Option<String>,
    /// A region is blank when the variance of its brightness is below this
    #[arg(long, default_value_t = DEFAULT_VARIANCE)]
    pub variance: f64,
    /// Drop stretches shorter than this many milliseconds
    #[arg(long, value_name = "MS", default_value_t = DEFAULT_MIN_DURATION_S * 1000.0)]
    pub min_ms: f64,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `measure`.
#[derive(Args, Debug)]
pub struct MeasureArgs {
    /// Session directory
    pub session: PathBuf,
    /// The region to measure, in display points: x,y,w,h [default: the whole display]
    #[arg(long, value_name = "X,Y,W,H")]
    pub region: Option<String>,
    /// What to measure, repeatable: mean (brightness), peak (brightness), width (drawn width, in
    /// points) [default: all three]
    #[arg(long = "stat", value_name = "STAT")]
    pub stats: Vec<Statistic>,
    /// Samples per second of the fixed-rate timeline
    #[arg(long, default_value_t = DEFAULT_HZ)]
    pub hz: f64,
    /// The brightness (0 to 255) that is background for `width` [default: the most common one in the region]
    #[arg(long, value_name = "0-255")]
    pub background: Option<u8>,
    /// Only this display (repeatable) [default: every display with video]
    #[arg(long = "display", value_name = "ID")]
    pub displays: Vec<u32>,
    /// From this moment [default: the start of the video]
    #[arg(long, value_name = "TIME")]
    pub from: Option<String>,
    /// To this moment [default: the end of the video]
    #[arg(long, value_name = "TIME")]
    pub to: Option<String>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The arguments of `compare`.
#[derive(Args, Debug)]
pub struct CompareArgs {
    /// The first session directory
    pub session_a: PathBuf,
    /// The second session directory
    pub session_b: PathBuf,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// The video-time range to work on: a bug region, or `--from`/`--to`, or everything.
fn range(
    analysis: &Analysis,
    from: Option<&str>,
    to: Option<&str>,
    bug: Option<&str>,
) -> eyre::Result<(f64, f64)> {
    if let Some(id) = bug {
        let loaded = annotations::load(&analysis.session.dir)?;
        let found = loaded
            .get(id)
            .ok_or_else(|| envelope::not_found(format!("there is no annotation {id}")))?;
        let end = found.end_or_session_end(&analysis.manifest);
        return Ok((
            (analysis.video_s(found.start_mach_ns) - BUG_PRE_ROLL_S).max(0.0),
            analysis.video_s(end),
        ));
    }

    let resolve = |text: Option<&str>, default: f64| -> eyre::Result<f64> {
        text.map_or(Ok(default), |text| {
            analysis.resolve(text).map(|resolved| resolved.video_s)
        })
    };
    let (_, end) = analysis.manifest.video_span();
    let (from, to) = (resolve(from, 0.0)?, resolve(to, end)?);
    if to < from {
        return Err(envelope::usage("the end of the range is before its start"));
    }

    Ok((from, to))
}

fn displays(video: &SessionVideo, wanted: &[u32]) -> eyre::Result<Vec<u32>> {
    let available = video.displays();
    if available.is_empty() {
        return Err(envelope::unusable("the session recorded no video"));
    }
    if wanted.is_empty() {
        return Ok(available);
    }

    for display in wanted {
        if !available.contains(display) {
            return Err(envelope::not_found(format!(
                "display {display} has no video; displays with video: {available:?}"
            )));
        }
    }
    Ok(wanted.to_vec())
}

pub(super) fn changes(args: &ChangesArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let video = SessionVideo::new(&analysis.manifest, analysis.guard());
    let (from, to) = range(
        &analysis,
        args.from.as_deref(),
        args.to.as_deref(),
        args.bug.as_deref(),
    )?;
    let shown = displays(&video, &args.displays)?;

    let data = analysis.changes(&video, &shown, from, to, args.sensitivity)?;
    if args.json {
        return print_json(&Envelope::ok(SCHEMA_CHANGES, data).warnings(analysis.warnings()));
    }

    print!("{}", report::changes_text(&data));
    Ok(())
}

pub(super) fn sheet(args: &SheetArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let video = SessionVideo::new(&analysis.manifest, analysis.guard());
    let (from, to) = range(
        &analysis,
        args.from.as_deref(),
        args.to.as_deref(),
        args.bug.as_deref(),
    )?;
    let shown = displays(&video, &args.displays)?;

    let times: Vec<f64> = if args.at.is_empty() {
        let count = args.count.max(1);
        if count == 1 {
            vec![from]
        } else {
            (0..count)
                .map(|index| from + (to - from) * index as f64 / (count - 1) as f64)
                .collect()
        }
    } else {
        args.at
            .iter()
            .map(|text| analysis.resolve(text).map(|resolved| resolved.video_s))
            .collect::<eyre::Result<_>>()?
    };

    let regions: Vec<Region> = annotations::load(&analysis.session.dir)?
        .bugs()
        .map(|bug| Region {
            id: bug.id.clone(),
            start_s: analysis.video_s(bug.start_mach_ns),
            end_s: analysis.video_s(bug.end_or_session_end(&analysis.manifest)),
            text: bug.text.clone(),
        })
        .collect();

    let out = args.out.clone().unwrap_or_else(|| {
        analysis.session.path(format!(
            "sheets/{}-{}.png",
            crate::timecode::format(times.iter().copied().fold(f64::INFINITY, f64::min))
                .replace(':', "-"),
            crate::timecode::format(times.iter().copied().fold(f64::NEG_INFINITY, f64::max))
                .replace(':', "-"),
        ))
    });

    let mut warnings = analysis.warnings();
    let windows = if args.windows {
        let track = analysis.window_track()?;
        if track.is_none() {
            warnings.push(
                "--windows was asked for but the session has no window track (it was recorded \
                 without --window-track), so no windows are drawn"
                    .to_string(),
            );
        }
        track
    } else {
        None
    };

    let data = analysis.sheet(
        &video,
        &SheetOptions {
            times,
            displays: shown,
            cell_width: args.width,
            columns: args.columns,
            regions,
            windows,
            out,
        },
    )?;
    if args.json {
        return print_json(&Envelope::ok(SCHEMA_SHEET, data).warnings(warnings));
    }

    for warning in &warnings {
        eprintln!("note: {warning}");
    }
    print!("{}", report::sheet_text(&data));
    Ok(())
}

fn region(text: Option<&str>) -> eyre::Result<Option<Crop>> {
    text.map(Crop::parse).transpose()
}

pub(super) fn motion(args: &MotionArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let video = SessionVideo::new(&analysis.manifest, analysis.guard());
    let (from, to) = range(&analysis, args.from.as_deref(), args.to.as_deref(), None)?;
    let shown = displays(&video, &args.displays)?;

    let data = analysis.motion(
        &video,
        &ScanOptions {
            displays: &shown,
            from_s: from,
            to_s: to,
            crop: region(args.region.as_deref())?,
            cancel: None,
        },
        args.sensitivity,
    )?;
    if args.json {
        return print_json(&Envelope::ok(SCHEMA_MOTION, data).warnings(analysis.warnings()));
    }

    print!("{}", data.to_text());
    Ok(())
}

pub(super) fn framediff(args: &FramediffArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let video = SessionVideo::new(&analysis.manifest, analysis.guard());
    let wanted: Vec<u32> = args.display.into_iter().collect();
    let display = displays(&video, &wanted)?[0];
    let options = FrameDiffOptions {
        crop: region(args.region.as_deref())?,
        width: args.width,
        png: args.png.as_deref(),
    };
    let first = FrameAt {
        analysis: &analysis,
        video: &video,
        display,
        time_s: analysis.resolve(&args.time_a)?.video_s,
    };

    let data = if let Some(path) = &args.against {
        let other = Analysis::open(path)?;
        let other_video = SessionVideo::new(&other.manifest, other.guard());
        let available = other_video.displays();
        let other_display = if available.contains(&display) {
            display
        } else if let [only] = available[..] {
            only
        } else {
            return Err(envelope::not_found(format!(
                "{} has no display {display}; displays with video: {available:?}",
                path.display()
            )));
        };
        let time_b = other
            .resolve(args.time_b.as_deref().unwrap_or(&args.time_a))?
            .video_s;
        crate::analysis::framediff::framediff(
            &first,
            &FrameAt {
                analysis: &other,
                video: &other_video,
                display: other_display,
                time_s: time_b,
            },
            &options,
        )?
    } else {
        let text = args.time_b.as_deref().ok_or_else(|| {
            envelope::usage("framediff needs a second time, or --against another session")
        })?;
        let time_b = analysis.resolve(text)?.video_s;
        crate::analysis::framediff::framediff(
            &first,
            &FrameAt {
                time_s: time_b,
                ..first
            },
            &options,
        )?
    };
    if args.json {
        return print_json(&Envelope::ok(SCHEMA_FRAMEDIFF, data).warnings(analysis.warnings()));
    }

    print!("{}", data.to_text());
    Ok(())
}

pub(super) fn blank(args: &BlankArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let video = SessionVideo::new(&analysis.manifest, analysis.guard());
    let (from, to) = range(&analysis, args.from.as_deref(), args.to.as_deref(), None)?;
    let shown = displays(&video, &args.displays)?;

    let data = analysis.blank(
        &video,
        &ScanOptions {
            displays: &shown,
            from_s: from,
            to_s: to,
            crop: region(Some(&args.region))?,
            cancel: None,
        },
        args.variance,
        args.min_ms / 1000.0,
    )?;
    if args.json {
        return print_json(&Envelope::ok(SCHEMA_BLANK, data).warnings(analysis.warnings()));
    }

    print!("{}", data.to_text());
    Ok(())
}

pub(super) fn measure(args: &MeasureArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let video = SessionVideo::new(&analysis.manifest, analysis.guard());
    let (from, to) = range(&analysis, args.from.as_deref(), args.to.as_deref(), None)?;
    let shown = displays(&video, &args.displays)?;
    let mut statistics = if args.stats.is_empty() {
        Statistic::ALL.to_vec()
    } else {
        args.stats.clone()
    };
    statistics.dedup();

    let data = analysis.measure(
        &video,
        &MeasureOptions {
            scan: ScanOptions {
                displays: &shown,
                from_s: from,
                to_s: to,
                crop: region(args.region.as_deref())?,
                cancel: None,
            },
            statistics: &statistics,
            hz: args.hz,
            background: args.background,
        },
    )?;
    if args.json {
        return print_json(&Envelope::ok(SCHEMA_MEASURE, data).warnings(analysis.warnings()));
    }

    print!("{}", data.to_text());
    Ok(())
}

pub(super) fn compare(args: &CompareArgs) -> eyre::Result<()> {
    let a = Analysis::open(&args.session_a)?;
    let b = Analysis::open(&args.session_b)?;

    let data = a.compare(&b, None)?;
    if args.json {
        let mut warnings = a.warnings();
        warnings.extend(b.warnings());
        return print_json(&Envelope::ok(SCHEMA_COMPARE, data).warnings(warnings));
    }

    print!("{}", data.to_text());
    Ok(())
}
