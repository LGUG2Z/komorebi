# Windows checkpoints

`komorebi-recorder` is built for Windows from a Mac (and, in CI, on Windows), so nothing that touches a real
Windows desktop has been seen running. Each checkpoint below is a short list of commands to run on a Windows PC or
VM, what to expect, and what to send back. The point of each is to replace something read in
komorebi's source with something observed.

Send back, for any step that does not behave: the exact command, what it printed, the output of
`komorebi-recorder.exe doctor --json`, and the session directory if there is one.

## Getting the binaries

On the Mac, in this repository:

```sh
nix build .#komorebi-recorder-windows --out-link result-windows
ls result-windows/bin
# komorebi-recorder.exe       console build: use this one to run commands and read their output
# komorebi-recorder-gui.exe   the same program without a console window (shortcuts, the viewer)
```

Copy `komorebi-recorder.exe` to the Windows machine. The `windows` job of CI builds the same executables natively and
uploads them, with the komorebi installer that carries them, as artifacts of the run. To check that the Windows
code still type-checks without building it: `nix build .#checks.x86_64-linux.windows-clippy` or, in the default
`nix develop` shell, `cargo clippy -p komorebi-recorder --all-targets`.

## W0: what komorebi for Windows really sends

Status: **can be run now, and is the most useful one.** Reading komorebi for Windows' events, state
and logs was written from komorebi's source and has not been checked against a recording of it (the
analysis says so in a warning until it has); this is what checks it.

1. Start komorebi for Windows with debug logging. The default of `RUST_LOG=debug` also prints every
   dependency, so name the crate:

   ```powershell
   komorebi.exe --log-level debug
   ```

   (or set `RUST_LOG=komorebi=debug` first).

2. Describe the machine:

   ```powershell
   komorebi-recorder.exe doctor
   komorebi-recorder.exe doctor --json > doctor.json
   ```

   Expect: `komorebi for Windows`, `running: yes`, a binary path and version, `log level: debug`,
   `answers a state query: yes`, the log directory under `%TEMP%` with a `komorebi_plaintext.log.<date>`
   in it, your displays with their scale, and `clock: Qpc`. `can record: yes` and `can decode: yes`
   are what this build claims; whether they are true is W1 and W3. Anything under "things to know"
   is worth reading. If the state query is
   reported as answered with nothing, say what komorebi was doing (paused, another virtual desktop).

3. Record 60 seconds without video, and during them:

   - focus a few windows of different programs (click, and with `komorebic.exe focus left|right`);
   - drag a window to a new place, and drag-resize one;
   - move windows with komorebic (`move`, `stack`, `unstack`, `focus-workspace 1` and back);
   - open and close a program, and minimize and restore one;
   - if you can: change the display scaling or resolution, connect or disconnect a monitor, switch to
     another virtual desktop and back, lock (Win+L) and unlock once.

   ```powershell
   komorebi-recorder.exe record --no-video --duration 60 --out C:\temp\w0
   ```

   Expect a line per second with `events` counting up as you act, `komorebi log` growing and `OS
   trace` counting up (WinEvents for every window, so it is busy while you work), then `session:
   C:\temp\w0`. If the OS trace is too much or too little, say so: it is folded per window but its
   volume on a real machine has not been seen. `--no-oslog` leaves it out.

4. Look at it, to see what the current (macOS-vocabulary) analysis makes of it. It is expected to be
   wrong in places, and what is wrong is the point:

   ```powershell
   komorebi-recorder.exe brief C:\temp\w0
   komorebi-recorder.exe events C:\temp\w0 --json > events.json
   ```

5. Send back `doctor.json` and the whole `C:\temp\w0` folder, zipped. It contains the titles and
   program names of your windows (`events.jsonl`, `states\*.json`, `komorebi.log`); redact them first
   if that matters (they only need to still be valid JSON, and the Windows fixtures are scrubbed by
   hand anyway).

Also worth one run, for the checks komorebi's state cannot make (it hides windows by cloaking them
in place): record with the probe on, and switch workspaces a few times (`komorebic.exe
focus-workspace 1`, `2`, `1`) while focusing windows.

```powershell
komorebi-recorder.exe record --probe-windows --no-video --duration 60 --out C:\temp\w0-probe
komorebi-recorder.exe overview C:\temp\w0-probe
```

`overview` should report nothing about hidden windows if komorebi and Windows agree. If it reports
`visible-window-hidden` or `hidden-window-visible`, `komorebi-recorder.exe trace C:\temp\w0-probe
event:<n>` shows what Windows said; say whether it is a real fault or a false alarm (a window mid
transition, say), and how many there were. The probe asks komorebi about a window by `DebugWindow`;
if komorebi stalls while it runs, say so.

Optional, for the part of the plan that stops the recorder window being tiled: for one window komorebi
manages and one it does not (get handles from `komorebic.exe state`), run

```powershell
komorebi-recorder.exe doctor --probe window --hwnd <handle>
```

and send what komorebi says.

## W1: capture and the clock across a sleep

Status: **can be run now.** Windows capture, the power watcher and the OS trace are built but have
never run on Windows; these are the first runs.

1. The clock across a sleep (see above): `doctor --probe clock --seconds 120`, sleeping the machine
   for about a minute while it runs.

2. Capture on a static screen. Leave the screen completely still for most of it:

   ```powershell
   komorebi-recorder.exe doctor --probe capture --seconds 20
   ```

   Expect, per display, a segment with a handful of frames, `idle callbacks 0` (Windows says nothing
   about a still screen), a file that `plays` for about the length recorded, and `latency` of a few
   milliseconds to well under a second, `(NOT on the host clock)` absent. Check it with ffmpeg if you
   have it: `ffprobe <session>\video-<id>.mp4`.

3. Two monitors, if you have them: the same again, and say what it printed for each.

4. Kill it and play the file. Start a longer one, then end it without letting it stop:

   ```powershell
   komorebi-recorder.exe doctor --probe capture --seconds 60
   # after 15 s, from another window:
   taskkill /F /IM komorebi-recorder.exe
   ```

   Then try to play `video-*.mp4` in the session folder (any player), and run
   `komorebi-recorder.exe reindex <session>`. This answers whether Media Foundation's fragmented MP4
   survives a kill (the part before the last fragment, about a second, should play) and whether
   `reindex` can recover the session. Say what the player did and what `reindex` printed.

## W2: a real recording, and the machine doing the things

Status: can be run once W1 shows capture works.

```powershell
komorebi-recorder.exe selftest --seconds 8
komorebi-recorder.exe record --duration 60 --out C:\temp\w2
komorebi-recorder.exe reindex C:\temp\w2
```

`selftest` needs `ffmpeg` on PATH; expect `PASS` with the worst disagreement under 100 ms. During the
60 s recording: dock or undock a display, put the machine to sleep and wake it, lock and unlock
(Win+L), and close the lid if it is a laptop. Afterwards `komorebi-recorder.exe summary C:\temp\w2`
and `komorebi-recorder.exe at C:\temp\w2 <time> --source recorder` should show each as an incident
(`suspend`, `wake`, `lid-closed`, `screen-locked`, ...) next to the video gaps. Send back the session
folder (the video is large; the rest is what matters, say so if you leave it out) and anything that
looked wrong.

## W3: playing it back

Status: can be run once W1 shows capture works.

```powershell
komorebi-recorder.exe doctor --probe decode --file C:\temp\w2\video-<id>.mp4
```

Expect the video's size and duration, a first frame in well under a second, and a speed in frames
per second, then three seeks each answered with a frame at or just before the time asked for. Say
if any answers "no frame" or is far from what was asked.

## W4: our windows

Status: can be run once W1 shows capture works.

1. Start komorebi, then open the viewer on a Windows session: `komorebi-recorder-gui.exe view
   C:\temp\w2`. It should open floating (not tiled into your layout) and be usable: moving, resizing,
   minimizing and closing it. If komorebi tiles it, say so.
   Its taskbar button and Alt+Tab entry should show the watermelon icon.
2. Ask komorebi what it thinks of it. Set `KOMOREBI_RECORDER_PROBE_WINDOW=1` before starting the
   viewer (or the recorder window), wait a few seconds, and send back
   `%TEMP%\komorebi-recorder-window-probe.txt`.
3. Open the recorder window (`komorebi-recorder-gui.exe`) and the viewer, record 10 s with them
   open, and check neither appears in the video (`doctor` warns if this Windows is too old to keep
   them out).
4. The OS trace: `komorebi-recorder.exe record --duration 20 --out C:\temp\w4` while moving a
   window about, then `komorebi-recorder.exe trace C:\temp\w4 event:<a MoveResizeEnd or FocusChange>
   --os`. It should list what Windows told komorebi around that event, grouped by program. Say what
   the volume looked like (`summary` shows the counts) and whether it was too much or too little.

## W5: the installer

Status: can be run once CI has built one. WiX only runs on Windows, so what a Mac can check about the installer is
`tests/packaging.rs`; everything below is first seen here. The installer is not signed, so SmartScreen or Defender may
warn on the first run.

1. Download the MSI from the `windows` job's artifact (or the nightly release) and install it. Open a new shell and run

   ```powershell
   komorebi-recorder doctor
   ```

   That it runs proves the `PATH` entry. `Program Files\komorebi-recorder\bin` should hold `komorebi-recorder.exe` and
   `komorebi-recorder-gui.exe`.
   In Explorer both executables should show the watermelon icon, and so should the entry in Apps & features.

2. Install a second build over it, such as the next nightly. It should upgrade in place: one entry in Apps & features,
   not two.

3. Uninstall it. The files and the `PATH` entry should go. If komorebi for Windows is installed from its own MSI, check
   that it is untouched: the two installers must not upgrade or uninstall each other.

4. The ARM64 installer has only been cross-built on an x86_64 runner. If you have an ARM64 machine, repeat 1 to 3 with it.
