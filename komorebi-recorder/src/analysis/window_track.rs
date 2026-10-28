//! Reading the window track back: which window was where at a moment.
//!
//! A recording made with `--window-track` has `windows.jsonl` (see
//! [`crate::sources::windowtrack`]): rows that each list every window, written when something
//! changed and once a second regardless. [`Analysis::window_track`] reads it into a
//! [`WindowTrack`], and [`WindowTrack::at`] answers "the windows at this `mach_ns`" with the last
//! row at or before it. Because every row is complete there is nothing to replay. This is the one
//! entry point for anything that wants to know where the windows were in a frame: the `sheet
//! --windows` overlay uses it, and a later `--window` filter for `motion` or `blank` would too.
//!
//! A moment before the first row has no answer (`None`): the track had not started. A moment long
//! after the last row gets the last row, since the sampler writes a keyframe every second while it
//! runs; use [`WindowTrack::last_mach_ns`] to tell if that is stale.
//!
//! # Example
//!
//! ```rust
//! use komorebi_recorder::analysis::Analysis;
//! use komorebi_recorder::sources::windowtrack::WINDOWS_FILE;
//! use komorebi_recorder::testkit::{SessionBuilder, mach_ns};
//!
//! let (session, _) = SessionBuilder::new("doc-window-track")
//!     .window_track(&[
//!         (1.0, &[(7, "term", [0, 0, 500, 400])]),
//!         (2.0, &[(7, "term", [100, 0, 600, 400])]),
//!     ])
//!     .build();
//! assert!(session.path(WINDOWS_FILE).exists());
//!
//! let analysis = Analysis::open(&session.dir)?;
//! let track = analysis.window_track()?.expect("the session has a track");
//! assert!(track.at(mach_ns(0.5)).is_none());
//! assert_eq!(track.at(mach_ns(1.5)).unwrap().windows[0].rect[0], 0);
//! assert_eq!(track.at(mach_ns(2.5)).unwrap().windows[0].rect[0], 100);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::Analysis;
use crate::sources::windowtrack::TrackRow;
use crate::sources::windowtrack::WINDOWS_FILE;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;

/// The rows of `windows.jsonl`, in time order.
#[derive(Debug, Clone, Default)]
pub struct WindowTrack {
    rows: Vec<TrackRow>,
}

impl WindowTrack {
    /// Reads the text of `windows.jsonl`. A line that is not a row (a half-written last line of a
    /// recording that was killed) is skipped, and rows are sorted by time.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut rows: Vec<TrackRow> = text
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        rows.sort_by_key(|row| row.mach_ns);
        Self { rows }
    }

    /// Returns the last row at or before `mach_ns`: the windows as they were then. `None` before
    /// the first row.
    #[must_use]
    pub fn at(&self, mach_ns: u64) -> Option<&TrackRow> {
        let after = self.rows.partition_point(|row| row.mach_ns <= mach_ns);
        after.checked_sub(1).map(|index| &self.rows[index])
    }

    /// How many rows there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether there are no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// When the last row was written, on the master clock.
    #[must_use]
    pub fn last_mach_ns(&self) -> Option<u64> {
        self.rows.last().map(|row| row.mach_ns)
    }
}

impl Analysis {
    /// Reads the session's window track, or `None` if it has none (it was recorded without
    /// `--window-track`). The file is read each time this is called, not kept. A session whose
    /// recorder died still has what was flushed.
    ///
    /// # Errors
    ///
    /// The file exists and cannot be read.
    ///
    /// # Example
    ///
    /// See the [module docs](self).
    pub fn window_track(&self) -> eyre::Result<Option<WindowTrack>> {
        let path = self.guard().resolve(WINDOWS_FILE)?;
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .wrap_err_with(|| format!("could not read {}", path.display()))?;
        Ok(Some(WindowTrack::parse(&text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_row_at_or_before_a_moment_answers() {
        let track = WindowTrack::parse(
            "{\"mach_ns\":20,\"windows\":[]}\nnot a row\n{\"mach_ns\":10,\"windows\":[]}\n{\"mach_ns\":30",
        );
        assert_eq!(track.len(), 2);
        assert!(track.at(9).is_none());
        assert_eq!(track.at(10).unwrap().mach_ns, 10);
        assert_eq!(track.at(19).unwrap().mach_ns, 10);
        assert_eq!(track.at(u64::MAX).unwrap().mach_ns, 20);
        assert_eq!(track.last_mach_ns(), Some(20));
    }
}
