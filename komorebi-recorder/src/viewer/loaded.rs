//! A session opened in the viewer: everything the window draws, gathered once off the UI thread.
//!
//! [`Loaded::load`] opens the session through [`Analysis`] and turns what it reads into the plain
//! lists the window draws every frame (events, incidents, findings, log marks, displays and
//! annotations), all of them already on the session's video time. Reading takes noticeable time
//! on a long recording, which is why [`ViewerApp::open`](super::ViewerApp::open) does it on a
//! thread. A [`Loaded`] does not change after it is made, except that
//! [`Loaded::reload_annotations`] reads the annotations again after the window changed them.
//!
//! Which komorebi produced the session (the flavor) and which host recorded it are already
//! decided by [`Analysis`]; nothing here depends on either.

use crate::analysis::Analysis;
use crate::analysis::catalog::Skew;
use crate::analysis::ghost::GhostTile;
use crate::analysis::logparse::Level;
use crate::analysis::overview::Finding;
use crate::analysis::overview::IncidentView;
use crate::analysis::views::EventView;
use crate::annotations;
use crate::annotations::AnnotationView;
use color_eyre::eyre;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

/// A display and the stretches of the recording that have video from it.
///
/// A display that was unplugged or slept and came back has more than one stretch: one `DisplayMeta`
/// per display, however many segments of video it has.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayMeta {
    /// The display's id, as the recorder and the manifest name it.
    pub id: u32,
    /// `[x, y, width, height]` in global points: where its picture sits among the monitors.
    pub frame: [f64; 4],
    /// The stretches with video from this display, as `(start, end)` in session video seconds,
    /// one for each segment of its video in the order the manifest lists them. A segment with no
    /// recorded end runs to the end of the recording.
    pub segments: Vec<(f64, f64)>,
}

/// An error or warning in komorebi's log, placed on the timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogMark {
    /// When the line was written, in session video seconds.
    pub video_s: f64,
    /// [`Level::Error`] or [`Level::Warn`]: no other level is marked.
    pub level: Level,
}

/// A session opened in the viewer: everything the window draws, read once.
///
/// All times are session video seconds, the same timeline as
/// [`Transport`](crate::playback::Transport): seconds from the start of the recording's video. A
/// line of komorebi's log that the session's clock cannot place on it has no [`LogMark`] and a
/// `NaN` in [`Loaded::log_times`].
///
/// A `Loaded` is made by [`Loaded::load`], which needs a finished session on disk, so it has no
/// example here: `a_session_loads_with_everything_the_window_draws` in this module's tests builds
/// one.
pub struct Loaded {
    /// The session, analyzed. Shared with the background threads that read more of it (the scan for
    /// changes in the picture).
    pub analysis: Arc<Analysis>,
    /// The session directory, as it was given to [`Loaded::load`].
    pub dir: PathBuf,
    /// The length of the video in session video seconds: the end of
    /// [`Manifest::video_span`](crate::session::Manifest::video_span).
    pub duration_s: f64,
    /// komorebi's events as a reader sees them, in the order they were recorded.
    pub events: Vec<EventView>,
    /// Video time of each event, for finding the one nearest a point. It has one entry for each of
    /// `events`, in the same order, so the two can be indexed together.
    pub event_times: Vec<f64>,
    /// Incidents in the recording environment (a display went away, the machine slept...).
    pub incidents: Vec<IncidentView>,
    /// Ranked, most worth looking at first.
    pub findings: Vec<Finding>,
    /// The errors and warnings in komorebi's log that have a time, for the log lane.
    pub log_marks: Vec<LogMark>,
    /// Video time of each line of komorebi's log (NaN where a line has no time). It has one entry
    /// for each line of the log, in order.
    pub log_times: Vec<f64>,
    /// The displays that have video, in ascending order of id.
    pub displays: Vec<DisplayMeta>,
    /// Bug regions, notes and findings, in the order of their start.
    pub annotations: Vec<AnnotationView>,
    /// Things the analysis noticed that a reader should be told, and the warnings from reading the
    /// annotations file.
    pub warnings: Vec<String>,
    /// How the komorebi source this build indexed compares with the komorebi that was recorded.
    pub skew: Skew,
    /// The ghost tiles the recording shows: tiles that held a window a person could not see.
    /// Empty for a recording of komorebi for Windows made without window probes
    /// (`--no-probe-windows`).
    pub ghosts: Vec<GhostTile>,
}

impl Loaded {
    /// Opens the session in `dir` and reads everything the window needs.
    ///
    /// This takes noticeable time on a long recording: call it off the UI thread.
    ///
    /// # Errors
    ///
    /// Returns an error if
    ///
    /// - `dir` does not exist or is not a session directory (it has no session manifest),
    /// - the manifest cannot be read, is not valid, or was written by a newer komorebi-recorder
    ///   than this build,
    /// - the session is still being recorded ("is still being recorded; stop the recording first"),
    /// - the session was left unfinished because its recorder was killed or crashed (the error
    ///   says to run `komorebi-recorder reindex` on it), or
    /// - the annotations file cannot be read, or the bug regions or the findings cannot be worked
    ///   out (a file they are read from cannot be read).
    ///
    /// Everything else that can be wrong with a session (a missing log, an event that cannot be
    /// read) is not an error: it becomes an entry of [`Loaded::warnings`].
    ///
    /// Covered by `a_session_loads_with_everything_the_window_draws` and
    /// `a_session_that_cannot_be_analyzed_says_why`.
    pub fn load(dir: &Path) -> eyre::Result<Self> {
        let analysis = Arc::new(Analysis::open(dir)?);
        let duration_s = analysis.manifest.video_span().1;

        let events: Vec<EventView> = (0..analysis.events().len())
            .filter_map(|index| analysis.event_view(index))
            .collect();
        let event_times = events.iter().map(|event| event.video_s).collect();

        let bugs = analysis.bugs()?;
        let bug_refs: Vec<&annotations::Annotation> = bugs.iter().collect();
        let findings = analysis.rank(analysis.findings()?, &bug_refs);

        let log_marks = analysis
            .log()
            .lines
            .iter()
            .filter_map(|line| {
                let level = line
                    .level
                    .filter(|level| matches!(level, Level::Error | Level::Warn))?;
                let mach_ns = analysis.clock().wall_to_mach(line.wall_us?)?;
                Some(LogMark {
                    video_s: analysis.video_s(mach_ns),
                    level,
                })
            })
            .collect();

        let log_times: Vec<f64> = analysis
            .log()
            .lines
            .iter()
            .map(|line| {
                line.wall_us
                    .and_then(|wall_us| analysis.clock().wall_to_mach(wall_us))
                    .map_or(f64::NAN, |mach_ns| analysis.video_s(mach_ns))
            })
            .collect();

        let mut displays: Vec<DisplayMeta> = Vec::new();
        for entry in analysis
            .manifest
            .displays
            .iter()
            .filter(|entry| entry.result.frames > 0)
        {
            let id = entry.result.display.id;
            let end = entry.end_s.unwrap_or(duration_s);
            match displays.iter_mut().find(|meta| meta.id == id) {
                Some(meta) => meta.segments.push((entry.offset_s, end)),
                None => displays.push(DisplayMeta {
                    id,
                    frame: entry.result.display.frame,
                    segments: vec![(entry.offset_s, end)],
                }),
            }
        }
        displays.sort_by_key(|meta| meta.id);

        let loaded_annotations = annotations::load(dir)?;
        let mut warnings = analysis.warnings();
        warnings.extend(loaded_annotations.warnings.clone());
        let skew = analysis.skew();
        let ghosts = analysis.ghost_tiles().unwrap_or_else(|error| {
            warnings.push(format!("could not look for ghost tiles: {error:#}"));
            Vec::new()
        });

        Ok(Self {
            incidents: analysis.incidents(),
            annotations: loaded_annotations
                .annotations
                .iter()
                .map(|annotation| annotation.view(&analysis.manifest))
                .collect(),
            analysis,
            dir: dir.to_path_buf(),
            duration_s,
            events,
            event_times,
            findings,
            log_marks,
            log_times,
            displays,
            warnings,
            skew,
            ghosts,
        })
    }

    /// Reads the annotations again, after one was added, changed or removed.
    ///
    /// # Errors
    ///
    /// Returns an error if the annotations file cannot be read. [`Loaded::annotations`] is then
    /// unchanged.
    pub fn reload_annotations(&mut self) -> eyre::Result<()> {
        let loaded = annotations::load(&self.dir)?;
        self.annotations = loaded
            .annotations
            .iter()
            .map(|annotation| annotation.view(&self.analysis.manifest))
            .collect();
        Ok(())
    }

    /// Returns the bug regions (the annotations of kind [`Kind::Bug`](annotations::Kind::Bug)), in
    /// time order.
    pub fn bugs(&self) -> impl Iterator<Item = &AnnotationView> {
        self.annotations
            .iter()
            .filter(|annotation| annotation.kind == annotations::Kind::Bug)
    }

    /// Returns the event with sequence number `seq`, if there is one.
    ///
    /// The search is linear in the number of events.
    #[must_use]
    pub fn event(&self, seq: u64) -> Option<&EventView> {
        self.events.iter().find(|event| event.seq == seq)
    }
}

/// Returns a name for a session in a list: the name of the folder it is in.
///
/// Sessions are folders named for when they began, so the name is a timestamp. A path with no final
/// name (a root) is shown whole.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::viewer::loaded::name_of;
/// use std::path::Path;
///
/// assert_eq!(name_of(Path::new("/recordings/20261026T120000Z")), "20261026T120000Z");
/// assert_eq!(name_of(Path::new("/")), "/");
/// ```
#[must_use]
pub fn name_of(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || dir.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::SessionBuilder;
    use serde_json::json;

    #[test]
    fn a_session_loads_with_everything_the_window_draws() {
        let (session, _) = SessionBuilder::new("viewer-loaded")
            .event(
                2.0,
                json!({"type": "Retile"}),
                Some(crate::testkit::state_with_windows(&[(1, "a")])),
            )
            .log(
                2.0,
                "ERROR",
                "komorebi::macos_api: failed to get window position: AXError::InvalidUIElement",
            )
            .incident(3.0, "display-disconnected", "display 3 went away")
            .build();

        let loaded = Loaded::load(&session.dir).expect("load");
        assert_eq!(loaded.events.len(), 1);
        assert_eq!(loaded.event_times.len(), 1);
        assert!((loaded.event_times[0] - 2.0).abs() < 1e-6);
        assert_eq!(loaded.incidents.len(), 1);
        assert_eq!(loaded.log_marks.len(), 1);
        assert_eq!(loaded.log_marks[0].level, Level::Error);
        assert_eq!(loaded.log_times.len(), 1);
        assert!((loaded.log_times[0] - 2.0).abs() < 1e-6);
        assert!((loaded.duration_s - 10.0).abs() < 1e-6);
        assert!(loaded.displays.is_empty(), "no video was recorded");
        assert!(!loaded.findings.is_empty());
    }

    #[test]
    fn annotations_are_read_again_after_a_change() {
        let (session, manifest) = SessionBuilder::new("viewer-reload").build();
        let mut loaded = Loaded::load(&session.dir).expect("load");
        assert_eq!(loaded.bugs().count(), 0);

        annotations::add(
            &session.dir,
            annotations::New {
                kind: Some(annotations::Kind::Bug),
                start_mach_ns: manifest.mach_at_video_seconds(1.0),
                end_mach_ns: Some(manifest.mach_at_video_seconds(2.0)),
                text: "wrong".to_string(),
                ..annotations::New::default()
            },
        )
        .expect("add");

        loaded.reload_annotations().expect("reload");
        assert_eq!(loaded.bugs().count(), 1);
        assert!((loaded.bugs().next().expect("bug").start_video_s - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_session_that_cannot_be_analyzed_says_why() {
        let (session, mut manifest) = SessionBuilder::new("viewer-live").build();
        manifest.status = "recording".to_string();
        session.write_manifest(&manifest).expect("manifest");

        let error = Loaded::load(&session.dir)
            .err()
            .expect("refused")
            .to_string();
        assert!(
            error.contains("left unfinished") || error.contains("still being recorded"),
            "{error}"
        );
        assert!(Loaded::load(&session.dir.join("nope")).is_err());
    }

    #[test]
    fn a_session_is_named_for_its_folder() {
        assert_eq!(
            name_of(Path::new("/a/b/20261026T000000Z")),
            "20261026T000000Z"
        );
    }
}
