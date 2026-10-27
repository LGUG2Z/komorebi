# Platform support

This document gathers, in one place, what differs by operating system. Most of the details are written down on the items
that implement them; here the same questions are answered once per host, so that someone who has to run the recorder on
a machine, or change it for one, does not have to read the code to find out.

The recorder is built for macOS and Windows. A third host, `unsupported`, builds on anything else so that the commands
that only read a session directory work on a Linux agent box; recording there says that it cannot.

## Status

| host | built | run |
|---|---|---|
| macOS (Apple silicon) | yes | yes: written and run on macOS 26 |
| Windows x86_64 | yes, natively on `windows-latest` and cross-compiled from a Mac | no: type-checked and cross-built only |
| Windows ARM64 | yes, cross-built on an x86_64 runner | no |
| anything else | the `unsupported` host builds | nothing records |

Nothing that touches a real Windows desktop has been seen running: the Windows host was written from the Windows API
documentation and komorebi for Windows' source, and every claim below about Windows says what the code does, not what
was observed.

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

## Degraded modes

- `record --no-video` records everything but the screen, on the same clock. It asks the system nothing about displays and
  needs no permission.
- `record --no-komorebi` skips touching komorebi altogether, and `--no-oslog` skips the OS trace.
- A komorebi that is not running when a recording starts is not an error: it is noted, and the recorder subscribes when
  it appears.
- The `unsupported` host investigates sessions and refuses to record.

## Advice by audience

- **Recording on macOS.** Grant Screen Recording to the terminal, and start komorebi with `--log-level trace`.
- **Recording on Windows.** There is nothing to grant. Use `komorebi-recorder.exe` for commands, since the GUI build
  prints nowhere, and start komorebi with `--log-level debug` and `RUST_LOG=komorebi=debug`.
- **Investigating on any host.** A session is read the same everywhere. `ffmpeg` is needed only for `frames` and
  `selftest`.
- **Changing code that has a host-specific part.** Provide the function on every host, and run `just check-windows` from a
  Mac: it type-checks every `cfg(windows)` line, tests included.
