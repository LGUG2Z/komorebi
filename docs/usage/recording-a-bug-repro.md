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

The recorder window is borderless, and on Windows is named `komorebi-recorder` only once komorebi is told to ignore it:
at start the recorder runs `komorebic ignore-rule exe komorebi-recorder.exe` (and the same for
`komorebi-recorder-gui.exe`, which opens the viewer) against the running komorebi, a live rule that is not saved. If
komorebi is not running the window is named too; if `komorebic` is not found or fails, and on a Mac, the window stays
untitled, because komorebi does not manage windows without a title, and the recorder says so on standard error and in
the window. Either way opening it does not move your layout around while you are trying to reproduce a layout bug.
`komorebi-recorder doctor` shows which you will get. To keep the rule across komorebi restarts, add this to your own
komorebi configuration (the entries below are in the shape of the application-specific configuration; in `komorebi.json`
the same match is an `ignore_rules` entry with `kind: Exe`, `id: komorebi-recorder.exe`, `matching_strategy: Equals`):

```json
  "komorebi-recorder": {
    "ignore": [
      {
        "kind": "Exe",
        "id": "komorebi-recorder.exe",
        "matching_strategy": "Equals"
      }
    ]
  },
  "komorebi-recorder-gui": {
    "ignore": [
      {
        "kind": "Exe",
        "id": "komorebi-recorder-gui.exe",
        "matching_strategy": "Equals"
      }
    ]
  }
```
 It is also excluded from the video, as is any other `komorebi-recorder` process.
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

To record exactly while a script reproduces the bug, give the command after `--` with `--until-exit`:

```
komorebi-recorder record --until-exit --out ./repro -- ./repro.sh --slow
```

The recorder starts the command once the recording is under way, and stops the recording when the command exits. If
`--duration` or Ctrl-C comes first, that stops the recording and the command is terminated, so whichever comes first
wins. The command runs in the directory you are in, with this terminal's input and output, and has
`KOMOREBI_RECORDER_SESSION` in its environment: the session directory (an absolute path) that is being recorded, so
that anything the script runs can mark the moment with `komorebi-recorder mark` or annotate the session without being
told where it is. On Windows the command is terminated with `TerminateProcess`, which does not reach programs it
started.

The exit status of `komorebi-recorder record` is about the recording, never the command: it is 0 when the session was
finalised, whatever status the command exited with. The command's status is in the summary
(`command: ... exited with status 3`) and in `record --json` as `data.child` (`end`: `exited`, `stopped`,
`failed-to-start` or `not-started`; `exit_code`; `success`). `--json` on `record` prints the `record/1` envelope on
standard output (progress stays on standard error): the session directory, the duration, each display's frames, the
recorder's warnings and the command's status. `displays`, `reindex` and `selftest` take `--json` too.

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
| `--window-track[=HZ]`                      | Also sample every window's rect and state alongside the video (30 Hz by default, 1 to 120; see below) |
| `--no-probe-windows`                       | Windows: do not record what Windows says about each window event, which is on by default (see below) |
| `--fps N`, `--retina`, `--codec hevc\|h264` | Video settings (display-rate capture (`--fps 0`, the default; N caps it), logical points, HEVC by default)                           |

### Marking where the bug is

A bug is marked in one place, the viewer, after the recording: `komorebi-recorder view <session>` opens it, and **Mark
bug** (or **B**) takes a stretch of the recording and what went wrong in it. That marks a _region_, not a single moment.
The recorder window has no button for it, so while recording there is nothing to do but reproduce the problem.

This is what makes the recording useful to whoever investigates it, person or agent: your description is the ground
truth of _what_ went wrong, and everything else in the session is evidence of _why_. See
[Investigating a recording](investigating-a-recording.md#marking-the-bug).

### Marking steps from a script

A script that performs the repro can drop named markers onto the timeline while the recording is going:

```
komorebi-recorder mark "step 3: move window right"
```

A marker is a note by the author `script`, written at the moment the command runs, so it appears in `annotations`,
`brief`, `search`, `overview` and as a tick on the viewer's bug lane. `mark` prints the video time (`marked 00:03.28 (a-3f9c1e02): ...`) once the first frame has been captured and
the wall time (`marked 12:00:08.12Z (...)`) before that.

It marks the session named by `--session DIR`, else by the environment variable `KOMOREBI_RECORDER_SESSION` (which
`record --until-exit` sets for the command it runs), else the newest recording in komorebi's data directory that is
still running. A recording that has finished is refused: it has no "now" to mark; use `annotate <session> add` with a
time instead. `--json` prints the `mark/1` envelope.

### Checking the result from a script

`assert` turns the recording into a pass or fail, so the same script that reproduces the bug can end by saying whether
it happened. The exit status is the result: 0 when every check holds, 6 when one does not, 4 when one could not be
measured (for example `motion` on a recording without video) and 2 when a check is malformed.

```
komorebi-recorder record --until-exit --out ./repro -- ./repro.sh   # repro.sh calls `komorebi-recorder mark` between steps
komorebi-recorder assert ./repro   --check "annotations kind=note count>=3"   --check "findings kind=layout-mismatch count<=0"   --check "log \"level=ERROR\" count==0"   --check "blank region=0,0,800,600 color=white ms<=100"
```

```
PASS annotations kind=note count>=3 (observed 4)
FAIL findings kind=layout-mismatch count<=0 (observed 2)
     f-3a9c1e invariant:layout-mismatch at 00:03.280: ...
PASS log "level=ERROR" count==0 (observed 0)
PASS blank region=0,0,800,600 color=white ms<=100 (observed 0)
1 of 4 check(s): 1 failed
```

The first check confirms the script's `mark`s were recorded. A check is a kind, its arguments and a comparison
(`==  !=  <=  >=  <  >`); the kinds are `findings`, `log` (a `logq` query), `motion`, `blank` and `annotations`, and
times in `from=` and `to=` are any time the other commands take, including `event:N` and `bug:<id>`. `--json` prints the
`assert/1` envelope. `assert` only reads the session.

### Without video

`komorebi-recorder record --no-video` records everything but the screen: komorebi's events and log, the OS trace and
what happened to the recording environment, all on the same clock. Use it on a machine with no
display (or no Screen Recording permission), or when what is on the screen must not be recorded. It asks the system
nothing about displays. A session made this way has no video, so video time `00:00.000` is the moment the recording
started, and there is nothing for `frames` to show.

### Probing windows on Windows

komorebi for Windows hides a window by _cloaking_ it where it is, so its state cannot show whether a
window is really hidden. So on Windows `record` follows each window event with a read of the
window's real state from Windows (visible, cloaked, minimized, where it is) and, now and then, asks
komorebi whether it manages the window and which rules matched. `overview` then compares the two:
a window komorebi shows that Windows has hidden, one it hides that Windows shows, and a visible
window it does not manage (usually correct, with komorebi's explanation). It costs a few system
calls per event and a query to komorebi the first time a window is seen, and it reads nothing but
those facts. `--no-probe-windows` turns it off, and without it `overview` cannot say whether a window is
really hidden, nor find a ghost tile. It does nothing on a Mac, where a hidden window is parked in a
corner and the state says so, and asking for `--probe-windows` there is an error.

### Tracking where every window is

The per-event probe above says what one window looked like when something happened. The recurring
question is different: _which window was where, in this frame_. `record --window-track` (or
`--window-track=60`) lists every top-level window many times a second, with its rect, whether it is
visible or minimized and, on Windows, whether it is cloaked, front to back, into `windows.jsonl`. A
row is written only when something changed, plus a full row each second, so a still desktop costs
about one row a second and a busy one a few kilobytes a second. The recorder's own windows and tool
windows (tooltips, toasts) are left out. It is off unless asked for. `komorebi-recorder sheet
<session> --windows` then draws each window's rect, labelled with its program and id, on every
frame of the sheet; a session with no track says so and gets a plain sheet. A recording that cannot
keep the rate says so in its warnings.

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

Times are video timecodes: `12.5`, `00:12.500` or `1:02:03`, or a time from komorebi's log such as
`logtime:15:46:22.743` (UTC), or `wall:...`, `event:12`. Video `00:00.000` is the first captured frame.

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
windows.jsonl           window track (only with --window-track): every window's rect, on change and once a second
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
