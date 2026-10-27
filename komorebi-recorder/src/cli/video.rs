//! `changes` and `sheet`: what the screen did.
//!
//! | Command | Asks | Answer, with `--json` |
//! |---|---|---|
//! | `changes` | [`Analysis::changes`](crate::analysis::Analysis::changes) | `changes/1` |
//! | `sheet` | [`Analysis::sheet`] | `sheet/1` |
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
use crate::analysis::changes::SCHEMA_CHANGES;
use crate::analysis::envelope;
use crate::analysis::envelope::Envelope;
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
    /// Where to write the PNG [default: in the session's sheets/ directory, named by the range]
    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,
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

    let data = analysis.sheet(
        &video,
        &SheetOptions {
            times,
            displays: shown,
            cell_width: args.width,
            columns: args.columns,
            regions,
            out,
        },
    )?;
    if args.json {
        return print_json(&Envelope::ok(SCHEMA_SHEET, data).warnings(analysis.warnings()));
    }

    print!("{}", report::sheet_text(&data));
    Ok(())
}
