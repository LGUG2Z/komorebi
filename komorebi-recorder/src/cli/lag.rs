//! `lag`: how long, and by how much, windows were off the rect komorebi's layout gave them.
//!
//! The answer is [`Analysis::geometry_lag`] (`lag/1` with `--json`); see [`crate::analysis::lag`]
//! for what is measured, what is not flagged and why KDE has no answer. `--tolerance` and
//! `--grace-ms` change what counts as off, and `--window` narrows it to one window.
//!
//! Covered by the tests of the analysis and `tests/lag.rs`, which runs the command through the
//! real binary.

use super::print_json;
use crate::analysis::Analysis;
use crate::analysis::envelope::Envelope;
use crate::analysis::lag::DEFAULT_GRACE_MS;
use crate::analysis::lag::DEFAULT_TOLERANCE;
use crate::analysis::lag::LagData;
use crate::analysis::lag::LagOptions;
use crate::analysis::lag::SCHEMA_LAG;
use crate::timecode;
use clap::Args;
use color_eyre::eyre;
use std::fmt::Write;
use std::path::PathBuf;

/// The arguments of `lag`.
#[derive(Args, Debug)]
pub struct LagArgs {
    /// Session directory
    pub session: PathBuf,
    /// Only this window (its id)
    #[arg(long, value_name = "ID")]
    pub window: Option<u64>,
    /// The most an edge may be off, in the track's units (pixels, or points on a Mac), before the
    /// window counts as off
    #[arg(long, value_name = "N", default_value_t = DEFAULT_TOLERANCE)]
    pub tolerance: i64,
    /// How long after a layout change nothing is flagged, in milliseconds: komorebi's animations
    /// move a window to its new rect over a fraction of a second
    #[arg(long, value_name = "MS", default_value_t = DEFAULT_GRACE_MS)]
    pub grace_ms: u64,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

/// Renders the answer as text, one episode to a line.
#[must_use]
pub fn lag_text(data: &LagData) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} episode(s) of geometry lag (tolerance {} {}, grace {} ms)",
        data.episodes.len(),
        data.tolerance,
        data.unit,
        data.grace_ms
    );
    if let Some(note) = &data.note {
        let _ = writeln!(out, "  note: {note}");
    }
    if data.track_available && data.independent && !data.borders_seen {
        let _ = writeln!(
            out,
            "  the track has no border windows, so which of border and window lagged is unknown"
        );
    }
    for episode in &data.episodes {
        let _ = writeln!(
            out,
            "  {} to {}: {}",
            timecode::format(episode.start_video_s),
            timecode::format(episode.end_video_s),
            episode.describe(&data.unit)
        );
    }
    out
}

pub(super) fn lag(args: &LagArgs) -> eyre::Result<()> {
    let analysis = Analysis::open(&args.session)?;
    let data = analysis.geometry_lag(&LagOptions {
        tolerance: args.tolerance,
        grace_ms: args.grace_ms,
        window: args.window,
    })?;

    if args.json {
        return print_json(&Envelope::ok(SCHEMA_LAG, data).warnings(analysis.warnings()));
    }
    print!("{}", lag_text(&data));
    Ok(())
}
