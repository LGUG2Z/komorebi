# Architecture

`komorebi-recorder` connects three things that do not know about each other: komorebi, which says what it is doing in
a log and in events on a socket; the operating system, which says what it is doing in its own trace and shows it on a
screen; and the person or agent who has to work out why they disagreed. It records all of them on one clock into a
session directory, and reads that directory back.

Two things about it vary independently, and everything below is arranged so that neither leaks into the other.

| axis | what varies | where the variation lives | what a session records |
|---|---|---|---|
| host | the OS the recorder runs on: macOS or Windows (a third `unsupported` host builds anywhere) | [`platform`](../komorebi-recorder/src/platform/mod.rs), selected by `cfg` | `host` in the manifest |
| flavor | the komorebi being recorded: komorebi for Mac or komorebi for Windows | `Rules` in [`analysis::flavor`](../komorebi-recorder/src/analysis/flavor/mod.rs), selected by data | `komorebi.flavor` in the manifest |

They are independent because a session recorded on one host is investigated on whichever machine the investigator has.
A session recorded on Windows is read on a Mac, and the other way round, so reading never asks which host it is
running on: it asks the session.

## The master clock

Everything in a session is stamped on one clock: the host's monotonic clock, in nanoseconds. That is
`mach_absolute_time` on macOS and `QueryPerformanceCounter` on Windows. Video frames and the OS trace are already on it.
komorebi's log lines carry UTC wall-clock time, so they are mapped onto it: the recorder samples a paired reading of
both clocks (an `Anchor`) at the start, every few seconds and at every sleep, and a `ClockMap` interpolates between
them. A `JumpDetector` notices sleep and clock steps by comparing how far each clock moved between two observations,
and the anchors on either side of the jump are what make the mapping right on both sides of it.

Three things are worth knowing before reading a session:

- The field names ending in `_mach_ns` keep their historical spelling. They hold master-clock nanoseconds on whichever
  host recorded the session; the manifest's `host.clock` says which clock that was (`mach`, `qpc` or `std`). A name that
  changed would stop older sessions being readable by newer builds and the reverse.
- The master clock is not the wall clock. On macOS it pauses while the machine sleeps, so video time skips a sleep
  instead of showing a hole. Whether the performance counter counts a suspend on Windows is measured, not assumed.
  (`doctor --probe clock` measures it.)
- A timestamp is a plain `u64` of nanoseconds. Nothing in the type system says which clock it is on: that is held by
  convention, and by the tests that place records at known times.

## The platform facade

The rest of the crate never names a system API. [`platform`](../komorebi-recorder/src/platform/mod.rs) is a facade of
small modules (`clock`, `shell` and, as the recorder grows, `capture`, `power`, `session`, `trace`, `process`, `apps`
and `window`), and `cfg` selects the directory behind it: `platform/macos/`, `platform/win/` or `platform/unsupported.rs`.
Each host provides the same names with the same signatures. A missing or mismatched function is a compile error on the
host being built, and on no other, so the Windows host is built from a Mac by `just check-windows` and by CI, and
`unsupported` (which exists so that the commands that only read a session directory work on a Linux agent box) is built
only on a third OS.

Where one host has no analogue of something, the facade function exists on both and one answers "nothing": `trace::start`
on the `unsupported` host returns no taps and a line saying so, and `power::start` returns no watcher.

## Flavors and rules

Which komorebi produced a session decides what its events mean (a process id and an accessibility notification on a Mac,
a window handle and a WinEvent on Windows), how it hides a window (by moving it to a corner on a Mac, by cloaking it in
place on Windows) and how its log marks where a handler starts and ends. `Flavor` names the two and is recorded in the
manifest. `Rules` is the trait that holds what each knows, and `Flavor::rules` is where a name becomes rules. Code that
reads komorebi's output asks `Rules` and does not match on `Flavor`; only convention holds that, and it has exceptions:
the index of komorebi's source that log lines are mapped to, and the places the log and a checkout of the source are
looked for on disk, are keyed by `Flavor` directly.

The contract is a counting rule: a flavor implements every method of `Rules` that has no default, and the compiler
refuses an implementor that misses one. A method with a default is a question most flavors answer the same way, and the
doc of each says what the default is. `Rules::verified` is how a flavor says that its rules were written from komorebi's
source and have not been checked against a recording of it: an analysis that reads such a session says so.

## The session

A session is a directory. Its manifest, `session.json`, is the one file everything else is read through: the host and the
flavor, the clock anchors, the komorebi processes that were seen and how each was logging, the byte ranges of komorebi's
log that were recorded and the applications that were traced. It is written to a temporary file and renamed, so a reader
or a crash never sees half of one, and its `schema_version` is checked on every read: a manifest written by a newer
build is refused with an error that says so, rather than read wrongly.

The files beside it are the raw records, and they are written as the data arrives and never rewritten: komorebi's log
(`komorebi.log`, the exact byte range recorded), its events (`events.jsonl`, with full `State` snapshots in `states/`
only when they changed), and what happened to the recording itself (`recorder.jsonl`).
The OS trace is under `oslog/`, one file per stream.
Each display's video is `video-<id>[-<n>].mp4` with a `frames-<id>[-<n>].csv` beside it holding every frame's timestamp,
one segment to a file: a display that is unplugged and returns starts a new segment instead of extending the old one.
`timeline.jsonl` is the only derived file: a time-sorted index of every record that points into the raw files by byte
range. Anything that happens to the recorder leaves the raw files readable, and the index is rebuilt from them with
`reindex`. The file formats are specified on the `session` and `timeline` modules.

## Recording

The recorder starts its sources in an order that makes the sources that are not video a superset of the video's span
(the clock anchor, the OS trace streams, the subscription to komorebi and its first `State`, komorebi's log offset, and
last the video), and stops them in the reverse order.
While it runs, the machine is docked, undocked, put to sleep, woken with the lid opened and locked, and none of that may
lose what was already captured. A tracker turns what the OS says (or, on a Mac, what the two clocks disagree about) into
reactions: a display that is gone has its video finalized at once, one that returns starts a new segment, and every
stream is started afresh after a wake. The capture backend is a trait, so that these policies are tested with a scripted
backend that docks and undocks without hardware.

## Investigating

Reading a session never needs a window: `analysis` answers each question as a plain serializable value, and the command
line and the viewer present the same answers. Every command takes `--json`, which prints an envelope with the data, its
schema, warnings, whether it was truncated and a cursor, and failures exit with a code that says what kind they were.

## Rules and the layer that enforces them

| Rule | Enforced by |
|---|---|
| Every host provides the same functions with the same signatures | the compiler, for the host being built; `just check-windows` and CI build the Windows host from a Mac; nothing builds `unsupported` |
| A timestamp in a session is on the master clock | convention, and the tests that place records at known times: the type is a plain `u64` |
| An implementor of `Rules` answers every question that has no default | the type system: a missing method is E0046, proved by a `compile_fail` example on `Rules` |
| Code that reads komorebi's output asks `Rules` and does not match on `Flavor` | convention only |
| Rules that were not checked against a recording of komorebi say so | the framework path: an analysis that reads such a session adds a warning |
| A running recorder can be told from one that died | the OS: an exclusive lock on `recorder.lock`, held for as long as the recorder runs and dropped by the OS when the process ends however it ends |
| A manifest from a newer build is not read | a runtime error when the manifest is read, covered by `rejects_manifests_from_the_future` |
| A manifest is never half-written | write to a temporary file and rename |
| The raw files are never rewritten | convention: the index only references them, and `reindex` rebuilds the index from them |
