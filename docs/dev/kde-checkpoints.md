# KDE checkpoints

`komorebi-recorder` is built and tested on Linux, but with no KWin to talk to: what touches a real Plasma session was
written from KWin's and komorebi for KDE's sources and has not been seen running. Each checkpoint below is a short list
of commands to run on a Plasma 6.8 session that runs komorebi for KDE, what to expect, and what to send back. The point
of each is to replace something read in a source with something observed.

Send back, for any step that does not behave: the exact command, what it printed, the output of
`komorebi-recorder doctor --json`, and the session directory if there is one.

## What has been seen

Against komorebi for KDE's in-memory desktop (`komorebi --fake-backend`, which has no windows) and the Wayland compositor
of WSLg (which has outputs and no screencast):

- `doctor` names komorebi for KDE and finds its socket in `$XDG_RUNTIME_DIR/komorebi`;
- `record --no-video` records its log, its commands as events with the handler of each, and the journal in two streams,
  and `events`, `summary` and `findings` read the session with the KDE rules;
- the displays are listed from the compositor, with a size and a position;
- a made-up source of frames is written to an H.264 file, which the decoder reads back, at the size of the display and
  at an odd size.

And in the Plasma guest of komorebi for KDE (`nix run .#run-kde`, KWin 6.8.2, komorebi with windows, headless over ssh;
the recorder from `nix build` runs in it, as the guest mounts the host's store):

- **K1:** `doctor` says `permitted: yes` and a recording works with no desktop file written. Whether KWin offers the
  screencast to every program, or this guest's configuration allows it, was not found out.
- **K2:** a recording of the output is a playable H.264 file with the windows, the panel and the cursor in it, and its
  length is the recording's. The frames' times were wrong at first (the GStreamer pipeline took the clock of the
  PipeWire stream, which starts at zero) and are on the master clock now. The log, the events and their handlers line up.
- **K3:** the rule that leaves the recorder's window out of the capture works: a window of the class
  `komorebi-recorder` that had the focus and was on top is not in the video, and one of another class is. KWin 6.8
  rewrites `kwinrulesrc` with `Order=` in place of `count=` and `rules=`, and the recorder extends whichever the file has.
- **`--probe-windows`:** a window of an event is found in KWin and its facts are read: frame, minimized, the desktops. The
  probes of a recording with workspace switches, a new window, a stack and a closed window agree with komorebi, and the
  one of a window that was closed says it no longer exists. komorebi's workspaces are KWin's virtual desktops, so a
  window of another workspace is not on the desktop that shows; the park desktop holds only windows hidden behind a
  front one. What was not seen: a probe that disagrees with komorebi (nothing was broken), and a window that cannot be
  told from another by class, title and frame (it has no probe).

## Getting the binary

```sh
nix build .#komorebi-recorder --out-link result
result/bin/komorebi-recorder doctor
```

The package wraps the program so that GStreamer finds its plugins (`gst-libav`, `x264enc`, the PipeWire source) and the
program finds `libwayland-client`. Outside Nix, install the same.

## K0: what komorebi for KDE really sends

Status: **can be run now, and is the most useful one.** The rules that read komorebi for KDE's events, `State` and log
were written from its source and checked against its log and commands, not against windows (the analysis says so in a
warning until they have been).

1. Start komorebi for KDE with debug logging: `komorebi --log-level debug` (or `RUST_LOG=komorebi=debug`).
2. `komorebi-recorder doctor`. Expect `komorebi for KDE`, `running: yes`, the socket under `$XDG_RUNTIME_DIR/komorebi`
   answering a state query, the log directory `~/.local/share/komorebi` with a `komorebi_plaintext.log.<date>` in it, your
   outputs, and `clock: Monotonic`.
3. Record 60 seconds with `--no-video`, and during them: open and close a few windows of different programs, focus
   between them, drag one to a new place and drag-resize one, stack and unstack, switch workspaces
   (`komorebic focus-workspace 1` and back), and lock and unlock the screen.
4. `komorebi-recorder events <session>`. Expect each window event with its window id, program and notification
   (`WindowAdded`, `InteractiveMoveFinished`, ...), each with a handler. Send back any event named `unreadable` or
   classified `other`, any window event with no handler, and any `MoveEnd` that is not classified `drag`.

## K1: the screencast is offered, and what it asks

Status: not run. This is the one the Linux host depends on.

1. `komorebi-recorder doctor`: under `capture`, `permitted: no` and the note with the desktop file.
2. Write `~/.local/share/applications/komorebi-recorder.desktop` as the note says (`Exec=` is the path of the executable
   that runs), and start the recorder again. Expect `permitted: yes`. In the Nix package the program
   is a wrapper that starts `.komorebi-recorder-wrapped`, and that is the executable the compositor sees, so it is the path
   `doctor` prints (it is `current_exe`).
3. If it is still `no`: say where KWin looks for desktop files and how it matches the program to one. Whether a
   desktop file in the user's `applications` directory is enough, whether `Exec=` has to be the executable's real path,
   and whether a program started from a terminal is matched at all, are what this finds out. If it cannot be made to
   work, the screencast portal (`xdg-desktop-portal`) is the way around it, behind the same `CaptureBackend`.

## K2: recording the screen

Status: not run.

1. `komorebi-recorder displays`. Expect each output with its size and position in the layout (logical pixels). Compare the
   ids with komorebi's: `komorebic monitor-information` (they agree for a monitor with no serial number).
2. `komorebi-recorder record --duration 20`, and move windows during it. Expect a `video-<id>.mp4` for each output and
   `frames-<id>.csv`, and in the session summary no error.
3. Play it with `komorebi-recorder view <session>`. Expect the picture to move with the windows, and the cursor in it.
4. `komorebi-recorder doctor --json`: the clock check (`latency`) of the segments should be near zero. A large number
   means the frames' times are not on the master clock, which would be the PipeWire timestamps: send the numbers back.
5. Leave the screen still for ten seconds: the video should still advance (the last frame is repeated).
6. Unplug a monitor, or turn one off, if there is one to spare: the segment ends and a new one starts when it returns.
7. Suspend and resume: every output starts a new segment.

## K3: the program's own window

Status: not run.

1. `komorebi-recorder` with no command opens the window. Expect it to be titled `komorebi-recorder` and not tiled.
2. If it was tiled, `komorebic ignore-rule class komorebi-recorder` was not applied in time, or the window's class is not
   what is expected: `komorebic state` says what the window's class is.
3. `record` writes a KWin window rule (`~/.config/kwinrulesrc`, group `komorebi-recorder-exclude-from-capture`) that
   forces `excludefromcapture` for the class `komorebi-recorder`. Record with the window open and play the video: the
   window should not be in it. If it is, say whether `kwinrulesrc` has the rule, whether System Settings > Window
   Rules shows it, and what `qdbus6 org.kde.KWin /KWin supportInformation` says of the window's class.

## K4: the OS trace and the window track

Status: the journal was followed on a machine with no Plasma.

1. `record --duration 30`, and cause some noise: open a program that is not in the session, plug something in.
   Expect `oslog/system.ndjson` to have `kwin_wayland` and `plasmashell` entries, and `oslog/eventlog.ndjson` the warnings.
   A user who is not in the `systemd-journal` group may see none of the system's: the problems say so.
2. `record --window-track --duration 30`. Expect `windows.jsonl` with the windows komorebi manages, a window behind a
   stack not shown, and a window of another workspace not shown.
