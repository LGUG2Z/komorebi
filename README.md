# komorebi-recorder

Synchronized screen, log and trace capture for komorebi bug reproduction. `komorebi-recorder` records a bug while it
happens, on macOS and on Windows: the screen, komorebi's log, every event komorebi sends its subscribers and the
operating system's own trace, all on one clock, into a single _session directory_. It then answers, from that
directory alone, what komorebi and the OS were doing at any second of the video and where in the video any log line or
event is: for a person, with a window that plays the recording with everything that happened laid out on a timeline,
and for an agent, with commands that print JSON.

## Install

Every release has, for macOS and for both architectures of Windows, a zip of the executables and, for Windows, an MSI
installer that puts them in `Program Files\komorebi-recorder\bin` and optionally on the `PATH`. The nightly release
is rebuilt every week from `master`.

| | file |
|---|---|
| macOS (Apple silicon) | `komorebi-recorder-<version>-aarch64-apple-darwin.zip` |
| Windows x86_64 | `komorebi-recorder-<version>-x86_64.msi` or `komorebi-recorder-<version>-x86_64-pc-windows-msvc.zip` |
| Windows ARM64 | `komorebi-recorder-<version>-aarch64.msi` or `komorebi-recorder-<version>-aarch64-pc-windows-msvc.zip` |

`checksums.txt` lists the SHA-256 of every file. The installers and executables are not signed, so Windows
SmartScreen and Defender may warn about them.

### From source

The repository is a Nix flake. In a checkout:

```sh
nix develop                  # the toolchain and the pinned komorebi sources
cargo install --path komorebi-recorder --locked
```

`nix build` makes the macOS binary in `./result/bin`, and `nix build .#komorebi-recorder-windows` cross-compiles
`komorebi-recorder.exe` and, with it, `komorebi-recorder-gui.exe` from a Mac. `nix run github:LGUG2Z/komorebi-recorder`
runs the latest commit without a checkout.

## Record a bug

```sh
komorebi-recorder displays                       # which displays can be recorded
komorebi-recorder record --duration 30 --out ./repro
komorebi-recorder record --until-exit --out ./repro -- ./repro.sh   # record while a script runs, stop when it exits
```

Run `komorebi-recorder` with no command for a small window with Record and Stop buttons instead.

On macOS the terminal needs the Screen Recording permission, as `komorebi` does. For a useful recording, start komorebi
with `--log-level trace` first: the recorder never changes komorebi's log level, it only warns. A session made with
`--no-video` has everything but the screen. [Recording a bug repro](docs/usage/recording-a-bug-repro.md) has the rest:
what is captured on each OS, docking, sleep and lock, and recovering a recording after a crash.
`record --window-track` also samples where every window is at video rate, and `sheet --windows` draws those rects on
the frames.

## Investigate it

```sh
komorebi-recorder brief ./repro        # what the user marked as the bug, and what the recording shows around it
komorebi-recorder brief ./repro --compact  # the same in about 40 lines, for a small context window
komorebi-recorder findings ./repro     # detector findings (with stable ids) and an agent's conclusions, in one list
komorebi-recorder overview ./repro     # the whole recording at a glance
komorebi-recorder trace ./repro event:7  # one event, from the handler that ran to what changed and who changed it
```

For example, a four second recording of a Mac with komorebi running:

```text
$ komorebi-recorder overview ./repro
session /private/tmp/kr-sample: 4 s of video, 2 events, 2 log lines, 1 incidents

findings, most worth looking at first (1 of 1):
  info  00:00.631  incident:recorder  recorder: recording started without video; komorebi pid 2732

when things happened (events | errors | incidents):
  00:00.500 ##############################    1   0  1
  00:04.500 ##############################    1   0  0
```

`komorebi-recorder view ./repro` opens the recording in the viewer, which is where you mark where the bug is.
Every command takes `--json` (including `record`, `displays`, `reindex` and `selftest`), and `komorebi-recorder guide`
prints a playbook for an agent.
[Investigating a recording](docs/usage/investigating-a-recording.md) walks through both ways of working.

## How it fits together

Two things vary independently: the _host_ the recorder runs on (macOS or Windows) and the _flavor_ of komorebi it
records (komorebi for Mac or komorebi for Windows). A session records both, so one recorded on Windows can be
investigated on a Mac. Every record in a session is stamped on one monotonic clock, and the files in it are never
rewritten after they are written.
[Architecture](docs/ARCHITECTURE.md) explains the model and which layer enforces each rule;
[Platform](docs/PLATFORM.md) answers the same questions for each host.

## Crates

- [`komorebi-recorder`](komorebi-recorder): the library, the `komorebi-recorder` command and, with the same code, the
  `komorebi-recorder-gui` executable (no console window on Windows).

## Checks

`just check` runs formatting, clippy and the tests for macOS, clippy for Windows (the Windows code is type-checked
from a Mac, not run), rustdoc with its warnings as errors for both hosts, and `cargo deny`. It needs `nix develop`.
CI runs the same checks, builds both platforms and, for Windows, the installer.

## Documentation

`cargo doc --workspace --no-deps --open` for the code. [CONTRIBUTING.md](CONTRIBUTING.md) says how to work on it.

## License

[Komorebi License 2.0.0](LICENSE.md).
