# Platform support

This document gathers, in one place, what differs by operating system. Most of the details are written down on the items
that implement them; here the same questions are answered once per host, so that someone who has to run the recorder on
a machine, or change it for one, does not have to read the code to find out.

The recorder is built for macOS, Windows and Linux (KDE Plasma). A fourth host, `unsupported`, builds on anything else so
that the commands that only read a session directory work there; recording says that it cannot.

## Status

| host | built | run |
|---|---|---|
| macOS (Apple silicon) | yes | yes: written and run on macOS 26 |
| Windows x86_64 | yes, natively on `windows-latest` and cross-compiled from a Mac | no: type-checked and cross-built only |
| Windows ARM64 | yes, cross-built on an x86_64 runner | no |
| Linux (KDE Plasma, x86_64) | yes, natively | decoding, the komorebi side of recording and the OS trace were run, with komorebi for KDE's in-memory desktop; the screencast was not: see [kde-checkpoints.md](dev/kde-checkpoints.md) |
| anything else | the `unsupported` host builds | nothing records |

Nothing that touches a real Windows desktop has been seen running: the Windows host was written from the Windows API
documentation and komorebi for Windows' source, and every claim below about Windows says what the code does, not what
was observed.
[windows-checkpoints.md](dev/windows-checkpoints.md) lists the checks that turn each into an observation.

## Vocabulary

- **Host**: the OS the recorder runs on. It decides how the screen is captured, which clock is read, how the OS says it
  is going to sleep and what the OS trace is made of.
- **Flavor**: which komorebi is being recorded. There is one per OS, and a recorder watches the one on its own host, but
  a session records its flavor, so a session from either can be read on either host.
- **Master clock**: the host's monotonic clock, in nanoseconds, on which every record in a session is stamped.
- **Session**: the directory a recording is written to.
- **Segment**: one contiguous stretch of one display's video, a file of its own. A display that is unplugged and plugged
  in again has two.

## The same questions, per host

### The master clock

| | macOS | Windows | unsupported |
|---|---|---|---|
| clock | `mach_absolute_time`, scaled by `mach_timebase_info` | `QueryPerformanceCounter`, scaled by `QueryPerformanceFrequency` (10 MHz when Windows will not say) | the time since the first reading in the process, plus one second so that no reading is zero |
| `host.clock` in the manifest | `mach` | `qpc` | `std` |
| counts the time the machine sleeps | no | not known: measured, not assumed | no |
| other clocks read beside it | `mach_continuous_time`, which counts sleep | `QueryInterruptTime`, which counts sleep, and `QueryUnbiasedInterruptTimePrecise`, which does not | none |

Video frame times and the OS trace are on the master clock already on both hosts (ScreenCaptureKit and the unified log's
`machTimestamp` on a Mac; Windows.Graphics.Capture on Windows). The field names ending in `_mach_ns` are spelled that way
on every host: see [the architecture](ARCHITECTURE.md#the-master-clock).

### Noticing sleep, wake, lock and display changes

| | macOS | Windows |
|---|---|---|
| a sleep | found afterwards: the master clock pauses while the wall clock does not, which the recording loop sees by comparing the two many times a second | heard as it happens, from a hidden window on a thread of its own that receives suspend and resume; the lid, the displays' power state and session lock are registered for |
| the screen locked | polled: CoreGraphics' session dictionary | polled: being refused the input desktop means a lock screen or a UAC prompt, and the screen cannot be captured then either way |
| displays added, removed, resized | polled: `CGGetActiveDisplayList` and `CGDisplayIsAsleep`, which are cheap and need no permission | polled, and heard as a display-change message |

Windows is told when the lock happens as well as polling for it. What is done about each of these, on both hosts, is the
same: a display that is gone has its video finalized at once, a display that returns starts a new segment, and every
stream is started afresh after a wake instead of trusting the old one.

### Capturing the screen

| | macOS | Windows |
|---|---|---|
| API | ScreenCaptureKit, through `objc2` bindings, and AVFoundation's `AVAssetWriter` | Windows.Graphics.Capture (Windows 10 version 1803 or later), and a Media Foundation MP4 sink |
| permission | Screen Recording, granted to the terminal or app that launches the binary | none |
| default codec | HEVC | H.264 |
| other codec | H.264 (`--codec h264`) | HEVC is refused with an error: Media Foundation's MP4 sink does not write it |
| what is captured | each display, excluding the recorder's own processes by name | each monitor whole, so the program's windows ask to be left out (see below) |
| a still screen | a healthy stream still calls back about 30 times a second, so silence for four seconds means a stalled stream | Windows sends nothing while the picture does not change, so silence says nothing there |

The video of a display is written as fragmented MP4 on both hosts, in segments, so that the part written before a crash
can still be played. Because ScreenCaptureKit only emits frames when the screen changes, the macOS capture re-submits the
last frame so that a static display still flushes fragments.

### The OS trace

| | macOS | Windows |
|---|---|---|
| source | the unified log, through `log stream --style ndjson` child processes | WinEvents for every top-level window, through a hook, and the System Event Log |
| files | `oslog/komorebi.ndjson`, `oslog/system.ndjson` and one `oslog/apps/<exe>-<pid>.ndjson` per managed application | `oslog/system.ndjson` (the WinEvents, repeats folded) and `oslog/eventlog.ndjson` (warnings, errors and the display, power and GPU providers) |
| permissions | none: no `sudo` and no change to SIP | none |
| redaction | `<private>` values stay redacted | not applicable |

A host with no OS trace says so in the recording instead of failing to start streams that do not exist.

### Locking a session

A recording holds an exclusive lock on `recorder.lock` in its session for as long as it runs, and writes its process id
to `recorder.pid`. The OS drops the lock when the process dies, however it dies, so a lock that is held means a live
recorder and one that is free means there is none. Windows file locks are mandatory, which is why the pid is not kept in
the locked file.

### komorebi's log and how the recorder talks to it

| | komorebi for Mac | komorebi for Windows |
|---|---|---|
| data directory | `~/Library/Application Support/komorebi` | `%LOCALAPPDATA%\komorebi` |
| log | `komorebi_plaintext.log.<UTC date>` in the data directory | `%TEMP%\komorebi_plaintext.log.<UTC date>` |
| command and subscriber sockets | unix domain sockets in the data directory | unix domain sockets in the data directory, through `uds_windows` |
| `--log-level debug` | `komorebi --log-level debug` | `komorebi.exe --log-level debug`; `RUST_LOG=debug` also prints every dependency, so name the crate: `RUST_LOG=komorebi=debug` |

The wire protocol is the same on both. The recorder's own binary does not depend on either komorebi's crates.

### Decoding the recorded video

macOS decodes with AVFoundation's `AVAssetReader`, Windows with Media Foundation's Source Reader, and the `unsupported`
host with nothing: it says so when asked to open a file. Both real hosts are asked for BGRA frames at a chosen size, so
that a contact sheet never decodes more pixels than it shows.

### The window track

`record` samples every top-level window (on by default; `--window-track=HZ` picks the rate, `--no-window-track` turns it off) into `windows.jsonl` (see
`src/sources/windowtrack.rs`), listed by `platform::window::list`.

| | macOS | Windows |
|---|---|---|
| source | `CGWindowListCopyWindowInfo(OptionAll)`, layer 0 windows | `EnumWindows` once per tick, then cheap reads of each window |
| `id` | the window server's window number | the window handle (komorebi's `hwnd`) |
| `rect` | `kCGWindowBounds`, in display points | the visible frame (`DWMWA_EXTENDED_FRAME_BOUNDS`), in pixels |
| `visible` | `kCGWindowIsOnscreen` (false for windows on other spaces) | `IsWindowVisible`; invisible windows and tool windows are not listed |
| `minimized` | always false: the window server does not say | `IsIconic` |
| `cloaked` | absent | `DWMWA_CLOAKED` |
| `border` | never set | `komoborder-<id>` tool windows are listed (other tool windows are not), with `border` true, and `border_of` when the id is a window handle (a floating window); a tiled window's border is named for its container, see `windowtrack.rs` |
| `title` | `kCGWindowName` | `GetWindowTextW` |
| status | written against the `objc2` sources and not built or run | built and run |

### The program's own windows

The program has one window, with the recorder as its first tab and the recordings as the others. It must not be in the
video it is recording, in either of its sizes (the small always-on-top one and the full one), and komorebi must not tile
it. Every `komorebi-recorder` launch (`gui`, `view`, no command, and `komorebi-recorder-gui.exe`) shares one local socket,
so there is never a second window to exclude.

| | macOS | Windows |
|---|---|---|
| left out of the video | by name: every process called `komorebi-recorder` is excluded from the capture | each window asks to be left out of captures with `WDA_EXCLUDEFROMCAPTURE` (Windows 10 version 2004 and later) |
| left alone by komorebi | the windows have no title, which komorebi does not manage; there is no ignore rule to rely on, so they are never titled | the windows are titled `komorebi-recorder` only once the recorder has run `komorebic ignore-rule exe` for its executables (or no komorebi runs), and are otherwise untitled; `doctor` says which. The rule is komorebi's live one and is not saved: to keep it, see [recording a bug repro](usage/recording-a-bug-repro.md). Also `KOMOREBI_RECORDER_PROBE_WINDOW=1` makes a window ask komorebi whether it would manage it and write the answer to `komorebi-recorder-window-probe.txt` in the temp directory |
| icon | a slice of watermelon, which the Dock shows while a window is open (eframe sets it as the application's icon image). The program is a bare executable and not an app bundle, so Finder gives the file the generic executable icon | the same picture as the window and taskbar icon while a window is open, and compiled into both executables as a resource, which is what Explorer shows for the files; the installer shows it beside the program in Apps & features |
| executables | `komorebi-recorder` | `komorebi-recorder.exe` (console: run commands with this one) and `komorebi-recorder-gui.exe` (no console window) |

## Linux (KDE Plasma)

The Linux host records komorebi for KDE (`komorebi-for-kde`), on Plasma 6.8 or later, on Wayland. It is the
only Linux desktop the recorder knows: the screen comes from KWin's own screencast, and the windows from komorebi for
KDE. What is written from the sources and has not been seen to work on a real Plasma session is listed, one check each, in
[kde-checkpoints.md](dev/kde-checkpoints.md).

| | Linux (KDE Plasma) |
|---|---|
| master clock | `CLOCK_MONOTONIC`, `host.clock` is `monotonic`; it does not count the time the machine is suspended, and `CLOCK_BOOTTIME`, which does, is the reference clock |
| sleep, wake and lock | heard on D-Bus: logind's `PrepareForSleep` on the system bus and the screen saver's `ActiveChanged` on the session bus; whether the screen is locked is `org.freedesktop.ScreenSaver.GetActive`. The lid and the displays' power are not read; the displays are polled |
| the displays | the compositor's outputs, over Wayland (`wl_output` and `xdg_output`), kept up to date by a thread that reads the compositor's events. A display's `id` is the FNV-1a hash of the output's name, which is komorebi for KDE's id for a monitor with no serial number |
| capturing the screen | KDE's `zkde_screencast_unstable_v1`, which gives a PipeWire node and asks the user nothing; `pipewiresrc` reads it, and the frames are placed in the file's time as on Windows |
| permission | KWin offers the screencast only to a program whose desktop file lists `zkde_screencast_unstable_v1` in `X-KDE-Wayland-Interfaces`. `doctor` prints the file to write, with the path of the executable in it |
| the video | H.264 only, written by GStreamer (`x264enc` and `mp4mux`) as fragmented MP4 in segments, as on the other hosts. HEVC is refused. A display of an odd size, or one H.264 cannot take, is scaled by the pipeline |
| decoding | GStreamer: `decodebin` to BGRA at the size asked for, seeking to the key frame before the time and decoding forward. A frame's time is its stream time, because an MP4's edit list puts the media time of the first frame after zero |
| the OS trace | the journal: `oslog/system.ndjson` (`journalctl --user`, of `kwin_wayland`, `plasmashell` and `komorebi`, stream `journal`) and `oslog/eventlog.ndjson` (every warning and worse, stream `journal-warnings`). An entry's time is `__MONOTONIC_TIMESTAMP`, which is the master clock |
| the window track | komorebi for KDE's own *beliefs*, not KWin's geometry: only windows komorebi manages are listed, with komorebi's rect, each shown when its workspace is showing and it is in front of its stack. **Not done:** listing KWin's real windows. KWin has no D-Bus call that lists windows: `getWindowInfo` needs a UUID, and the only listing the recorder uses is `/WindowsRunner` `Match(query)`, which is a KRunner text search (it may need a minimum query length; not checked, so not relied on) and costs one `getWindowInfo` per window per tick. A complete, cheap source needs a KWin script (`loadScript` over `org.kde.kwin.Scripting`) that pushes `frameGeometryChanged`/`windowAdded` to a D-Bus service of the recorder's, or the `org.komorebi.Bridge` events; that was not built or tried on the Plasma guest. Until then the track on KDE cannot show a window komorebi does not manage, nor a frame that disagrees with komorebi's `rect` (use `--probe-windows` for per-event real frames). komorebi for KDE has no border windows (KWin's rounded-corners effect paints them), so there is nothing to mark `border` there |
| komorebi's sockets | `$XDG_RUNTIME_DIR/komorebi/` (the command socket `komorebi.sock` and the subscribers'), and the data directory where there is no runtime directory |
| komorebi's log | `~/.local/share/komorebi/komorebi_plaintext.log.<UTC date>`; the span of an event's handler is `handle_event` and of a command's `process_command` |
| the program's own windows | left out of the video by a KWin window rule (`excludefromcapture`, forced for the window class `komorebi-recorder`), which `record` writes to `~/.config/kwinrulesrc` and has KWin read; komorebi is told to ignore them by their application id, `komorebic ignore-rule class komorebi-recorder`, run before the window exists. Whether KWin honours the rule for a screencast of an output is a check still to make (K3) |
| the window's application id | the window's application id is `komorebi-recorder`, which is the `class` komorebi for KDE sees |
| `--probe-windows` | on by default, as on Windows: after each window event KWin is asked about the window (`org.kde.KWin.getWindowInfo`: the desktops it is on, minimized, its frame). komorebi's window is found by its class, title and frame through KWin's windows runner, because komorebi's ids are a count and not KWin's UUIDs. A window on a desktop that is not showing is `not visible`, one alone on `komorebi-park` is `cloaked` |
| ghost tiles | found from the frames komorebi has in its state, as on a Mac (a window in front on a showing workspace that has no frame anyone can see). No rule is proposed for one: komorebi for KDE's rules are in its own repository |

komorebi for KDE's rules are checked against the log and the events of a running komorebi (`komorebi --fake-backend`, an
in-memory desktop with no windows), and against its source. They are not checked against a recording of a real
desktop, so an analysis of a KDE session says so, as it does for Windows.

### What is needed on the machine

- the GStreamer plugins that read PipeWire (`pipewire`), decode and encode H.264 (`gst-libav`, `gst-plugins-ugly` for
  `x264enc`), and mux MP4 (`gst-plugins-good`); the Nix package puts them on the plugin path;
- `libwayland-client`, which is opened when it is needed, and what the window draws through (`libxkbcommon`, `libGL` or
  Vulkan): the Nix package and the dev shell put them on `LD_LIBRARY_PATH`;
- `ffmpeg` for `frames` and `selftest`, and nothing else.

## Degraded modes

- `record --no-video` records everything but the screen, on the same clock. It asks the system nothing about displays and
  needs no permission.
- `record --no-komorebi` skips touching komorebi altogether, and `--no-oslog` skips the OS trace.
- A komorebi that is not running when a recording starts is not an error: it is noted, and the recorder subscribes when
  it appears.
- The `unsupported` host investigates sessions and refuses to record.

## Advice by audience

- **Recording on macOS.** Grant Screen Recording to the terminal, and start komorebi with `--log-level trace`.
- **Recording on Linux (KDE Plasma).** Install the desktop file `doctor` prints, so that KWin offers the screencast, and start
  komorebi for KDE with `--log-level debug`. Use `record --no-video` where there is no screencast to be had.
- **Recording on Windows.** There is nothing to grant. Use `komorebi-recorder.exe` for commands, since the GUI build
  prints nowhere, and start komorebi with `--log-level debug` and `RUST_LOG=komorebi=debug`.
- **Investigating on any host.** A session is read the same everywhere. `ffmpeg` is needed only for `frames` and
  `selftest`.
- **Changing code that has a host-specific part.** Provide the function on every host, and run `just check-windows` from a
  Mac: it type-checks every `cfg(windows)` line, tests included.
