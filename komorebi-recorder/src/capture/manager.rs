//! Keeps every wanted display recording while displays come and go.
//!
//! A laptop that is docked and undocked, woken with the lid opened, or locked and unlocked
//! changes its set of displays and kills `ScreenCaptureKit` streams at unpredictable moments. The
//! rule here is that *nothing already captured is ever put at risk by that*:
//!
//! - every recording is a **segment**: one display, one file, one contiguous span. When a display
//!   disappears or its stream dies, that segment is finalized immediately (so it is complete and
//!   playable no matter what happens next) and the other displays keep recording untouched;
//! - when a display (re)appears it starts a **new segment** in a new file, after a short settle
//!   delay so a display that is still waking up is not started half-formed;
//! - a start that fails (the display is not ready, the screen is locked) is retried with backoff
//!   and reported once, not every attempt;
//! - an unreadable display list changes nothing, since "I could not ask" is not "they all left".
//!
//! Time is passed in rather than read, so the whole policy is testable with a scripted backend.
//!
//! # What ends a capture
//!
//! [`DisplayManager::poll`] looks at every running capture and applies the first rule that
//! matches, in this order:
//!
//! | situation | [`EndReason`] | incident kind ([`kind`]) | what happens next |
//! |---|---|---|---|
//! | the capture reported an error | `StreamError`, ended when the error was reported | `capture-stopped` | a new segment after a one second backoff, if the display is still present and awake |
//! | the display is missing from the display list | `DisplayDisconnected` | `display-disconnected` | a new segment when the display returns, after the settle delay |
//! | the display is asleep | `DisplayAsleep` | `display-asleep` | a new segment when it wakes, after the settle delay |
//! | no sign of life for longer than four seconds | `StreamError`, ended when it last showed life | `capture-stopped` | as for an error |
//! | the display's size changed | `Reconfigured` | `display-reconfigured` | a new segment in the same poll |
//!
//! The order matters when several apply at once: a display that vanished is a disconnect even if
//! its stream had also gone quiet. The stall rule applies only to a capture that reports a sign of
//! life ([`RunningCapture::last_signal_mach_ns`]); the macOS backend does, and the Windows backend
//! does not, so a Windows capture is never replaced for silence.
//!
//! [`DisplayManager::restart_all`] is the one way a capture ends that is not found by looking: the
//! caller knows the machine slept and asks for every capture to be replaced.
//!
//! # Timing
//!
//! | what | how long |
//! |---|---|
//! | settle: a display that appeared after recording began must be present this long before it is started, so that one still waking up is not started half-formed | one second |
//! | stall: a capture with no sign of life, not even an idle callback, for this long has stalled | four seconds |
//! | retry backoff after a failed start or a dead stream | one second, doubling up to ten seconds |
//!
//! The first display list of a recording is not delayed ([`DisplayManager::start`]).
//!
//! # What this does not do
//!
//! A display's consecutive segments are not guaranteed to be gap-free: after a disconnect, a sleep
//! or a dead stream, the settle delay and the backoff are time in which that display is not
//! recorded (a resolution change is the exception: the next segment starts in the same poll).
//! What is guaranteed is that what was captured before is complete. The manager does not poll by
//! itself: the caller calls [`DisplayManager::poll`] several times a second and records the
//! returned [`Draft`]s.

use crate::capture::CaptureBackend;
use crate::capture::CaptureConfig;
use crate::capture::DisplayDescriptor;
use crate::capture::DisplayResult;
use crate::capture::EndReason;
use crate::capture::RunningCapture;
use crate::incidents::Draft;
use crate::incidents::kind;
use color_eyre::eyre;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// How long a display that appeared after recording began must be present before it is started.
const SETTLE_NS: u64 = 1_000_000_000;
/// A capture that shows no sign of life (not even an idle callback, which a healthy stream makes
/// about thirty times a second even on a static screen) for this long has stalled.
const STALL_NS: u64 = 4_000_000_000;
/// The first retry after a stream dies or a start fails, doubling up to [`MAX_BACKOFF_NS`].
const FIRST_BACKOFF_NS: u64 = 1_000_000_000;
const MAX_BACKOFF_NS: u64 = 10_000_000_000;

/// Which displays to record.
///
/// `record` follows [`Follow::All`] unless it is given display ids, and then [`Follow::Only`]
/// those.
///
/// # Example
///
/// ```rust
/// use komorebi_recorder::capture::manager::Follow;
/// use std::collections::BTreeSet;
///
/// assert!(Follow::All.wants(7));
///
/// let only = Follow::Only(BTreeSet::from([2]));
/// assert!(only.wants(2));
/// assert!(!only.wants(7));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Follow {
    /// Every display, including ones connected while recording.
    All,
    /// Only these displays (and only when they are present). A selected display that is
    /// unplugged and plugged back in is recorded again; one that was not selected never is.
    Only(BTreeSet<u32>),
}

impl Follow {
    /// Returns `true` if and only if the display with this id is to be recorded.
    #[must_use]
    pub fn wants(&self, display: u32) -> bool {
        match self {
            Follow::All => true,
            Follow::Only(set) => set.contains(&display),
        }
    }
}

struct Capturing {
    capture: Box<dyn RunningCapture>,
    /// The display as the probe described it when the segment began.
    probed: DisplayDescriptor,
}

struct Pending {
    next_attempt_ns: u64,
    failures: u32,
    /// Whether the current outage has been reported yet.
    reported: bool,
}

/// Keeps every wanted display recording as displays come and go, one [`DisplayResult`] per segment.
///
/// Generic over the [`CaptureBackend`] so that the whole policy runs against a scripted backend.
/// The caller drives it: [`start`](DisplayManager::start) once, then
/// [`poll`](DisplayManager::poll) several times a second,
/// [`restart_all`](DisplayManager::restart_all) after a sleep,
/// [`checkpoint`](DisplayManager::checkpoint) now and then for the session manifest, and
/// [`shutdown`](DisplayManager::shutdown) at the end. Every method that can notice something
/// returns [`Draft`]s, incidents not yet timestamped, for the caller to log; the manager itself
/// writes nothing but the captures' files. See the [module documentation](self) for the rules.
///
/// Every `now_ns` is a time on the master clock (see [`crate::clock`]), passed in so that the
/// manager never reads a clock itself.
///
/// # Example
///
/// A laptop with two displays, one of which is unplugged and plugged back in. The backend is a
/// script: the example changes the list of attached displays between polls.
///
/// ```rust
/// use color_eyre::eyre;
/// use komorebi_recorder::capture::manager::{DisplayManager, Follow};
/// use komorebi_recorder::capture::{
///     CaptureBackend, CaptureConfig, DisplayDescriptor, EndReason, RunningCapture,
/// };
/// use komorebi_recorder::incidents::kind;
/// use std::cell::RefCell;
/// use std::path::{Path, PathBuf};
/// use std::rc::Rc;
/// # use komorebi_recorder::capture::{
/// #     CaptureStats, Codec, DisplayResult, LatencyStats, frames_file_name, video_file_name,
/// # };
/// # use std::sync::Arc;
/// #
/// # // A capture that writes nothing. Its first frame is at a time that makes the results
/// # // sort by segment and then by display.
/// # struct Recording {
/// #     display: u32,
/// #     segment: u32,
/// # }
/// #
/// # impl Recording {
/// #     fn result(&self, reason: Option<EndReason>) -> DisplayResult {
/// #         DisplayResult {
/// #             display: DisplayDescriptor {
/// #                 id: self.display,
/// #                 frame: [0.0, 0.0, 100.0, 100.0],
/// #                 width_points: 100,
/// #                 height_points: 100,
/// #                 asleep: false,
/// #                 scale: 1.0,
/// #             },
/// #             segment: self.segment,
/// #             scale: 1.0,
/// #             width_px: 100,
/// #             height_px: 100,
/// #             codec: Codec::H264,
/// #             file: video_file_name(self.display, self.segment),
/// #             frames_file: frames_file_name(self.display, self.segment),
/// #             video_t0_mach_ns: Some(u64::from(self.segment) * 10 + u64::from(self.display)),
/// #             video_end_mach_ns: None,
/// #             end_reason: reason,
/// #             frames: 0,
/// #             dropped: 0,
/// #             idle_callbacks: 0,
/// #             latency: LatencyStats::default(),
/// #             error: None,
/// #         }
/// #     }
/// # }
/// #
/// # impl RunningCapture for Recording {
/// #     fn display_id(&self) -> u32 {
/// #         self.display
/// #     }
/// #     fn segment(&self) -> u32 {
/// #         self.segment
/// #     }
/// #     fn stats(&self) -> Arc<CaptureStats> {
/// #         Arc::default()
/// #     }
/// #     fn partial(&self) -> DisplayResult {
/// #         self.result(None)
/// #     }
/// #     fn stop(self: Box<Self>, reason: EndReason, _end: Option<u64>) -> DisplayResult {
/// #         self.result(Some(reason))
/// #     }
/// # }
///
/// const SECOND: u64 = 1_000_000_000;
///
/// fn display(id: u32) -> DisplayDescriptor {
///     DisplayDescriptor {
///         id,
///         frame: [0.0, 0.0, 1920.0, 1080.0],
///         width_points: 1920,
///         height_points: 1080,
///         asleep: false,
///         scale: 1.0,
///     }
/// }
///
/// /// The displays that are attached, shared with the example so that it can unplug them.
/// struct Screen(Rc<RefCell<Vec<DisplayDescriptor>>>);
///
/// impl CaptureBackend for Screen {
///     fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
///         Ok(self.0.borrow().clone())
///     }
///
///     fn start(
///         &self,
///         display: &DisplayDescriptor,
///         segment: u32,
///         _config: &CaptureConfig,
///         _dir: &Path,
///     ) -> eyre::Result<Box<dyn RunningCapture>> {
///         Ok(Box::new(Recording { display: display.id, segment }))
///     }
/// }
///
/// let attached = Rc::new(RefCell::new(vec![display(1), display(2)]));
/// let mut manager = DisplayManager::new(
///     Screen(attached.clone()),
///     CaptureConfig::default(),
///     PathBuf::from("/session"),
///     Follow::All,
/// );
///
/// // Both displays are recorded at once, with no settle delay.
/// let drafts = manager.start(0)?;
/// assert_eq!(drafts.len(), 2);
/// assert!(drafts.iter().all(|draft| draft.kind == kind::CAPTURE_STARTED));
/// assert_eq!(manager.recording_count(), 2);
///
/// // Display 2 is unplugged. Its segment is finalized on the spot, and display 1 carries on.
/// attached.borrow_mut().retain(|display| display.id != 2);
/// let drafts = manager.poll(5 * SECOND);
/// assert_eq!(drafts.len(), 1);
/// assert_eq!(drafts[0].kind, kind::DISPLAY_DISCONNECTED);
/// assert_eq!(drafts[0].display, Some(2));
/// assert_eq!(manager.recording_count(), 1);
///
/// // It is plugged back in. It is reported as available again, not as new, and is recorded as a
/// // new segment once it has been present for a second.
/// attached.borrow_mut().push(display(2));
/// let drafts = manager.poll(20 * SECOND);
/// assert_eq!(drafts[0].kind, kind::DISPLAY_AWAKE);
/// assert_eq!(manager.recording_count(), 1, "still settling");
/// let drafts = manager.poll(21 * SECOND);
/// assert_eq!(drafts[0].kind, kind::CAPTURE_STARTED);
/// assert_eq!(manager.recording_count(), 2);
///
/// // Stopping returns every segment in time order. Display 2 has two, in two files.
/// let results = manager.shutdown();
/// let segments: Vec<_> = results
///     .iter()
///     .map(|result| (result.file.as_str(), result.end_reason.expect("every segment ended")))
///     .collect();
/// assert_eq!(
///     segments,
///     [
///         ("video-1.mp4", EndReason::Stopped),
///         ("video-2.mp4", EndReason::DisplayDisconnected),
///         ("video-2-2.mp4", EndReason::Stopped),
///     ]
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct DisplayManager<B: CaptureBackend> {
    backend: B,
    config: CaptureConfig,
    dir: PathBuf,
    follow: Follow,
    active: BTreeMap<u32, Capturing>,
    pending: BTreeMap<u32, Pending>,
    next_segment: BTreeMap<u32, u32>,
    finished: Vec<DisplayResult>,
    /// Every display id ever seen, so one that returns is reported as returning, not as new.
    seen: BTreeSet<u32>,
    probe_failed: bool,
}

fn backoff_ns(failures: u32) -> u64 {
    FIRST_BACKOFF_NS
        .saturating_mul(1 << failures.saturating_sub(1).min(8))
        .min(MAX_BACKOFF_NS)
}

impl<B: CaptureBackend> DisplayManager<B> {
    /// Creates a manager that records the displays `follow` selects through `backend`, with
    /// `config`, into `dir`.
    ///
    /// `dir` is the session directory the segment files are written to. Nothing is started and the
    /// backend is not asked anything: see [`start`](DisplayManager::start).
    pub fn new(backend: B, config: CaptureConfig, dir: PathBuf, follow: Follow) -> Self {
        Self {
            backend,
            config,
            dir,
            follow,
            active: BTreeMap::new(),
            pending: BTreeMap::new(),
            next_segment: BTreeMap::new(),
            finished: Vec::new(),
            seen: BTreeSet::new(),
            probe_failed: false,
        }
    }

    /// Returns the displays that are present right now and wanted: followed (see [`Follow`]) and
    /// not asleep.
    ///
    /// # Errors
    ///
    /// Returns the backend's error when it cannot list the displays
    /// ([`CaptureBackend::active_displays`]).
    pub fn wanted_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
        Ok(self
            .backend
            .active_displays()?
            .into_iter()
            .filter(|display| self.follow.wants(display.id) && !display.asleep)
            .collect())
    }

    /// Begins recording every wanted display that is present, with no settle delay, and returns the
    /// incidents that came of it.
    ///
    /// Call it once, before the first [`poll`](DisplayManager::poll). A display whose capture fails
    /// to start is not an error here: the failure comes back as a `capture-failed` draft (once) and
    /// the start is retried with a backoff by `poll`. A caller that needs at least one display
    /// recording checks [`recording_count`](DisplayManager::recording_count) afterwards.
    ///
    /// # Errors
    ///
    /// Returns the backend's error when it cannot list the displays
    /// ([`CaptureBackend::active_displays`]). Nothing has been started in that case.
    pub fn start(&mut self, now_ns: u64) -> eyre::Result<Vec<Draft>> {
        for display in self.wanted_displays()? {
            self.seen.insert(display.id);
            self.pending.insert(
                display.id,
                Pending {
                    next_attempt_ns: now_ns,
                    failures: 0,
                    reported: false,
                },
            );
        }

        Ok(self.poll(now_ns))
    }

    /// Reconciles what is recording with what is present, and returns the incidents that came of
    /// it. Cheap when nothing changed; call it several times a second.
    ///
    /// Every poll first gives each running capture its
    /// [`keepalive`](RunningCapture::keepalive). It then lists the displays. If listing fails,
    /// nothing else changes and the failure is reported once as a `recorder` draft until a listing
    /// succeeds again. Otherwise it ends the captures that have to end (see the [module
    /// documentation](self) for the rules and their order) and starts the ones that are missing:
    ///
    /// - a display seen for the first time is reported as `display-connected`, and one that had
    ///   been seen before as `display-awake` ("available again"). Either is started after the
    ///   settle delay;
    /// - a start that fails is reported once as `capture-failed`, whatever the number of
    ///   attempts, and retried after a backoff that doubles from one second up to ten;
    /// - a start that succeeds is reported as `capture-started`, or as `capture-recovered` if it
    ///   follows a failure or a dead stream;
    /// - a display that is asleep or not followed is left alone.
    ///
    /// A draft dated by [`Draft::at`] carries the time the thing happened when that is earlier
    /// than the poll (a stream that died).
    pub fn poll(&mut self, now_ns: u64) -> Vec<Draft> {
        let mut drafts = Vec::new();

        for capturing in self.active.values() {
            capturing.capture.keepalive(now_ns);
        }

        let probe = match self.backend.active_displays() {
            Ok(displays) => {
                self.probe_failed = false;
                displays
            }
            Err(error) => {
                // not being able to ask is not the same as every display having left
                if !self.probe_failed {
                    self.probe_failed = true;
                    drafts.push(Draft::new(
                        kind::RECORDER,
                        format!("could not list displays, keeping current captures: {error:#}"),
                    ));
                }
                return drafts;
            }
        };
        let present: BTreeMap<u32, DisplayDescriptor> = probe
            .into_iter()
            .map(|display| (display.id, display))
            .collect();

        self.end_dead_captures(&present, now_ns, &mut drafts);
        self.start_missing_captures(&present, now_ns, &mut drafts);
        self.pending.retain(|id, _| present.contains_key(id));

        drafts
    }

    /// Finalize captures whose stream died, whose display left, or whose display changed size.
    fn end_dead_captures(
        &mut self,
        present: &BTreeMap<u32, DisplayDescriptor>,
        now_ns: u64,
        drafts: &mut Vec<Draft>,
    ) {
        let ids: Vec<u32> = self.active.keys().copied().collect();
        for id in ids {
            let Some(capturing) = self.active.get(&id) else {
                continue;
            };

            let stats = capturing.capture.stats();
            let stalled_for = capturing
                .capture
                .last_signal_mach_ns()
                .map(|last| (last, now_ns.saturating_sub(last)))
                .filter(|(_, silent_for)| *silent_for > STALL_NS);

            let (reason, end, detail) = if let Some(error) = stats.error() {
                (
                    EndReason::StreamError,
                    // end at the moment it died, not whenever we noticed
                    Some(stats.error_at_mach_ns().unwrap_or(now_ns)),
                    format!("display {id}: {error}"),
                )
            } else if !present.contains_key(&id) {
                (
                    EndReason::DisplayDisconnected,
                    None,
                    format!("display {id} disconnected"),
                )
            } else if present.get(&id).is_some_and(|display| display.asleep) {
                (
                    EndReason::DisplayAsleep,
                    None,
                    format!("display {id} went to sleep"),
                )
            } else if let Some((last_signal, silent_for)) = stalled_for {
                (
                    EndReason::StreamError,
                    Some(last_signal),
                    format!(
                        "display {id}: the stream stalled (no frames or idle callbacks for {:.1}s)",
                        silent_for as f64 / 1e9
                    ),
                )
            } else if let Some(now_display) = present.get(&id) {
                if capturing.probed.size_differs(now_display) {
                    (
                        EndReason::Reconfigured,
                        None,
                        format!(
                            "display {id} changed from {}x{} to {}x{} points",
                            capturing.probed.width_points,
                            capturing.probed.height_points,
                            now_display.width_points,
                            now_display.height_points
                        ),
                    )
                } else {
                    continue;
                }
            } else {
                continue;
            };

            let result = self.finalize(id, reason, end);
            let event_kind = match reason {
                EndReason::DisplayDisconnected => kind::DISPLAY_DISCONNECTED,
                EndReason::DisplayAsleep => kind::DISPLAY_ASLEEP,
                EndReason::Reconfigured => kind::DISPLAY_RECONFIGURED,
                _ => kind::CAPTURE_STOPPED,
            };
            let mut draft = Draft::new(
                event_kind,
                format!(
                    "{detail}; segment {} ({}) finalized with {} frames",
                    result.segment, result.file, result.frames
                ),
            )
            .display(id);
            if let Some(end) = end {
                draft = draft.at(end);
            }
            drafts.push(draft);

            // a display that is still here but whose stream died is retried; one that left, or
            // went to sleep, is picked up again if and when it comes back
            if present.get(&id).is_some_and(|display| !display.asleep) {
                let delay = if reason == EndReason::Reconfigured {
                    0
                } else {
                    FIRST_BACKOFF_NS
                };
                self.pending.insert(
                    id,
                    Pending {
                        next_attempt_ns: now_ns + delay,
                        failures: u32::from(reason == EndReason::StreamError),
                        reported: reason != EndReason::Reconfigured,
                    },
                );
            }
        }
    }

    fn start_missing_captures(
        &mut self,
        present: &BTreeMap<u32, DisplayDescriptor>,
        now_ns: u64,
        drafts: &mut Vec<Draft>,
    ) {
        for (id, display) in present {
            if self.active.contains_key(id) || !self.follow.wants(*id) {
                continue;
            }

            // nothing to record until it wakes
            if display.asleep {
                continue;
            }

            let returning = !self.seen.insert(*id);
            let pending = self.pending.entry(*id).or_insert_with(|| {
                drafts.push(
                    Draft::new(
                        if returning {
                            kind::DISPLAY_AWAKE
                        } else {
                            kind::DISPLAY_CONNECTED
                        },
                        format!(
                            "display {id} {} ({}x{} points at {}, {})",
                            if returning {
                                "is available again"
                            } else {
                                "connected"
                            },
                            display.width_points,
                            display.height_points,
                            display.frame[0],
                            display.frame[1]
                        ),
                    )
                    .display(*id),
                );
                // a display that just appeared may still be waking up: let it settle first
                Pending {
                    next_attempt_ns: now_ns + SETTLE_NS,
                    failures: 0,
                    reported: false,
                }
            });

            if now_ns < pending.next_attempt_ns {
                continue;
            }

            let segment = self.next_segment.get(id).copied().unwrap_or(1);
            match self
                .backend
                .start(display, segment, &self.config, &self.dir)
            {
                Ok(capture) => {
                    let recovered = pending.reported;
                    self.next_segment.insert(*id, segment + 1);
                    self.pending.remove(id);
                    self.active.insert(
                        *id,
                        Capturing {
                            capture,
                            probed: display.clone(),
                        },
                    );

                    drafts.push(
                        Draft::new(
                            if recovered {
                                kind::CAPTURE_RECOVERED
                            } else {
                                kind::CAPTURE_STARTED
                            },
                            format!(
                                "display {id}: recording segment {segment} to {}",
                                crate::capture::video_file_name(*id, segment)
                            ),
                        )
                        .display(*id),
                    );
                }
                Err(error) => {
                    pending.failures += 1;
                    pending.next_attempt_ns = now_ns + backoff_ns(pending.failures);

                    if !pending.reported {
                        pending.reported = true;
                        drafts.push(
                            Draft::new(
                                kind::CAPTURE_FAILED,
                                format!("display {id}: could not start recording, will keep retrying: {error:#}"),
                            )
                            .display(*id),
                        );
                    }
                }
            }
        }
    }

    fn finalize(&mut self, id: u32, reason: EndReason, end_mach_ns: Option<u64>) -> DisplayResult {
        let Some(capturing) = self.active.remove(&id) else {
            unreachable!("only called for ids taken from the active set");
        };

        let result = capturing.capture.stop(reason, end_mach_ns);
        self.finished.push(result.clone());
        result
    }

    /// Ends every capture and starts fresh ones shortly after, for when the machine has been
    /// asleep: streams are not trusted across a sleep, because they can stall silently.
    ///
    /// Each running capture is stopped with `reason` (the recorder passes
    /// [`EndReason::Resumed`]) and its display is started again after the settle delay, as a new
    /// segment. A display that did not come back is not resurrected: the next
    /// [`poll`](DisplayManager::poll) drops the pending start of a display that is no longer
    /// present. A start after a restart is reported as `capture-started`.
    pub fn restart_all(&mut self, now_ns: u64, reason: EndReason) -> Vec<Draft> {
        let mut drafts = Vec::new();

        for id in self.active.keys().copied().collect::<Vec<_>>() {
            let result = self.finalize(id, reason, None);
            drafts.push(
                Draft::new(
                    kind::CAPTURE_STOPPED,
                    format!(
                        "display {id}: {reason}; segment {} ({}) finalized with {} frames and a new one will start",
                        result.segment, result.file, result.frames
                    ),
                )
                .display(id),
            );

            self.pending.insert(
                id,
                Pending {
                    next_attempt_ns: now_ns + SETTLE_NS,
                    failures: 0,
                    reported: false,
                },
            );
        }

        drafts
    }

    /// Returns the number of displays being recorded right now.
    #[must_use]
    pub fn recording_count(&self) -> usize {
        self.active.len()
    }

    /// Returns the ids of the displays waiting for another attempt after a failed start or a dead
    /// stream.
    #[must_use]
    pub fn retrying(&self) -> Vec<u32> {
        self.pending
            .iter()
            .filter(|(_, pending)| pending.reported)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Returns `(frames, dropped)` summed over every segment so far, finished or live.
    #[must_use]
    pub fn totals(&self) -> (u64, u64) {
        let (mut frames, mut dropped) = (0, 0);
        for result in &self.finished {
            frames += result.frames;
            dropped += result.dropped;
        }

        for capturing in self.active.values() {
            let stats = capturing.capture.stats();
            frames += stats.frames.load(std::sync::atomic::Ordering::Relaxed);
            dropped += stats.dropped.load(std::sync::atomic::Ordering::Relaxed);
        }

        (frames, dropped)
    }

    /// Returns every segment so far, with live ones in their current state
    /// ([`RunningCapture::partial`]).
    ///
    /// The segments are in time order: by the time of their first frame (those with none last),
    /// then display id, then segment number. For manifest checkpoints: it does not end anything.
    #[must_use]
    pub fn checkpoint(&self) -> Vec<DisplayResult> {
        let mut results = self.finished.clone();
        results.extend(
            self.active
                .values()
                .map(|capturing| capturing.capture.partial()),
        );
        sort_segments(&mut results);
        results
    }

    /// Stops everything and returns every segment in time order, as [`checkpoint`] orders them.
    ///
    /// Each running capture is stopped with [`EndReason::Stopped`]. Afterwards no capture is
    /// running, no start is pending and the finished segments have been handed over, so a second
    /// call returns an empty list.
    ///
    /// [`checkpoint`]: DisplayManager::checkpoint
    pub fn shutdown(&mut self) -> Vec<DisplayResult> {
        let ids: Vec<u32> = self.active.keys().copied().collect();
        for id in ids {
            self.finalize(id, EndReason::Stopped, None);
        }

        self.pending.clear();
        let mut results = std::mem::take(&mut self.finished);
        sort_segments(&mut results);
        results
    }
}

fn sort_segments(results: &mut [DisplayResult]) {
    results.sort_by_key(|result| {
        (
            result.video_t0_mach_ns.unwrap_or(u64::MAX),
            result.display.id,
            result.segment,
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::CaptureStats;
    use crate::capture::LatencyStats;
    use crate::capture::frames_file_name;
    use crate::capture::video_file_name;
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::Arc;

    const SEC: u64 = 1_000_000_000;

    fn display(id: u32, width: i64, height: i64, x: f64) -> DisplayDescriptor {
        DisplayDescriptor {
            id,
            frame: [x, 0.0, width as f64, height as f64],
            width_points: width,
            height_points: height,
            asleep: false,
            scale: 1.0,
        }
    }

    fn asleep(mut display: DisplayDescriptor) -> DisplayDescriptor {
        display.asleep = true;
        display
    }

    fn builtin() -> DisplayDescriptor {
        display(1, 1800, 1169, -1800.0)
    }

    fn external() -> DisplayDescriptor {
        display(2, 3840, 1620, 0.0)
    }

    /// (display, segment, reason, end) of a stopped capture
    type Stop = (u32, u32, EndReason, Option<u64>);

    #[derive(Default)]
    struct World {
        displays: RefCell<Vec<DisplayDescriptor>>,
        /// displays whose start currently fails
        failing: RefCell<BTreeSet<u32>>,
        probe_fails: Cell<bool>,
        /// (display, segment) for every start
        starts: RefCell<Vec<(u32, u32)>>,
        /// (display, segment, reason, end) for every stop
        stops: RefCell<Vec<Stop>>,
        /// the stats of every live capture, by display, so a test can kill one
        stats: RefCell<BTreeMap<u32, Arc<CaptureStats>>>,
        /// when each live capture last showed a sign of life; absent means the mock cannot tell
        signals: RefCell<BTreeMap<u32, u64>>,
        /// the display of every keepalive call
        keepalives: RefCell<Vec<u32>>,
    }

    struct MockBackend(Rc<World>);

    struct MockCapture {
        id: u32,
        segment: u32,
        stats: Arc<CaptureStats>,
        world: Rc<World>,
    }

    impl RunningCapture for MockCapture {
        fn display_id(&self) -> u32 {
            self.id
        }

        fn segment(&self) -> u32 {
            self.segment
        }

        fn stats(&self) -> Arc<CaptureStats> {
            self.stats.clone()
        }

        fn partial(&self) -> DisplayResult {
            self.result(None, None)
        }

        fn last_signal_mach_ns(&self) -> Option<u64> {
            self.world.signals.borrow().get(&self.id).copied()
        }

        fn keepalive(&self, _now_ns: u64) {
            self.world.keepalives.borrow_mut().push(self.id);
        }

        fn stop(self: Box<Self>, reason: EndReason, end_mach_ns: Option<u64>) -> DisplayResult {
            self.world
                .stops
                .borrow_mut()
                .push((self.id, self.segment, reason, end_mach_ns));
            self.world.stats.borrow_mut().remove(&self.id);
            self.result(Some(reason), end_mach_ns)
        }
    }

    impl MockCapture {
        fn result(&self, reason: Option<EndReason>, end: Option<u64>) -> DisplayResult {
            DisplayResult {
                display: display(self.id, 100, 100, 0.0),
                segment: self.segment,
                scale: 1.0,
                width_px: 100,
                height_px: 100,
                codec: crate::capture::Codec::Hevc,
                file: video_file_name(self.id, self.segment),
                frames_file: frames_file_name(self.id, self.segment),
                video_t0_mach_ns: Some(u64::from(self.segment) * 1_000 + u64::from(self.id)),
                video_end_mach_ns: end,
                end_reason: reason,
                frames: 10,
                dropped: 0,
                idle_callbacks: 0,
                latency: LatencyStats::default(),
                error: None,
            }
        }
    }

    impl CaptureBackend for MockBackend {
        fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
            if self.0.probe_fails.get() {
                eyre::bail!("window server is busy");
            }

            Ok(self.0.displays.borrow().clone())
        }

        fn start(
            &self,
            display: &DisplayDescriptor,
            segment: u32,
            _config: &CaptureConfig,
            _dir: &Path,
        ) -> eyre::Result<Box<dyn RunningCapture>> {
            if self.0.failing.borrow().contains(&display.id) {
                eyre::bail!("display not ready");
            }

            self.0.starts.borrow_mut().push((display.id, segment));
            let stats = Arc::new(CaptureStats::default());
            self.0.stats.borrow_mut().insert(display.id, stats.clone());
            Ok(Box::new(MockCapture {
                id: display.id,
                segment,
                stats,
                world: self.0.clone(),
            }))
        }
    }

    fn manager(world: &Rc<World>, follow: Follow) -> DisplayManager<MockBackend> {
        DisplayManager::new(
            MockBackend(world.clone()),
            CaptureConfig::default(),
            PathBuf::from("/session"),
            follow,
        )
    }

    fn kinds(drafts: &[Draft]) -> Vec<&'static str> {
        drafts.iter().map(|draft| draft.kind).collect()
    }

    fn world_with(displays: Vec<DisplayDescriptor>) -> Rc<World> {
        let world = Rc::new(World::default());
        *world.displays.borrow_mut() = displays;
        world
    }

    #[test]
    fn every_present_display_starts_immediately() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);

        let drafts = manager.start(0).expect("start");

        assert_eq!(manager.recording_count(), 2);
        assert_eq!(*world.starts.borrow(), vec![(1, 1), (2, 1)]);
        assert_eq!(
            kinds(&drafts),
            vec![kind::CAPTURE_STARTED, kind::CAPTURE_STARTED]
        );
    }

    #[test]
    fn a_display_connected_mid_recording_is_picked_up_after_it_settles() {
        let world = world_with(vec![builtin()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        // the dock is connected
        world.displays.borrow_mut().push(external());
        let drafts = manager.poll(10 * SEC);
        assert_eq!(kinds(&drafts), vec![kind::DISPLAY_CONNECTED]);
        assert_eq!(
            manager.recording_count(),
            1,
            "not started yet: it is still settling"
        );

        assert!(manager.poll(10 * SEC + SETTLE_NS / 2).is_empty());
        let drafts = manager.poll(10 * SEC + SETTLE_NS);
        assert_eq!(kinds(&drafts), vec![kind::CAPTURE_STARTED]);
        assert_eq!(manager.recording_count(), 2);
        assert_eq!(*world.starts.borrow(), vec![(1, 1), (2, 1)]);
    }

    #[test]
    fn a_disconnect_finalizes_only_that_display_and_keeps_the_others_untouched() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        // the external monitor is unplugged
        world
            .displays
            .borrow_mut()
            .retain(|display| display.id != 2);
        let drafts = manager.poll(5 * SEC);

        assert_eq!(kinds(&drafts), vec![kind::DISPLAY_DISCONNECTED]);
        assert_eq!(
            *world.stops.borrow(),
            vec![(2, 1, EndReason::DisplayDisconnected, None)]
        );
        assert_eq!(manager.recording_count(), 1);
        assert!(
            world.stats.borrow().contains_key(&1),
            "the built-in display's capture must keep running"
        );
    }

    #[test]
    fn a_reconnected_display_gets_a_new_segment_in_a_new_file() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        world
            .displays
            .borrow_mut()
            .retain(|display| display.id != 2);
        manager.poll(5 * SEC);

        world.displays.borrow_mut().push(external());
        manager.poll(20 * SEC);
        manager.poll(20 * SEC + SETTLE_NS);

        assert_eq!(*world.starts.borrow(), vec![(1, 1), (2, 1), (2, 2)]);

        let results = manager.shutdown();
        let files: Vec<&str> = results.iter().map(|result| result.file.as_str()).collect();
        assert!(
            files.contains(&"video-2.mp4") && files.contains(&"video-2-2.mp4"),
            "{files:?}"
        );
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn a_dead_stream_ends_at_the_moment_it_died_and_is_restarted_after_a_delay() {
        let world = world_with(vec![external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        // the machine slept: ScreenCaptureKit reports the stream stopped at t=30s
        world.stats.borrow()[&2].set_error("stream stopped: display sleep".into(), 30 * SEC);

        let drafts = manager.poll(33 * SEC);
        assert_eq!(kinds(&drafts), vec![kind::CAPTURE_STOPPED]);
        assert_eq!(
            drafts[0].mach_ns,
            Some(30 * SEC),
            "dated when it died, not when noticed"
        );
        assert_eq!(
            *world.stops.borrow(),
            vec![(2, 1, EndReason::StreamError, Some(30 * SEC))]
        );
        assert_eq!(manager.recording_count(), 0);

        // not restarted immediately...
        assert!(manager.poll(33 * SEC + FIRST_BACKOFF_NS / 2).is_empty());
        // ...but after the backoff, as a new segment
        let drafts = manager.poll(33 * SEC + FIRST_BACKOFF_NS);
        assert_eq!(kinds(&drafts), vec![kind::CAPTURE_RECOVERED]);
        assert_eq!(*world.starts.borrow(), vec![(2, 1), (2, 2)]);
    }

    #[test]
    fn a_failing_start_is_retried_with_backoff_and_reported_once() {
        let world = world_with(vec![external()]);
        world.failing.borrow_mut().insert(2);
        let mut manager = manager(&world, Follow::All);

        let drafts = manager.start(0).expect("start");
        assert_eq!(kinds(&drafts), vec![kind::CAPTURE_FAILED]);
        assert_eq!(manager.retrying(), vec![2]);

        // more failures: silent, with growing delays
        let mut attempts = 0;
        let mut now = 0;
        for _ in 0..40 {
            now += SEC;
            let drafts = manager.poll(now);
            assert!(drafts.is_empty(), "outage reported again: {drafts:?}");
            attempts += 1;
        }
        assert!(attempts > 0);

        // the display becomes ready (the screen was unlocked)
        world.failing.borrow_mut().clear();
        let mut recovered = Vec::new();
        for _ in 0..12 {
            now += SEC;
            recovered.extend(manager.poll(now));
        }

        assert_eq!(kinds(&recovered), vec![kind::CAPTURE_RECOVERED]);
        assert_eq!(manager.recording_count(), 1);
        assert!(manager.retrying().is_empty());
        assert_eq!(
            *world.starts.borrow(),
            vec![(2, 1)],
            "the failed attempts did not use up segment numbers"
        );
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff_ns(1), SEC);
        assert_eq!(backoff_ns(2), 2 * SEC);
        assert_eq!(backoff_ns(3), 4 * SEC);
        assert_eq!(backoff_ns(4), 8 * SEC);
        assert_eq!(backoff_ns(5), MAX_BACKOFF_NS);
        assert_eq!(backoff_ns(500), MAX_BACKOFF_NS);
    }

    #[test]
    fn explicitly_selected_displays_ignore_the_others_but_recover_their_own() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::Only(BTreeSet::from([2])));
        manager.start(0).expect("start");
        assert_eq!(*world.starts.borrow(), vec![(2, 1)]);

        // a third monitor appears: not wanted
        world
            .displays
            .borrow_mut()
            .push(display(3, 1920, 1080, 3840.0));
        assert!(manager.poll(10 * SEC).is_empty());
        assert!(manager.poll(20 * SEC).is_empty());
        assert_eq!(*world.starts.borrow(), vec![(2, 1)]);

        // the selected one is unplugged and replugged: recorded again
        world
            .displays
            .borrow_mut()
            .retain(|display| display.id != 2);
        manager.poll(30 * SEC);
        world.displays.borrow_mut().push(external());
        manager.poll(40 * SEC);
        manager.poll(40 * SEC + SETTLE_NS);
        assert_eq!(*world.starts.borrow(), vec![(2, 1), (2, 2)]);
    }

    #[test]
    fn a_size_change_starts_a_new_segment_but_a_move_does_not() {
        let world = world_with(vec![builtin()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        // arrangement changed: same size, new position
        *world.displays.borrow_mut() = vec![display(1, 1800, 1169, 0.0)];
        assert!(manager.poll(5 * SEC).is_empty());
        assert!(world.stops.borrow().is_empty());

        // resolution changed: the old segment is finalized and the new one starts in the same
        // poll, so there is no gap
        *world.displays.borrow_mut() = vec![display(1, 1512, 982, 0.0)];
        let drafts = manager.poll(10 * SEC);
        assert_eq!(
            kinds(&drafts),
            vec![kind::DISPLAY_RECONFIGURED, kind::CAPTURE_STARTED]
        );
        assert_eq!(world.stops.borrow()[0].2, EndReason::Reconfigured);
        assert_eq!(*world.starts.borrow(), vec![(1, 1), (1, 2)]);
        assert_eq!(manager.recording_count(), 1);
    }

    #[test]
    fn an_unreadable_display_list_changes_nothing() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        world.probe_fails.set(true);
        let drafts = manager.poll(5 * SEC);
        assert_eq!(kinds(&drafts), vec![kind::RECORDER], "reported once");
        assert!(manager.poll(6 * SEC).is_empty(), "and not again");
        assert!(
            world.stops.borrow().is_empty(),
            "not being able to ask is not a disconnect"
        );
        assert_eq!(manager.recording_count(), 2);

        world.probe_fails.set(false);
        assert!(manager.poll(7 * SEC).is_empty());
    }

    #[test]
    fn a_whole_sleep_wake_cycle_ends_and_restarts_every_display() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        // sleep: every display leaves at once
        world.displays.borrow_mut().clear();
        let drafts = manager.poll(60 * SEC);
        assert_eq!(drafts.len(), 2);
        assert_eq!(manager.recording_count(), 0);

        // wake: both come back; they settle, then record as new segments
        *world.displays.borrow_mut() = vec![builtin(), external()];
        manager.poll(3600 * SEC);
        manager.poll(3600 * SEC + SETTLE_NS);
        assert_eq!(manager.recording_count(), 2);
        assert_eq!(*world.starts.borrow(), vec![(1, 1), (2, 1), (1, 2), (2, 2)]);

        let results = manager.shutdown();
        assert_eq!(results.len(), 4);
        assert!(
            results
                .iter()
                .filter(|result| result.segment == 1)
                .all(|result| result.end_reason == Some(EndReason::DisplayDisconnected)),
            "the pre-sleep segments were finalized when the displays left"
        );
    }

    #[test]
    fn nothing_is_lost_when_displays_flap_repeatedly() {
        let world = world_with(vec![external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        let mut now = 0;
        for _ in 0..5 {
            now += 10 * SEC;
            world.displays.borrow_mut().clear();
            manager.poll(now);
            now += SEC;
            *world.displays.borrow_mut() = vec![external()];
            manager.poll(now);
            manager.poll(now + SETTLE_NS);
            now += SETTLE_NS;
        }

        let results = manager.shutdown();
        assert_eq!(results.len(), 6, "one segment per connection");
        let mut files: Vec<_> = results.iter().map(|result| result.file.clone()).collect();
        files.sort();
        files.dedup();
        assert_eq!(files.len(), 6, "no two segments share a file");
        // every segment was finalized, none abandoned
        assert!(results.iter().all(|result| result.end_reason.is_some()));
    }

    #[test]
    fn checkpoints_include_live_segments_without_ending_them() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");
        world
            .displays
            .borrow_mut()
            .retain(|display| display.id != 2);
        manager.poll(5 * SEC);

        let checkpoint = manager.checkpoint();
        assert_eq!(checkpoint.len(), 2);
        assert_eq!(
            checkpoint
                .iter()
                .filter(|result| result.end_reason.is_none())
                .count(),
            1,
            "the live segment has no end yet"
        );
        assert_eq!(manager.recording_count(), 1, "checkpointing did not end it");
        // the finished segment's frames plus the live capture's own counters
        assert_eq!(manager.totals(), (10, 0));
    }

    #[test]
    fn no_wanted_display_present_means_nothing_to_record_yet() {
        let world = world_with(vec![]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");
        assert_eq!(manager.recording_count(), 0);
        assert!(manager.wanted_displays().expect("list").is_empty());

        // a display shows up later and is recorded
        world.displays.borrow_mut().push(external());
        manager.poll(SEC);
        manager.poll(SEC + SETTLE_NS);
        assert_eq!(manager.recording_count(), 1);
    }

    #[test]
    fn a_stalled_stream_is_replaced_and_ends_when_it_last_showed_life() {
        let world = world_with(vec![external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        // healthy at 10s, then silent: no frames and no idle callbacks
        world.signals.borrow_mut().insert(2, 10 * SEC);
        assert!(manager.poll(10 * SEC + STALL_NS / 2).is_empty(), "not yet");

        let drafts = manager.poll(10 * SEC + STALL_NS + SEC);
        assert_eq!(kinds(&drafts), vec![kind::CAPTURE_STOPPED]);
        assert!(drafts[0].detail.contains("stalled"), "{}", drafts[0].detail);
        assert_eq!(
            *world.stops.borrow(),
            vec![(2, 1, EndReason::StreamError, Some(10 * SEC))],
            "the segment ends when the stream last showed life, not when we noticed"
        );

        // and it is restarted as a new segment after the backoff
        world.signals.borrow_mut().clear();
        manager.poll(10 * SEC + STALL_NS + SEC + FIRST_BACKOFF_NS);
        assert_eq!(*world.starts.borrow(), vec![(2, 1), (2, 2)]);
    }

    #[test]
    fn a_healthy_idle_stream_is_never_mistaken_for_a_stalled_one() {
        let world = world_with(vec![external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        // a static screen: no new frames for ten minutes, but idle callbacks keep arriving
        let mut now = 0;
        for _ in 0..600 {
            now += SEC;
            world.signals.borrow_mut().insert(2, now);
            assert!(manager.poll(now).is_empty());
        }

        assert!(world.stops.borrow().is_empty());
        assert_eq!(manager.recording_count(), 1);
    }

    #[test]
    fn a_stream_that_never_signals_is_not_assumed_stalled_when_the_backend_cannot_tell() {
        let world = world_with(vec![external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        assert!(manager.poll(3600 * SEC).is_empty());
        assert_eq!(manager.recording_count(), 1);
    }

    #[test]
    fn a_vanished_display_is_a_disconnect_even_if_its_stream_had_also_gone_quiet() {
        let world = world_with(vec![external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");
        world.signals.borrow_mut().insert(2, SEC);

        world.displays.borrow_mut().clear();
        let drafts = manager.poll(60 * SEC);
        assert_eq!(kinds(&drafts), vec![kind::DISPLAY_DISCONNECTED]);
    }

    #[test]
    fn every_live_capture_is_offered_a_keepalive_on_every_poll() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");
        world.keepalives.borrow_mut().clear();

        manager.poll(SEC);
        manager.poll(2 * SEC);
        let calls = world.keepalives.borrow().clone();
        assert_eq!(calls.iter().filter(|id| **id == 1).count(), 2);
        assert_eq!(calls.iter().filter(|id| **id == 2).count(), 2);
    }

    #[test]
    fn waking_from_sleep_replaces_every_capture_after_a_settle_delay() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        // the machine slept for an hour and woke with the same displays attached
        let drafts = manager.restart_all(100 * SEC, EndReason::Resumed);
        assert_eq!(drafts.len(), 2);
        assert_eq!(manager.recording_count(), 0);
        assert!(
            world
                .stops
                .borrow()
                .iter()
                .all(|stop| stop.2 == EndReason::Resumed)
        );

        assert!(manager.poll(100 * SEC).is_empty(), "still settling");
        let drafts = manager.poll(100 * SEC + SETTLE_NS);
        assert_eq!(
            kinds(&drafts),
            vec![kind::CAPTURE_STARTED, kind::CAPTURE_STARTED]
        );
        assert_eq!(*world.starts.borrow(), vec![(1, 1), (2, 1), (1, 2), (2, 2)]);
    }

    #[test]
    fn restarting_after_sleep_does_not_resurrect_a_display_that_did_not_come_back() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        manager.restart_all(100 * SEC, EndReason::Resumed);
        // the external monitor was unplugged while the laptop slept
        world
            .displays
            .borrow_mut()
            .retain(|display| display.id != 2);

        manager.poll(100 * SEC + SETTLE_NS);
        assert_eq!(manager.recording_count(), 1);
        assert_eq!(*world.starts.borrow(), vec![(1, 1), (2, 1), (1, 2)]);
    }

    #[test]
    fn a_sleeping_display_ends_its_segment_at_once_and_returns_as_a_new_one_when_it_wakes() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        // the built-in display goes to sleep (still attached, but dark)
        *world.displays.borrow_mut() = vec![asleep(builtin()), external()];
        let drafts = manager.poll(30 * SEC);
        assert_eq!(kinds(&drafts), vec![kind::DISPLAY_ASLEEP]);
        assert_eq!(
            *world.stops.borrow(),
            vec![(1, 1, EndReason::DisplayAsleep, None)]
        );
        assert_eq!(
            manager.recording_count(),
            1,
            "the other display is untouched"
        );

        // nothing is attempted, reported or retried while it sleeps
        for step in 1..30 {
            assert!(manager.poll((30 + step) * SEC).is_empty());
        }
        assert_eq!(*world.starts.borrow(), vec![(1, 1), (2, 1)]);
        assert!(manager.retrying().is_empty());

        // it wakes: reported as available again (not as a new display), then recorded afresh
        *world.displays.borrow_mut() = vec![builtin(), external()];
        let drafts = manager.poll(100 * SEC);
        assert_eq!(kinds(&drafts), vec![kind::DISPLAY_AWAKE]);
        let drafts = manager.poll(100 * SEC + SETTLE_NS);
        assert_eq!(kinds(&drafts), vec![kind::CAPTURE_STARTED]);
        assert_eq!(*world.starts.borrow(), vec![(1, 1), (2, 1), (1, 2)]);
    }

    #[test]
    fn a_display_that_is_asleep_when_recording_begins_waits_to_be_recorded() {
        let world = world_with(vec![asleep(builtin()), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");
        assert_eq!(*world.starts.borrow(), vec![(2, 1)]);

        *world.displays.borrow_mut() = vec![builtin(), external()];
        manager.poll(10 * SEC);
        manager.poll(10 * SEC + SETTLE_NS);
        assert_eq!(*world.starts.borrow(), vec![(2, 1), (1, 1)]);
    }

    #[test]
    fn a_reconnected_display_is_reported_as_available_again_not_as_new() {
        let world = world_with(vec![builtin(), external()]);
        let mut manager = manager(&world, Follow::All);
        manager.start(0).expect("start");

        world
            .displays
            .borrow_mut()
            .retain(|display| display.id != 2);
        manager.poll(5 * SEC);
        world.displays.borrow_mut().push(external());
        let drafts = manager.poll(9 * SEC);
        assert_eq!(kinds(&drafts), vec![kind::DISPLAY_AWAKE]);
        assert!(
            drafts[0].detail.contains("available again"),
            "{}",
            drafts[0].detail
        );

        // a genuinely new display is still "connected" (and the returning one, settled by now,
        // has started recording)
        world
            .displays
            .borrow_mut()
            .push(display(7, 1920, 1080, 3840.0));
        let drafts = manager.poll(20 * SEC);
        let connected: Vec<_> = drafts
            .iter()
            .filter(|draft| draft.kind == kind::DISPLAY_CONNECTED)
            .collect();
        assert_eq!(connected.len(), 1);
        assert_eq!(connected[0].display, Some(7));
        assert!(kinds(&drafts).contains(&kind::CAPTURE_STARTED));
    }
}
