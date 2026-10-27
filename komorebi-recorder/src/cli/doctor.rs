//! `doctor`: what this machine can do for a recording, and probes that measure what a recording
//! leans on.
//!
//! With no `--probe`, `doctor` describes the machine ([`crate::doctor::report`]) and prints it as
//! text, or with `--json` as the `doctor/1` envelope whose warnings are the report's. With
//! `--probe`, it runs one measurement and prints what it found, as text or as the `probe/1`
//! envelope:
//!
//! | `--probe` | Measures | Takes | Runs |
//! |---|---|---|---|
//! | `clock` | whether the master clock keeps counting while the machine sleeps | `--seconds` | [`crate::doctor::probe_clock`] |
//! | `capture` | what the capture pipeline produces from the screen alone | `--seconds`, `--out` | [`crate::doctor::probe_capture`] |
//! | `decode` | first-frame time, speed and exactness of seeking of a recorded video | `--file` (required) | [`crate::doctor::probe_decode`] |
//! | `window` | what komorebi for Windows says about whether it manages a window | `--hwnd` (required) | [`crate::doctor::probe_window`] |
//!
//! A probe that needs an argument and is not given it fails with a usage error. The clock and
//! capture probes print to standard error what they are about to do before they start.
//!
//! The probes need the outside world (sleep, a screen, a video file, komorebi), so they have no
//! example.

use super::print_json;
use crate::analysis::envelope::Envelope;
use crate::doctor;
use crate::doctor::ProbeReport;
use clap::Args;
use clap::ValueEnum;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use std::path::PathBuf;
use std::time::Duration;

/// A measurement `doctor --probe` can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Probe {
    /// Whether the master clock keeps counting while the machine sleeps (put it to sleep and wake
    /// it while this runs)
    Clock,
    /// Record the screen alone for a while and report what the capture pipeline produced
    Capture,
    /// Decode a recorded video: first-frame time, speed and whether seeking is exact
    Decode,
    /// Ask komorebi (for Windows) whether it would manage a window, and why
    Window,
}

/// The arguments of `doctor`.
#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Measure one thing instead of describing the machine
    #[arg(long, value_enum)]
    pub probe: Option<Probe>,
    /// How long the clock and capture probes run
    #[arg(long, default_value_t = 60.0, value_name = "SECONDS")]
    pub seconds: f64,
    /// The video file for the decode probe
    #[arg(long, value_name = "FILE")]
    pub file: Option<PathBuf>,
    /// The window handle for the window probe
    #[arg(long, value_name = "HWND")]
    pub hwnd: Option<u32>,
    /// Where the capture probe puts its recording [default: next to the others]
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Machine-readable JSON output
    #[arg(long)]
    pub json: bool,
}

pub(super) fn doctor(args: &DoctorArgs) -> eyre::Result<()> {
    let Some(kind) = args.probe else {
        let report = doctor::report();
        let warnings = report.warnings.clone();
        if args.json {
            return print_json(&Envelope::ok(doctor::SCHEMA_DOCTOR, report).warnings(warnings));
        }

        print!("{}", doctor::text(&report));
        return Ok(());
    };

    let probe = match kind {
        Probe::Clock => {
            eprintln!(
                "reading the clocks for {:.0} s: put the machine to sleep and wake it while this \
                 runs to learn what the master clock does then",
                args.seconds
            );
            ProbeReport::Clock(doctor::probe_clock(
                args.seconds,
                Duration::from_millis(250),
            ))
        }
        Probe::Capture => {
            eprintln!(
                "recording the screen for {:.0} s: leave it still for part of that",
                args.seconds
            );
            ProbeReport::Capture(doctor::probe_capture(args.seconds, args.out.clone())?)
        }
        Probe::Decode => {
            let file = args
                .file
                .as_ref()
                .ok_or_else(|| crate::analysis::envelope::usage("--probe decode needs --file"))?;
            ProbeReport::Decode(
                doctor::probe_decode(file)
                    .wrap_err_with(|| format!("could not decode {}", file.display()))?,
            )
        }
        Probe::Window => {
            let hwnd = args
                .hwnd
                .ok_or_else(|| crate::analysis::envelope::usage("--probe window needs --hwnd"))?;
            ProbeReport::Window(doctor::probe_window(hwnd)?)
        }
    };

    if args.json {
        return print_json(&Envelope::ok(doctor::SCHEMA_PROBE, probe));
    }

    print!("{}", doctor::probe_text(&probe));
    Ok(())
}
