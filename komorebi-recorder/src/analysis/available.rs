//! What a recording has in it, and so which commands can answer anything about it.
//!
//! An investigator that has only the guide knows every command there is, not which of them this
//! recording can feed: `lag` is no use without a window track, `measure` is no use without video,
//! `trace --os` is no use without an OS trace. [`Analysis::available`] lists, for each kind of
//! data a recording can hold, whether this one has it, the commands that read it, and the limit of
//! what it says (the track of komorebi for KDE is komorebi's own state, a log at INFO has no
//! pushed times). `brief` and `overview` print it, and `--json` has it as `session.available`.
//!
//! It is read from the manifest and the recorded events, so it costs nothing to ask for.

use super::Analysis;
use crate::analysis::flavor::Flavor;
use schemars::JsonSchema;
use serde::Serialize;

/// One kind of data a recording can have, and what it makes possible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Available {
    /// What the data is: `events`, `log`, `video`, `window-track`, `window-probes` or `os-trace`.
    pub name: String,
    /// Whether this recording has it.
    pub present: bool,
    /// The commands that read it (command names, with the flags that need this data).
    pub commands: Vec<String>,
    /// The limit of what it says, or why it is missing, when there is one worth knowing.
    pub note: Option<String>,
}

fn entry(name: &str, present: bool, commands: &[&str], note: Option<String>) -> Available {
    Available {
        name: name.to_string(),
        present,
        commands: commands
            .iter()
            .map(|command| (*command).to_string())
            .collect(),
        note,
    }
}

impl Analysis {
    /// Returns what the recording has in it and the commands that read each part.
    ///
    /// Always the same six entries, in the same order, so a reader can rely on the position; a part the
    /// recording lacks has `present: false` and a `note` that says how to get it next time.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi_recorder::analysis::Analysis;
    /// use komorebi_recorder::testkit::SessionBuilder;
    ///
    /// let (session, _) = SessionBuilder::new("doc-available")
    ///     .event(1.0, serde_json::json!({"type": "Retile"}), None)
    ///     .build();
    /// let available = Analysis::open(&session.dir)?.available();
    /// let names: Vec<&str> = available.iter().map(|part| part.name.as_str()).collect();
    /// assert_eq!(
    ///     names,
    ///     ["events", "log", "video", "window-track", "window-probes", "os-trace"]
    /// );
    /// // nothing was recorded but the events, so the window track is missing and says why
    /// let track = &available[3];
    /// assert!(!track.present);
    /// assert!(track.note.as_deref().is_some_and(|note| note.contains("--window-track")));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn available(&self) -> Vec<Available> {
        let manifest = &self.manifest;
        let flavor = manifest.komorebi.flavor;
        let events = self.events();
        let level = manifest
            .komorebi
            .log_level
            .as_ref()
            .map(|level| level.level.to_uppercase());

        let log_lines = self.log().lines.len();
        let log_note = match level.as_deref() {
            _ if log_lines == 0 => Some(
                "komorebi's log was not recorded: `logq`, `code` and the pushed times have nothing to read".to_string(),
            ),
            Some("DEBUG" | "TRACE") => None,
            Some(level) => Some(format!(
                "komorebi logged at {level}: an event's komorebi time is its received time (`r`), since the pushed line is DEBUG, and handler times are lower bounds"
            )),
            None => Some(
                "the level komorebi logged at is unknown: pushed times (`p`) need DEBUG".to_string(),
            ),
        };

        let video = !manifest.displays.is_empty();
        let video_note = if !video {
            Some("no video was recorded: the picture commands have nothing to read".to_string())
        } else if !crate::av::supported() {
            Some(
                "this host cannot decode video: the picture commands need one that can".to_string(),
            )
        } else {
            None
        };

        let track = manifest.window_track.as_ref();
        let track_note = match track {
            None => Some(
                "not recorded: it is on by default on hosts that can list windows, so this was made with `--no-window-track` or on a host that cannot; record with `--window-track[=HZ]`".to_string(),
            ),
            Some(info) if info.error.is_some() => Some(format!(
                "the sampler stopped early: {}",
                info.error.as_deref().unwrap_or_default()
            )),
            Some(_) if flavor == Flavor::Kde => Some(
                "on KDE the track is komorebi's own state, not KWin's windows, so `lag` cannot say a window was off its layout rect"
                    .to_string(),
            ),
            Some(info) if info.late_ticks > 0 => Some(format!(
                "{} sample(s) were late: a gap in the track may be the sampler, not the windows",
                info.late_ticks
            )),
            Some(_) => None,
        };

        let probed = events.iter().any(|event| event.probe.is_some());
        let probe_note = if probed {
            None
        } else if flavor == Flavor::Macos {
            Some("komorebi for Mac has no window probe: a window's position is what says whether it is hidden".to_string())
        } else {
            Some(
                "no window was probed (`--no-probe-windows`, or nothing to probe): whether a window was really hidden is not knowable from komorebi's state alone".to_string(),
            )
        };

        let os_trace = self.has_os_trace();
        let os_note = (!os_trace).then(|| {
            "no OS trace was recorded: `trace --os` and `search --oslog` have nothing to read"
                .to_string()
        });

        vec![
            entry(
                "events",
                !events.is_empty(),
                &["events", "state", "diff", "trace", "window", "at", "range"],
                events
                    .is_empty()
                    .then(|| "no komorebi notification was recorded".to_string()),
            ),
            entry("log", log_lines > 0, &["logq", "code", "search"], log_note),
            entry(
                "video",
                video,
                &[
                    "sheet",
                    "changes",
                    "motion",
                    "blank",
                    "framediff",
                    "measure",
                    "compare",
                ],
                video_note,
            ),
            entry(
                "window-track",
                track.is_some(),
                &["lag", "sheet --windows", "findings (geometry-lag)"],
                track_note,
            ),
            entry(
                "window-probes",
                probed,
                &["trace (the probe of an event)", "findings (probed)"],
                probe_note,
            ),
            entry(
                "os-trace",
                os_trace,
                &["trace --os", "search --oslog"],
                os_note,
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::flavor::Flavor;
    use crate::testkit::SessionBuilder;
    use serde_json::json;

    fn part<'a>(available: &'a [Available], name: &str) -> &'a Available {
        available
            .iter()
            .find(|part| part.name == name)
            .expect("every part is always listed")
    }

    fn available_of(builder: SessionBuilder) -> Vec<Available> {
        let (session, _) = builder.build();
        Analysis::open(&session.dir).expect("open").available()
    }

    #[test]
    fn a_part_that_was_not_recorded_says_so_and_how_to_get_it() {
        let available = available_of(SessionBuilder::new("available-bare"));
        assert_eq!(available.len(), 6);
        for name in ["video", "window-track", "window-probes", "os-trace"] {
            let missing = part(&available, name);
            assert!(!missing.present, "{name}");
            assert!(missing.note.is_some(), "{name} has no note");
        }
        assert!(
            part(&available, "window-track")
                .note
                .as_deref()
                .is_some_and(|note| note.contains("--window-track"))
        );
    }

    #[test]
    fn a_part_that_was_recorded_is_present_with_its_commands() {
        let available = available_of(
            SessionBuilder::new("available-full")
                .event(1.0, json!({"type": "Retile"}), None)
                .window_track(&[(1.0, &[(7, "term", [0, 0, 100, 100])])])
                .os_record(2.0, "system", 1, "kernel", "something"),
        );
        assert!(part(&available, "events").present);
        assert!(part(&available, "events").note.is_none());
        let track = part(&available, "window-track");
        assert!(track.present);
        assert!(track.commands.iter().any(|command| command == "lag"));
        assert!(track.note.is_none());
        let os = part(&available, "os-trace");
        assert!(os.present);
        assert!(os.commands.iter().any(|command| command == "trace --os"));
    }

    #[test]
    fn on_kde_the_track_says_it_is_komorebis_own_state() {
        let available = available_of(
            SessionBuilder::new("available-kde")
                .flavor(Flavor::Kde)
                .window_track(&[(1.0, &[(7, "term", [0, 0, 100, 100])])]),
        );
        let track = part(&available, "window-track");
        assert!(track.present);
        assert!(
            track
                .note
                .as_deref()
                .is_some_and(|note| note.contains("komorebi's own state"))
        );
    }
}
