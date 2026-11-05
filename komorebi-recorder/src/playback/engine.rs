//! Keeping the picture of every display in step with the playhead.
//!
//! A worker thread owns the decoding: the decoder's objects (`AVFoundation`'s on macOS, Media
//! Foundation's on Windows) belong to the thread that made them. It follows the [`Transport`]:
//! whatever the clock says the time is, it makes sure each display's published frame is the one
//! that was on screen then. A display that had no video at that moment publishes a gap.
//!
//! Playing never waits for the decoder to be "ready": position comes from the clock, and if the
//! decoder falls behind, the frame shown is the latest one it has.
//!
//! # How the worker follows the playhead
//!
//! | the playhead | what the worker does |
//! |---|---|
//! | paused, or playing forwards | decodes forwards, holding the next frame until its time is due, and publishes it then |
//! | playing backwards | decodes half a second ending at the playhead, serves it newest first, and decodes the stretch before it once the playhead is a quarter of a second from the start of the frames it has decoded |
//! | moved (any change to the transport, or to the width) | drops the frames it has decoded, opens the video at the new time and publishes the frame that was on screen then |
//! | more than a second and a half from the frame being shown | the same: a jump, not playing along |
//! | on a display with no video at that time | publishes a gap, and goes on publishing it until there is video |
//! | the decoder fails | publishes the failure, drops the frames it has decoded and tries again on its next pass |
//!
//! A pass runs every 3 ms while playing and every 100 ms while paused, and at once when something
//! wakes the worker (every change made through [`Playback`]). After a stream of jumps, such as
//! scrubbing, the published frames end at the last place asked for.
//!
//! # What is shared
//!
//! The worker is handed the manifest and the session path when it starts. After that [`Playback`]
//! and the worker share the transport, the published views and a few counters behind an `Arc`, and
//! a channel of commands (look again, step a frame, quit). [`Playback::version`] counts
//! published views, so a window that redraws when it moves does not have to compare pictures.
//!
//! Covered by the tests in this module that build sessions of numbered videos (`ffmpeg` makes
//! them; without it they skip): `seeking_shows_the_frame_that_was_on_screen_at_that_time`,
//! `playing_forwards_follows_the_clock`, `playing_backwards_runs_the_frames_in_reverse`,
//! `stepping_moves_by_exactly_one_frame`, `a_display_with_no_video_shows_a_gap_and_then_its_frames`,
//! `scrubbing_wildly_ends_on_the_last_place_asked_for` and `a_looped_region_stays_inside_the_region`.

use super::transport::Transport;
use crate::analysis::guard::SessionPath;
use crate::analysis::video::SessionVideo;
use crate::analysis::video::Stream;
use crate::av::Scale;
use crate::session::Manifest;
use color_eyre::eyre;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use crossbeam_channel::bounded;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

/// How much video is decoded at a time to play backwards: short enough to hold in memory at the
/// size it is drawn.
const REVERSE_CHUNK_S: f64 = 0.5;

/// When playing backwards, decode the stretch before the one in hand once the playhead is this
/// close to its start.
const PREFETCH_S: f64 = 0.25;

/// A playhead further than this from the frame being shown is a jump, not playing along.
const JUMP_S: f64 = 1.5;

/// A decoded frame, RGBA, ready to become a texture.
///
/// Published behind an [`Arc`], so a new frame is a new allocation and its address says whether
/// the picture changed.
#[derive(Debug, PartialEq)]
pub struct Frame {
    /// When it was put on screen, in session video time. A published frame is not later than the
    /// playhead it was published for, to within a microsecond.
    pub shown_s: f64,
    /// The width in pixels. Frames are decoded at the width asked of the [`Playback`] (see
    /// [`Playback::set_width`]), keeping the aspect ratio of the recording.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// The pixels as `R, G, B, A` bytes, row by row with no padding between rows: four bytes for
    /// each of `width` times `height` pixels. The alpha of every pixel is `255`, so straight
    /// (unpremultiplied) and premultiplied alpha agree.
    pub rgba: Vec<u8>,
}

/// The picture to draw for a display: the answer of [`Playback::view`].
///
/// A window draws a [`Frame`](DisplayView::Frame) as a picture and every other variant as a
/// placeholder.
#[derive(Debug, Clone)]
pub enum DisplayView {
    /// Nothing decoded yet.
    Loading,
    /// The frame that was on screen at the playhead.
    Frame(Arc<Frame>),
    /// The display had no video at the playhead: unplugged, asleep, or not yet recorded.
    Gap,
    /// Decoding failed. Holds the error with its causes, as the `{:#}` format of an error report
    /// shows them. The worker tries again on its next pass, and a pass that succeeds replaces this
    /// with the frame.
    Failed(String),
}

enum Command {
    /// The transport changed: look again.
    Wake,
    /// Move one frame of `display` forwards (`1`) or backwards (`-1`).
    Step {
        display: u32,
        direction: i32,
    },
    Quit,
}

struct Shared {
    transport: Mutex<Transport>,
    epoch: Instant,
    /// Bumped whenever the transport is changed from outside, so the worker knows its decoding
    /// position may be stale.
    generation: AtomicU64,
    views: Mutex<HashMap<u32, DisplayView>>,
    /// Bumped whenever a view changes: a hint to redraw.
    version: AtomicU64,
    width: AtomicU32,
    stopping: AtomicBool,
}

impl Shared {
    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    fn publish(&self, display: u32, view: DisplayView) {
        self.views.lock().insert(display, view);
        self.version.fetch_add(1, Ordering::Release);
    }
}

/// The playback of one session: a clock and the pictures that go with it.
///
/// A `Playback` owns a [`Transport`] and the thread that decodes for it. Every method takes
/// `&self`. The control methods
/// ([`seek`](Playback::seek), [`play`](Playback::play), [`pause`](Playback::pause),
/// [`toggle`](Playback::toggle), [`set_rate`](Playback::set_rate),
/// [`set_loop`](Playback::set_loop), [`step`](Playback::step) and
/// [`set_width`](Playback::set_width)) change the clock and wake the worker, and the queries
/// ([`position`](Playback::position), [`is_playing`](Playback::is_playing),
/// [`rate`](Playback::rate), [`looping`](Playback::looping), [`view`](Playback::view) and
/// [`version`](Playback::version)) read it. The controls are those of [`Transport`], with the
/// clock supplied (seconds since the `Playback` started), so the rules there hold: positions are
/// clamped, rates are limited to [`MIN_RATE`](super::transport::MIN_RATE) and
/// [`MAX_RATE`](super::transport::MAX_RATE), and a [`seek`](Playback::seek) after the playhead
/// ran into an end sets it moving again.
///
/// Dropping a `Playback` stops the worker and waits for it to finish what it is decoding.
///
/// The decoded pictures are not what is in the video file: they are scaled to the width asked for.
/// Playing needs a decoder, which macOS and Windows have; see [`crate::av::supported`].
///
/// # Example
///
/// Playing a session needs its video on disk, so this compiles but does not run. The behavior is
/// covered by the tests of this module, for example
/// `seeking_shows_the_frame_that_was_on_screen_at_that_time` and
/// `playing_forwards_follows_the_clock`.
///
/// ```rust,no_run
/// use komorebi_recorder::analysis::guard::SessionPath;
/// use komorebi_recorder::playback::{DisplayView, Playback};
/// use komorebi_recorder::session::Session;
/// use std::path::Path;
///
/// let guard = SessionPath::new(Path::new("/recordings/20261026T120000Z"))?;
/// let manifest = Session::open(guard.root())?.manifest()?;
/// let playback = Playback::start(manifest, guard, 1600)?;
///
/// // start playing at normal speed from two seconds in
/// playback.seek(2.0);
/// playback.play(1.0);
///
/// // each time the window is drawn: ask where the playhead is, and what to show
/// let seconds = playback.position();
/// for &display in playback.displays() {
///     match playback.view(display) {
///         DisplayView::Frame(frame) => println!("{display}: {}x{}", frame.width, frame.height),
///         DisplayView::Gap => println!("{display}: no video at {seconds:.1} s"),
///         DisplayView::Loading | DisplayView::Failed(_) => {}
///     }
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Playback {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    worker: Option<JoinHandle<()>>,
    displays: Vec<u32>,
}

fn rgba_of(bgra: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    rgba
}

impl Playback {
    /// Starts the playback of `manifest`'s video, paused at the beginning.
    ///
    /// `guard` is the session directory the video files are read from, and `width` is how wide
    /// decoded frames are wanted, in pixels (at least 64: a smaller value is raised to it). The
    /// duration of the [`Transport`] is the end of [`Manifest::video_span`], and the displays are
    /// those with at least one recorded frame, in ascending order of id and each once.
    ///
    /// A session with no video plays: there is nothing to show, and the clock still runs.
    ///
    /// # Errors
    ///
    /// Fails only if the worker thread cannot be started (the operating system refused a new
    /// thread). A video file that cannot be opened or decoded is not an error here: the display it
    /// belongs to shows [`DisplayView::Failed`].
    ///
    /// [`Manifest::video_span`]: crate::session::Manifest::video_span
    pub fn start(manifest: Manifest, guard: SessionPath, width: u32) -> eyre::Result<Self> {
        let displays: Vec<u32> = {
            let mut ids: Vec<u32> = manifest
                .displays
                .iter()
                .filter(|entry| entry.result.frames > 0)
                .map(|entry| entry.result.display.id)
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };

        let shared = Arc::new(Shared {
            transport: Mutex::new(Transport::new(manifest.video_span().1)),
            epoch: Instant::now(),
            generation: AtomicU64::new(1),
            views: Mutex::new(HashMap::new()),
            version: AtomicU64::new(0),
            width: AtomicU32::new(width.max(64)),
            stopping: AtomicBool::new(false),
        });

        let (commands, inbox) = bounded::<Command>(256);
        let worker_shared = shared.clone();
        let worker_displays = displays.clone();
        let worker = std::thread::Builder::new()
            .name("playback".to_string())
            .spawn(move || run(&worker_shared, &inbox, &manifest, &guard, &worker_displays))?;

        Ok(Self {
            shared,
            commands,
            worker: Some(worker),
            displays,
        })
    }

    /// Returns the ids of the displays that have video, in ascending order.
    ///
    /// These are the displays the worker follows: the ones with at least one recorded frame in the
    /// manifest, in any segment of the recording.
    #[must_use]
    pub fn displays(&self) -> &[u32] {
        &self.displays
    }

    fn changed(&self) {
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        let _ = self.commands.try_send(Command::Wake);
    }

    /// Returns the duration of the recording in session video seconds: the end of the video.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        self.shared.transport.lock().duration_s()
    }

    /// Returns where the playhead is now, in session video time (see [`Transport::position`]).
    #[must_use]
    pub fn position(&self) -> f64 {
        self.shared.transport.lock().position(self.shared.now())
    }

    /// Returns `true` if and only if the playhead is moving now (see [`Transport::is_playing`]).
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.shared.transport.lock().is_playing(self.shared.now())
    }

    /// Returns the rate the playhead plays at, negative for backwards (see [`Transport::rate`]).
    #[must_use]
    pub fn rate(&self) -> f64 {
        self.shared.transport.lock().rate()
    }

    /// Returns the loop region, if one is set (see [`Transport::looping`]).
    #[must_use]
    pub fn looping(&self) -> Option<(f64, f64)> {
        self.shared.transport.lock().looping()
    }

    /// Moves the playhead to `time` and wakes the worker, which publishes the frame that was on
    /// screen then (see [`Transport::seek`]).
    ///
    /// The frame arrives later, not before this returns: watch [`Playback::view`] or
    /// [`Playback::version`].
    pub fn seek(&self, time: f64) {
        let now = self.shared.now();
        self.shared.transport.lock().seek(time, now);
        self.changed();
    }

    /// Plays at `rate` from where the playhead is: negative for backwards (see
    /// [`Transport::play`]).
    pub fn play(&self, rate: f64) {
        let now = self.shared.now();
        self.shared.transport.lock().play(rate, now);
        self.changed();
    }

    /// Stops the playhead where it is (see [`Transport::pause`]).
    pub fn pause(&self) {
        let now = self.shared.now();
        self.shared.transport.lock().pause(now);
        self.changed();
    }

    /// Plays if the playhead is not moving, and pauses if it is (see [`Transport::toggle`]).
    pub fn toggle(&self) {
        let now = self.shared.now();
        self.shared.transport.lock().toggle(now);
        self.changed();
    }

    /// Changes the speed without moving the playhead or starting it (see
    /// [`Transport::set_rate`]).
    pub fn set_rate(&self, rate: f64) {
        let now = self.shared.now();
        self.shared.transport.lock().set_rate(rate, now);
        self.changed();
    }

    /// Sets the region to repeat while playing, or clears it with `None` (see
    /// [`Transport::set_loop`]).
    pub fn set_loop(&self, region: Option<(f64, f64)>) {
        let now = self.shared.now();
        self.shared.transport.lock().set_loop(region, now);
        self.changed();
    }

    /// Pauses and moves the playhead one frame of `display` forwards or backwards.
    ///
    /// A positive `direction` steps forwards; zero and negative values step backwards. The worker
    /// moves the playhead to the time the neighboring frame was put on screen, so what the display
    /// shows afterwards is exactly that frame. If `display` has no frame showing (it is loading, in
    /// a gap, has failed, or is not one of the [`displays`](Playback::displays)) or has no frame in
    /// that direction, the playhead does not move, but it is still paused.
    pub fn step(&self, display: u32, direction: i32) {
        self.pause();
        let _ = self.commands.try_send(Command::Step {
            display,
            direction: direction.signum(),
        });
    }

    /// Changes how wide decoded frames are wanted, in pixels, with at least 64 (a smaller value is
    /// raised to it).
    ///
    /// The worker decodes again at the playhead, so the frames that follow have the new width.
    pub fn set_width(&self, width: u32) {
        self.shared.width.store(width.max(64), Ordering::Release);
        self.changed();
    }

    /// Returns what to draw for `display` now.
    ///
    /// It is [`DisplayView::Loading`] until the worker has published something for `display`,
    /// which includes any id that is not one of the [`displays`](Playback::displays).
    #[must_use]
    pub fn view(&self, display: u32) -> DisplayView {
        self.shared
            .views
            .lock()
            .get(&display)
            .cloned()
            .unwrap_or(DisplayView::Loading)
    }

    /// Returns a counter that increases whenever a view is published for any display; redraw when
    /// it moves.
    ///
    /// The counter is a hint that something may have changed. It does not say what: ask
    /// [`Playback::view`].
    #[must_use]
    pub fn version(&self) -> u64 {
        self.shared.version.load(Ordering::Acquire)
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        let _ = self.commands.try_send(Command::Quit);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Where the decoding of one display has got to.
struct Cursor {
    display: u32,
    generation: u64,
    width: u32,
    /// The frame published now.
    current: Option<Arc<Frame>>,
    /// Forwards: the next frame, decoded but not due yet.
    ahead: Option<Arc<Frame>>,
    stream: Option<Stream>,
    /// Backwards: a short stretch decoded, oldest first.
    behind: Vec<Arc<Frame>>,
    gap: bool,
}

impl Cursor {
    fn new(display: u32) -> Self {
        Self {
            display,
            generation: 0,
            width: 0,
            current: None,
            ahead: None,
            stream: None,
            behind: Vec::new(),
            gap: false,
        }
    }

    fn forget(&mut self) {
        self.stream = None;
        self.ahead = None;
        self.behind.clear();
    }
}

fn frame_of(shown_s: f64, frame: &crate::av::Frame) -> Arc<Frame> {
    Arc::new(Frame {
        shown_s,
        width: frame.width,
        height: frame.height,
        rgba: rgba_of(&frame.bgra),
    })
}

/// Decode forwards from `time`: the frame on screen then becomes the current one and the next is
/// held ahead.
fn restart_forwards(
    cursor: &mut Cursor,
    video: &SessionVideo,
    shared: &Shared,
    time: f64,
) -> eyre::Result<()> {
    cursor.forget();
    let Some(mut stream) = video.open_at(cursor.display, time, None, Scale::Width(cursor.width))?
    else {
        cursor.current = None;
        cursor.gap = true;
        shared.publish(cursor.display, DisplayView::Gap);
        return Ok(());
    };

    cursor.gap = false;
    if let Some((shown_s, frame)) = stream.next_frame()? {
        let first = frame_of(shown_s, &frame);
        cursor.current = Some(first.clone());
        shared.publish(cursor.display, DisplayView::Frame(first));
    }
    cursor.ahead = stream
        .next_frame()?
        .map(|(shown_s, frame)| frame_of(shown_s, &frame));
    cursor.stream = Some(stream);
    Ok(())
}

/// Decode `REVERSE_CHUNK_S` of video ending at `until`, oldest first, for playing backwards.
fn fill_behind(cursor: &mut Cursor, video: &SessionVideo, until: f64) -> eyre::Result<()> {
    let from = (until - REVERSE_CHUNK_S).max(0.0);
    cursor.behind.clear();
    let Some(mut stream) = video.open_at(
        cursor.display,
        from,
        Some(until),
        Scale::Width(cursor.width),
    )?
    else {
        return Ok(());
    };

    while let Some((shown_s, frame)) = stream.next_frame()? {
        cursor.behind.push(frame_of(shown_s, &frame));
    }
    Ok(())
}

/// Everything `follow` needs to know about the moment it is following.
struct Moment {
    time: f64,
    playing: bool,
    rate: f64,
    generation: u64,
    width: u32,
}

/// Make the published frame of `cursor` the one on screen at `moment.time`.
fn follow(
    cursor: &mut Cursor,
    video: &SessionVideo,
    shared: &Shared,
    moment: &Moment,
) -> eyre::Result<()> {
    let time = moment.time;
    let resynchronize = cursor.generation != moment.generation || cursor.width != moment.width;
    cursor.generation = moment.generation;
    cursor.width = moment.width;

    // a display with no video at this moment shows a gap, and goes on showing one
    if !video.has_video_at(cursor.display, time) {
        if !cursor.gap {
            cursor.forget();
            cursor.current = None;
            cursor.gap = true;
            shared.publish(cursor.display, DisplayView::Gap);
        }
        return Ok(());
    }

    if moment.playing && moment.rate < 0.0 {
        // backwards: serve frames out of a decoded stretch, decoding the stretch before it when
        // the playhead runs off the start of it
        let needs_fill = resynchronize
            || cursor.gap
            || cursor
                .behind
                .first()
                .is_none_or(|first| time < first.shown_s)
            || cursor
                .behind
                .last()
                .is_some_and(|last| time > last.shown_s + JUMP_S);
        if needs_fill {
            cursor.stream = None;
            cursor.ahead = None;
            cursor.gap = false;
            fill_behind(cursor, video, time)?;
        } else if let Some(first) = cursor.behind.first().map(|first| first.shown_s)
            && time - first < PREFETCH_S
            && first > 0.0
        {
            // running low: decode the stretch before this one now, so it is there when needed
            let mut earlier = Vec::new();
            if let Some(mut stream) = video.open_at(
                cursor.display,
                (first - REVERSE_CHUNK_S).max(0.0),
                Some(first),
                Scale::Width(cursor.width),
            )? {
                while let Some((shown_s, frame)) = stream.next_frame()? {
                    if shown_s < first - 1e-6 {
                        earlier.push(frame_of(shown_s, &frame));
                    }
                }
            }
            earlier.append(&mut cursor.behind);
            cursor.behind = earlier;
            // what is already behind the playhead's direction of travel is no longer wanted
            cursor
                .behind
                .retain(|frame| frame.shown_s <= time + REVERSE_CHUNK_S * 2.0);
        }

        let wanted = cursor
            .behind
            .iter()
            .rev()
            .find(|frame| frame.shown_s <= time + 1e-6)
            .cloned();
        if let Some(wanted) = wanted
            && cursor
                .current
                .as_ref()
                .is_none_or(|current| (current.shown_s - wanted.shown_s).abs() > 1e-9)
        {
            cursor.current = Some(wanted.clone());
            shared.publish(cursor.display, DisplayView::Frame(wanted));
        }
        return Ok(());
    }

    // forwards or paused: carry on from where the decoder is, unless the playhead jumped
    let can_continue =
        !resynchronize && !cursor.gap && cursor.stream.is_some() && cursor.current.is_some();
    if can_continue {
        let current_s = cursor
            .current
            .as_ref()
            .map_or(0.0, |current| current.shown_s);
        let ahead_due = cursor
            .ahead
            .as_ref()
            .is_some_and(|ahead| ahead.shown_s <= time + 1e-6);
        let far_ahead = ahead_due
            && cursor
                .ahead
                .as_ref()
                .is_some_and(|ahead| time - ahead.shown_s > JUMP_S);
        let went_back = current_s > time + 1e-6;

        if !far_ahead && !went_back {
            let mut advanced = false;
            while cursor
                .ahead
                .as_ref()
                .is_some_and(|ahead| ahead.shown_s <= time + 1e-6)
            {
                cursor.current = cursor.ahead.take();
                advanced = true;
                cursor.ahead = match cursor.stream.as_mut() {
                    Some(stream) => stream
                        .next_frame()?
                        .map(|(shown_s, frame)| frame_of(shown_s, &frame)),
                    None => None,
                };
            }
            if advanced && let Some(current) = &cursor.current {
                shared.publish(cursor.display, DisplayView::Frame(current.clone()));
            }
            return Ok(());
        }
    }

    restart_forwards(cursor, video, shared, time)
}

/// The worker.
fn run(
    shared: &Shared,
    inbox: &Receiver<Command>,
    manifest: &Manifest,
    guard: &SessionPath,
    displays: &[u32],
) {
    let video = SessionVideo::new(manifest, guard);
    let mut cursors: Vec<Cursor> = displays
        .iter()
        .map(|display| Cursor::new(*display))
        .collect();

    loop {
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }

        // commands first: a step moves the transport, which the rest of this pass then follows
        let mut quit = false;
        while let Ok(command) = inbox.try_recv() {
            match command {
                Command::Quit => quit = true,
                Command::Wake => {}
                Command::Step { display, direction } => {
                    if let Some(cursor) =
                        cursors.iter_mut().find(|cursor| cursor.display == display)
                    {
                        step(cursor, &video, shared, direction);
                    }
                }
            }
        }
        if quit {
            return;
        }

        let now = shared.now();
        let (time, playing, rate) = {
            let transport = shared.transport.lock();
            (
                transport.position(now),
                transport.is_playing(now),
                transport.rate(),
            )
        };
        let moment = Moment {
            time,
            playing,
            rate,
            generation: shared.generation.load(Ordering::Acquire),
            width: shared.width.load(Ordering::Acquire),
        };

        for cursor in &mut cursors {
            if let Err(error) = follow(cursor, &video, shared, &moment) {
                cursor.forget();
                cursor.current = None;
                shared.publish(cursor.display, DisplayView::Failed(format!("{error:#}")));
            }
        }

        // playing is paced finely; paused, there is nothing to do until something changes
        let wait = if playing {
            Duration::from_millis(3)
        } else {
            Duration::from_millis(100)
        };
        match inbox.recv_timeout(wait) {
            Ok(Command::Quit) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            Ok(Command::Step { display, direction }) => {
                if let Some(cursor) = cursors.iter_mut().find(|cursor| cursor.display == display) {
                    step(cursor, &video, shared, direction);
                }
            }
            Ok(Command::Wake) | Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Move the playhead to the frame after (or before) the one `cursor` shows.
fn step(cursor: &mut Cursor, video: &SessionVideo, shared: &Shared, direction: i32) {
    let Some(shown) = cursor.current.as_ref().map(|frame| frame.shown_s) else {
        return;
    };

    let target = if direction > 0 {
        // the next frame is the one held ahead, or the first after the current if nothing is
        let mut stream =
            match video.open_at(cursor.display, shown, None, Scale::Width(cursor.width)) {
                Ok(Some(stream)) => stream,
                _ => return,
            };
        let mut found = None;
        while let Ok(Some((time, _))) = stream.next_frame() {
            if time > shown + 1e-6 {
                found = Some(time);
                break;
            }
        }
        found
    } else {
        // the last frame before the current one, from the stretch of video just before it
        let from = (shown - REVERSE_CHUNK_S).max(0.0);
        let mut stream = match video.open_at(
            cursor.display,
            from,
            Some(shown),
            Scale::Width(cursor.width),
        ) {
            Ok(Some(stream)) => stream,
            _ => return,
        };
        let mut previous = None;
        while let Ok(Some((time, _))) = stream.next_frame() {
            if time < shown - 1e-6 {
                previous = Some(time);
            }
        }
        previous
    };

    if let Some(target) = target {
        let now = shared.now();
        shared.transport.lock().seek(target, now);
        shared.generation.fetch_add(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;
    use crate::testkit::SessionBuilder;
    use std::time::Instant;

    const FPS: u32 = 30;

    /// A session with display 1 numbered 0..90 at 30 fps and display 2 only from 1.0 s to 3.0 s.
    fn playback(name: &str) -> Option<Playback> {
        if !testkit::ffmpeg_available() {
            eprintln!("skipped: ffmpeg is not available to make a test video");
            return None;
        }

        let (session, manifest) = SessionBuilder::new(name)
            .video(1, 0.0, 90, FPS)
            .video(2, 1.0, 60, FPS)
            .build();
        let guard = SessionPath::new(&session.dir).expect("guard");
        Some(Playback::start(manifest, guard, 128).expect("start"))
    }

    /// Wait until `check` holds, up to a few seconds.
    fn eventually(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if check() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    /// Which frame number a display is showing, if it is showing one.
    fn shown(playback: &Playback, display: u32) -> Option<u32> {
        match playback.view(display) {
            DisplayView::Frame(frame) => Some(testkit::numbered_frame_index(
                &frame.rgba,
                frame.width,
                frame.height,
            )),
            _ => None,
        }
    }

    #[test]
    fn seeking_shows_the_frame_that_was_on_screen_at_that_time() {
        let Some(playback) = playback("playback-seek") else {
            return;
        };

        for (time, expected) in [(1.0, 30), (0.5, 15), (2.5, 75), (0.0, 0)] {
            playback.seek(time);
            eventually(&format!("frame {expected} at {time}"), || {
                shown(&playback, 1) == Some(expected)
            });
        }
        assert!(!playback.is_playing());
    }

    #[test]
    fn playing_forwards_follows_the_clock() {
        let Some(playback) = playback("playback-forward") else {
            return;
        };

        playback.seek(0.5);
        eventually("the first frame", || shown(&playback, 1) == Some(15));
        // slowly, so that how busy the machine is cannot decide whether it catches up
        playback.play(0.25);
        assert!(playback.is_playing());

        // the picture moves on, and catches up with the clock: it is the frame for the time (to
        // within a few frames of decode lag) and never a frame from the future
        eventually("the picture to advance", || {
            shown(&playback, 1).is_some_and(|frame| frame > 15)
        });
        eventually("the picture to be the one for the time", || {
            let position = playback.position();
            let expected = (position * f64::from(FPS)) as i64;
            shown(&playback, 1).is_some_and(|frame| {
                let frame = i64::from(frame);
                position < 2.9 && frame <= expected + 1 && expected - frame <= 6
            })
        });

        playback.pause();
        assert!(!playback.is_playing());
        let held = playback.position();
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            (playback.position() - held).abs() < 1e-9,
            "paused means still"
        );
    }

    #[test]
    fn playing_backwards_runs_the_frames_in_reverse() {
        let Some(playback) = playback("playback-reverse") else {
            return;
        };

        playback.seek(2.0);
        eventually("frame 60", || shown(&playback, 1) == Some(60));
        playback.play(-0.25);

        eventually("the picture to go back", || {
            shown(&playback, 1).is_some_and(|frame| frame < 60)
        });
        // and to be the frame for the time: at or before the playhead, close behind it in time
        eventually("the picture to be the one for the time", || {
            let position = playback.position();
            let expected = (position * f64::from(FPS)) as i64;
            shown(&playback, 1).is_some_and(|frame| {
                let frame = i64::from(frame);
                position > 0.2 && frame <= expected + 1 && expected - frame <= 6
            })
        });

        // it keeps going back past the stretch it started with
        let first = shown(&playback, 1).expect("a frame");
        eventually("the picture to keep going back", || {
            shown(&playback, 1).is_some_and(|frame| frame + 20 < first)
        });
    }

    #[test]
    fn stepping_moves_by_exactly_one_frame() {
        let Some(playback) = playback("playback-step") else {
            return;
        };

        playback.seek(1.0);
        eventually("frame 30", || shown(&playback, 1) == Some(30));

        playback.step(1, 1);
        eventually("frame 31", || shown(&playback, 1) == Some(31));
        assert!((playback.position() - 31.0 / f64::from(FPS)).abs() < 0.002);

        playback.step(1, -1);
        eventually("frame 30 again", || shown(&playback, 1) == Some(30));
        playback.step(1, -1);
        eventually("frame 29", || shown(&playback, 1) == Some(29));
        assert!(!playback.is_playing());
    }

    #[test]
    fn a_display_with_no_video_shows_a_gap_and_then_its_frames() {
        let Some(playback) = playback("playback-gap") else {
            return;
        };

        playback.seek(0.5);
        eventually("a gap on display 2", || {
            matches!(playback.view(2), DisplayView::Gap)
        });
        eventually("display 1's frame", || shown(&playback, 1) == Some(15));

        playback.seek(1.5);
        // display 2 starts at 1.0 s, so 1.5 s is its 15th frame
        eventually("display 2's frame", || shown(&playback, 2) == Some(15));
        eventually("display 1's frame", || shown(&playback, 1) == Some(45));

        playback.seek(3.5);
        eventually("a gap on display 2 again", || {
            matches!(playback.view(2), DisplayView::Gap)
        });
    }

    #[test]
    fn scrubbing_wildly_ends_on_the_last_place_asked_for() {
        let Some(playback) = playback("playback-scrub") else {
            return;
        };

        for index in 0..60 {
            playback.seek(f64::from(index % 9) * 0.3);
        }
        playback.seek(2.2);

        eventually("frame 66", || shown(&playback, 1) == Some(66));
        // and it stays there
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(shown(&playback, 1), Some(66));
    }

    #[test]
    fn a_looped_region_stays_inside_the_region() {
        let Some(playback) = playback("playback-loop") else {
            return;
        };

        playback.seek(1.0);
        playback.set_loop(Some((1.0, 1.5)));
        playback.play(1.0);

        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(1400) {
            let position = playback.position();
            assert!(
                (1.0..=1.5).contains(&position),
                "position {position} left the loop"
            );
            if let Some(frame) = shown(&playback, 1) {
                assert!(
                    (28..=47).contains(&frame),
                    "frame {frame} is outside the loop"
                );
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        assert!(playback.is_playing());
    }

    #[test]
    fn a_session_with_no_video_has_nothing_to_play_and_does_not_fail() {
        let (session, manifest) = SessionBuilder::new("playback-none").build();
        let guard = SessionPath::new(&session.dir).expect("guard");
        let playback = Playback::start(manifest, guard, 128).expect("start");

        assert!(playback.displays().is_empty());
        playback.play(1.0);
        playback.seek(2.0);
        assert!(playback.position() >= 2.0);
    }
}
