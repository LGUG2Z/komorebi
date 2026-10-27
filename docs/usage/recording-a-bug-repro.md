# Recording a Bug Repro

`komorebi-recorder` records a bug while it happens: the screen, komorebi's log, every komorebi event and the macOS OS
trace, all on one clock, into a single _session directory_. Afterwards you can ask "what was komorebi doing, and what
was the OS doing, at 00:12.5 in the video?" and the other way around.

```
komorebi-recorder record --duration 30  # record without a window
komorebi-recorder                       # or open the Record / Stop window
```

It is built for the situations where komorebi bugs actually show up: laptops that get docked and undocked, woken with the
lid opened, locked and unlocked. Recording carries on through all of that without losing what was already captured; see
[Docking, sleep and lock](#docking-sleep-and-lock).

## What is captured

| What                    | Where it comes from                                                                                   |
| ----------------------- | ----------------------------------------------------------------------------------------------------- |
| Screen                  | ScreenCaptureKit, one `mp4` per display per contiguous span (a _segment_), with every frame's timestamp |
| komorebi's log          | The exact byte range of `komorebi_plaintext.log.<date>` written during the recording                  |
| komorebi's events       | A subscriber socket: every `WindowManagerEvent` / socket message plus the full `State`                |
| OS trace: komorebi      | `log stream` for komorebi's own process: system frameworks (AX, LaunchServices, ...) as it calls them |
| OS trace: system        | Accessibility, SkyLight, HIServices and workspace subsystems                                          |
| OS trace: each app      | `log stream` for **every process that owns a window in komorebi's state**, one file per app           |
| Recording environment   | Displays connecting, disconnecting and sleeping; machine sleep/wake; screen lock; restarts of komorebi or of a log stream |

The OS trace is what lets you see both seams: komorebi calling into the system, and the system talking to the apps
komorebi is tiling.

## On Windows

The same tool records komorebi for Windows, on the same clock, into the same kind of session
directory; a session recorded on one OS can be investigated on the other. What differs:

| | macOS | Windows |
| --- | --- | --- |
| Screen | ScreenCaptureKit, HEVC | Windows.Graphics.Capture, H.264 (Windows 10 1803+) |
| OS trace | the unified log: komorebi's process, the system, each managed app | WinEvents for every top-level window, folded when repeated, and the System Event Log's warnings and errors and its display, power and GPU providers |
| komorebi's log | its data directory | `%TEMP%\komorebi_plaintext.log.<date>` |
| Permission | Screen Recording, granted to the terminal or app | none |
| Executables | `komorebi-recorder` | `komorebi-recorder.exe` (console: use this to run commands) and `komorebi-recorder-gui.exe` (no console window) |
| Sleep | found from the host clock pausing | heard from Windows as it happens (suspend, resume, lid, displays off, lock) |

Start komorebi with `komorebi.exe --log-level debug` before recording a bug. (`RUST_LOG=debug` also
prints every dependency's log; `RUST_LOG=komorebi=debug` does not.)
`komorebi-recorder doctor` says what the machine can do, and `komorebi-recorder record --no-video` records without the
screen.
The recorder window keeps itself out of the video (Windows 10 version 2004 or later).

## Before you record

- **Screen Recording permission.** macOS attaches the grant to the terminal (or app) that launches the binary, exactly as
  it does for `komorebi` itself. The window shows a "Grant permission" button if it is missing.
- **komorebi's log level.** The recorder never changes komorebi's log level or lifecycle; it only warns. For a useful
  repro, start komorebi with `komorebi --log-level trace` (or `RUST_LOG=komorebi=trace`) first. At the default `info`
  level most of what the window manager _decided_ is not in the log.
- **`ffmpeg`** is only needed for `frames` and `selftest`. It is never used to capture. The `nix develop` shell provides
  it; otherwise `brew install ffmpeg`.

The recorder window is borderless and untitled on purpose: komorebi does not manage windows without a title, so opening
it does not move your layout around while you are trying to reproduce a layout bug. It is also excluded from the video, as is any other `komorebi-recorder` process.
The viewer is one such process.

## Recording

Press **Record**, reproduce the problem, press **Stop**. The window shows live counters, recent events (a display being
unplugged, a wake) and any warnings. The session is written to
`~/Library/Application Support/komorebi/recordings/<UTC timestamp>/` unless you pass `--out`.

With every display ticked, the recorder follows _all_ displays, including ones connected while it is recording. Untick a
display to record only the others.

Headless recording is the same thing without the window; Ctrl-C, `SIGTERM` or `--duration` stops and finalizes it:

```
komorebi-recorder record --out ./repro --duration 60
komorebi-recorder displays            # list display ids
komorebi-recorder record --display 2  # record just one display
```

Useful options:

| Option                                     | Effect                                                                                  |
| ------------------------------------------ | --------------------------------------------------------------------------------------- |
| `--oslog-apps managed\|all-gui\|none`      | Which apps get their own OS trace file (default: those komorebi manages)                |
| `--trace-process NAME` / `--trace-pid PID` | Also trace an app komorebi does not manage                                              |
| `--oslog-exclude NAME`                     | Never trace this (chatty) app                                                           |
| `--oslog-level debug\|info\|default`       | Unified log level (default `debug`)                                                     |
| `--system-process NAME`                    | Add a system daemon to the `system` stream, e.g. `WindowServer` (see volume note below) |
| `--predicate P`                            | Replace the `system` stream's predicate                                                 |
| `--oslog-all`                              | One extra unfiltered system-wide stream (very large)                                    |
| `--no-oslog`, `--no-komorebi`              | Skip the OS trace, or skip touching komorebi entirely                                   |
| `--no-video`                               | Do not capture the screen; record everything else on the same clock (see below)         |
| `--probe-windows`                          | Windows: after each window event, record what Windows says about that window (see below) |
| `--fps N`, `--retina`, `--codec hevc\|h264` | Video settings (30 fps cap, logical points, HEVC by default)                           |

### Marking where the bug is

A bug is marked in one place, the viewer, after the recording: `komorebi-recorder view <session>` opens it, and **Mark
bug** (or **B**) takes a stretch of the recording and what went wrong in it. That marks a _region_, not a single moment.
The recorder window has no button for it, so while recording there is nothing to do but reproduce the problem.

This is what makes the recording useful to whoever investigates it, person or agent: your description is the ground
truth of _what_ went wrong, and everything else in the session is evidence of _why_. See
[Investigating a recording](investigating-a-recording.md#marking-the-bug).

### Without video

`komorebi-recorder record --no-video` records everything but the screen: komorebi's events and log, the OS trace and
what happened to the recording environment, all on the same clock. Use it on a machine with no
display (or no Screen Recording permission), or when what is on the screen must not be recorded. It asks the system
nothing about displays. A session made this way has no video, so video time `00:00.000` is the moment the recording
started, and there is nothing for `frames` to show.

### Probing windows on Windows

komorebi for Windows hides a window by _cloaking_ it where it is, so its state cannot show whether a
window is really hidden. `record --probe-windows` follows each window event with a read of the
window's real state from Windows (visible, cloaked, minimized, where it is) and, now and then, asks
komorebi whether it manages the window and which rules matched. `overview` then compares the two:
a window komorebi shows that Windows has hidden, one it hides that Windows shows, and a visible
window it does not manage (usually correct, with komorebi's explanation). It costs a few system
calls per event and a query to komorebi the first time a window is seen, and it reads nothing but
those facts. It does nothing on a Mac, where a hidden window is parked in a corner and the state
says so.

### Checking your setup

```
komorebi-recorder doctor          # what this machine can do for a recording
komorebi-recorder doctor --json   # the same, for a bug report
```

`doctor` says which komorebi is running (binary, version and commit, log level, whether it answers a state query and
where its log is), whether the screen can be captured and played back, whether `ffmpeg` is there, the displays and
their scale, and the host clock. Anything wrong is listed at the end in plain words. `doctor --probe clock|capture|decode`
measures one thing instead; `--probe clock` shows what the clock does while the machine sleeps (put it to sleep while it
runs).

## Docking, sleep and lock

The set of displays, and whether ScreenCaptureKit streams stay alive, changes underneath a laptop all the time. The
recorder is built so that **nothing already captured is put at risk by any of it**.

- **A display disappears** (unplugged, undocked, lid closed, or it goes to sleep): that display's video file is finalized
  on the spot, so it is complete and playable whatever happens next. The other displays carry on untouched.
- **A display appears or returns:** after a one-second settle, it starts a new _segment_ in a new file
  (`video-<id>-2.mp4`, `-3`, ...). Each segment is a whole file; `session.json` lists every one with its time span.
- **The machine sleeps and wakes:** the wall clock and the host clock are compared many times a second, so a sleep is
  noticed the moment the machine wakes and the clock anchors on either side of it are recorded exactly. Every display is
  then given a fresh stream rather than trusting the old one, because streams can stall across a sleep without reporting
  anything. The host clock pauses while asleep, so video time skips the sleep instead of showing a hole.
- **A stream stalls or dies:** a healthy ScreenCaptureKit stream calls back about thirty times a second even when the
  screen is not changing, so silence for a few seconds means a dead stream. That segment ends at its last sign of life and
  a new one starts.
- **The screen locks, or another user's session comes to the front:** noted, and the displays are watched as above.
- **A `log stream` process dies** (for instance when logd restarts): it is restarted into the same file, appending.
- **komorebi exits or is restarted:** noted, and when it is running again the recorder re-subscribes and traces the new
  process. komorebi not running when recording starts is not an error either.

All of this is recorded in `recorder.jsonl`.
It is shown as the `recorder` source, right next to the log lines and trace around it:

```
komorebi-recorder at <session> 12.5 --source recorder
```

`summary` marks a second in which something changed with `recorder=N`, `locate` says which segment of each display covers
a moment (and which displays had no video then), and `frames` extracts from the right segment.

### If the recorder dies

Everything is written as it arrives and the manifest is checkpointed every few seconds, so a crash, a `kill -9` or a
power loss does not lose the session. Video is written as fragmented mp4, which plays up to the last fragment (about a
second) even though it was never finalized, and a static screen is kept moving so that it, too, is flushed. To recover:

```
komorebi-recorder reindex <session>
```

This closes the segments that were still open (using the video files themselves to decide how much survived), rebuilds
the timeline, and marks the session `recovered`. The queries tell you to do this if you point them at an unfinished
session.

## Asking questions of a session

Times are video timecodes: `12.5`, `00:12.500` or `1:02:03`. Video `00:00.000` is the first captured frame.

```
# everything within +-1s of 12.5s, grouped by source
komorebi-recorder at <session> 12.5

# one app's OS trace only, or one source
komorebi-recorder at <session> 12.5 --app Ghostty
komorebi-recorder at <session> 12.5 --source events

# a span, and a per-second digest of the whole recording
komorebi-recorder range <session> 10 15
komorebi-recorder summary <session>

# reverse: where in the video was this?
komorebi-recorder locate <session> --log-line 120
komorebi-recorder locate <session> --event-seq 7
komorebi-recorder locate <session> --utc 2026-10-26T19:43:35.062613Z

# look at the exact frame
komorebi-recorder frames <session> 12.5 14.0
```

Add `--json` to any query for machine-readable output, which is also what makes a session easy to hand to an LLM: the
generated `README.md` in every session directory explains each file and contains these commands with the session path
filled in.

## The session directory

```
session.json            manifest: versions, displays and every video segment, clock anchors, log segments, streams, apps, warnings
video-<displayID>[-<n>].mp4     screen recording of one segment (variable frame rate: frames exist only where the screen changed)
frames-<displayID>[-<n>].csv    every frame's host-clock timestamp
komorebi.log            exact slice of komorebi's own log
events.jsonl, states/   subscriber notifications; full State stored only when it changed
state-start.json, state-end.json
recorder.jsonl          what happened to the recording environment (displays, sleep/wake, lock, restarts)
annotations.jsonl       bug regions, notes and findings (appended, never rewritten)
oslog/komorebi.ndjson   unified log, komorebi's process (oslog/komorebi-<pid>.ndjson after a restart)
oslog/system.ndjson     unified log, accessibility / SkyLight / workspace subsystems
oslog/apps/<exe>-<pid>.ndjson   unified log, one managed application
timeline.jsonl          merged, sorted index pointing into the raw files
README.md               generated guide to this session
```

Raw files are never rewritten; `timeline.jsonl` only references them, so `komorebi-recorder reindex <session>` can rebuild
it at any time (including after hand-editing a log).

## How accurate is the alignment?

Everything is placed on the host monotonic clock (`mach_absolute_time`). Video frame timestamps and the unified log's
`machTimestamp` are on that clock directly. komorebi's log lines carry UTC wall-clock time and are mapped onto it with
anchors sampled every 5 seconds during the recording and at every sleep, so clock steps and sleep are handled.

- video to OS trace: within about one frame
- video to komorebi's log: within about one frame plus the wall-clock anchor error (tens of microseconds)
- subscriber events are stamped when the recorder receives them. komorebi sends the notification _after_ it has handled
  the event, so the stamp is when the handler **finished**; the `processing event` line in `komorebi.log` is when it
  began.
  `komorebi-recorder trace` ties the two together.

`komorebi-recorder selftest` checks this on your machine: it records a rendered on-screen clock and compares the clock
visible in the video with the time the session predicts. The result is stored in `session.json` under `calibration`.

## Things to know

- **What can still be missing.** Events from a `log stream` during the moment it was dead and being restarted; komorebi
  events while komorebi was not running; the last second or so of video if the recorder is killed rather than stopped; a
  display's video while it was asleep or unplugged (there is simply nothing to record). Each of these leaves a line in
  `recorder.jsonl` saying so, rather than a silent hole.
- **Windows komorebi manages are found through the window server.** komorebi's `State` has no process ids, so the
  recorder resolves window ids to owning processes itself. A process that appears mid-recording gets its own stream as
  soon as it shows up in a state or event; events it emitted in the few hundred milliseconds before that are missed
  (`session.json` records `first_seen_mach_ns` for each app so you know where each file starts).
- **Volume.** Per-process debug streams are modest, but system-wide daemons are not: `launchservices` alone was ~500
  events/s (~29 MB/min) and `runningboard` ~290 events/s on a normal desktop, which is why they are opt-in. komorebi's
  own process emits a few hundred events per second while it is busy. The recorder warns when a stream is abnormal, since
  `log stream` can drop events under extreme load.
- **`<private>` values stay redacted.** The tool needs no `sudo` and changes no SIP settings, and it does not do
  syscall-level tracing.
- **Privacy.** The video contains whatever was on screen and application logs can contain window titles and URLs. Review
  a session before sharing it.
