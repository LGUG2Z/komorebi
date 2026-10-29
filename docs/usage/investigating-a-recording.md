# Investigating a Recording

A [recording](recording-a-bug-repro.md) answers "what was komorebi doing when this went wrong?" if you can read it.
`komorebi-recorder` has commands that do the reading, built so that a person or an agent can go from "the user says
this is broken" to "this is the line of komorebi that causes it" using what was recorded rather than guessing.

```
komorebi-recorder brief <session>     # start here
komorebi-recorder brief <session> --compact  # the same, in about 40 lines
komorebi-recorder findings <session>  # everything the detectors found and an agent concluded
komorebi-recorder guide               # the playbook, for an agent
```

Every command takes `--json`, which prints an envelope `{ok, schema, data, warnings, truncated, next_cursor, error}`;
`komorebi-recorder schema` lists the schemas and `komorebi-recorder schema brief/1` prints one. Exit codes: 0 ok, 1
error, 2 usage, 3 not found, 4 the session cannot answer (still recording, or the recorder died: `reindex` it), 5 over
budget, 6 an `assert` check did not hold.

## The viewer

```
komorebi-recorder view <session>
```

opens the recording in the window, for the person who made it (or anyone with the folder) to play, scrub and mark.
The Record tab of the same window has **Open session…** and **Open last** for the same thing.

There is only ever one window, for recording and for reviewing: its first tab is **Record** (see [Recording a bug
repro](recording-a-bug-repro.md#the-window)) and each recording is a tab after it. `komorebi-recorder view
<session>...` (from another terminal, or from the Record tab) adds tabs to the window that is already open and brings it
to the front, so several recordings can be loaded side by side. A recording that is already open is just switched to. **+** (or ⌘/Ctrl-T)
adds a start tab with your recent recordings and **Open…**, × (or middle-click, or ⌘/Ctrl-W) closes a tab (closing the
last one shows the Record tab), Ctrl-Tab and ⌘/Ctrl-1 to 9 move between them, ⌘/Ctrl-0 goes to the Record tab, and only the tab you are looking at plays. `view --new-window` skips all of this and
opens a window of its own.

- The **video** of every display, side by side. A display that was unplugged or asleep shows why it has nothing there,
  and the others carry on. **State overlay** draws komorebi's idea of the layout over the picture: where each tiled
  window was meant to be (dashed) and where it really was (solid, red if it looks wrong).
- The **timeline** under it has a lane for the **bug regions** you marked, komorebi's **events** (coloured by what kind,
  with echoes faint), errors and warnings in komorebi's log, what happened to the **machine** (a display unplugged, a
  sleep), the **findings** that look wrong, and the **video** each display has. Click anywhere to go there, drag to
  scrub, pinch or ⌘-scroll to zoom, scroll to pan, Shift-drag to select a range.
- **Playing** goes forwards and backwards at 0.25× to 8×; J, K and L shuttle like a video editor; ← and → step one
  frame; `,` and `.` jump between events. Press **?** in the window for every key.
- The **inspector** on the right: _Now_ is the state at the playhead and what has just happened; _Event_ is a selected
  event traced end to end, as `trace` shows it, ending with what the OS trace recorded around it (komorebi's own calls
  into the OS, the system, each application) as groups you can open; _Findings_ is what looks wrong, ranked; _Bugs_ is what was marked and
  concluded; _Log_ is komorebi's own log, following the playhead.

### Marking the bug

Press **B** (or **Mark bug**) to describe the bug: the form takes the region from your selection (`[` and `]` mark its
ends, or Shift-drag on the timeline) or from the playhead, asks what goes wrong and what you expected, and saves it the
moment you press Save. Drag a region's edges on the timeline to adjust it. **Copy a prompt for an agent** in the _Bugs_
tab puts an instruction on the clipboard that starts an agent on `brief` with your description.

### Proposing a rule for a ghost tile

A _ghost tile_ is a tile of the layout that holds a window nobody can see: komorebi has the window in front on a
workspace that is showing, and it never was on screen. When a bug region you marked was made while one was there, the
_Bugs_ tab says so under the region, and **Propose a rule to komorebi-application-specific-configuration** opens a
form with the rule that would ignore that window: which window it was, what the recording showed of it, and the entry
that would go in the repository. Nothing is sent until you press **Send as a pull request**.

Sending clones the repository into a temporary folder, commits the rule on a new branch with a message that says what
was seen and why, and opens a pull request under your account with the credentials you already have set up for `git`
(and the GitHub CLI, `gh`, if you have it). With `gh` signed in, the branch goes to your fork of the repository, made
if you have none, unless you may push to the repository itself. Without `gh` the branch is pushed with `git` and you
are given the link that opens the pull request. If anything fails, the form says what, keeps the clone, and gives the
commands to push it and open the pull request yourself.

The rule goes in the file of the komorebi the recording is of, whatever you are looking at it on:
`applications.json` for komorebi for Windows and `applications.mac.json` for komorebi for Mac. It is always narrow
enough to leave the application's other windows alone:

| | komorebi for Windows | komorebi for Mac |
|---|---|---|
| matches | the program and the window class, as a composite rule | the application and the window title, as a composite rule |
| when the application's visible windows share that class | the title is added, if it tells them apart | (windows have no class) |
| when nothing tells the ghost from a visible window | no rule is written, and the form says why | no rule is written, and the form says why |

The entry is put where it sorts and the rest of the file is unchanged, line for line.

What counts as a ghost tile depends on what the recording can say. On Windows it needs the window probes that `record` takes by
default (see
[Probing windows on Windows](recording-a-bug-repro.md#probing-windows-on-windows)): Windows says whether each window is
on screen, and a ghost is one it said was cloaked or not visible on every read and never on screen. A Mac cannot be
asked, so there it is a window whose frame, as komorebi read it from the accessibility API, was never one a person
could see: zero-sized, or outside every display. That is weaker evidence, and the pull request says which it is. A
recording that has none of this has no ghost tiles to propose, which is not the same as having none.

Everything the viewer shows is also available from the command line, so an agent can investigate headlessly:

## What the user said

The first thing in a [brief](#brief) is the **bug region**: the stretch of the recording the user marked, with their
description of what is wrong and what they expected.
The user marks it in the viewer. To add one without a window, as a script or an agent would:

```
komorebi-recorder annotate <session> add --kind bug --start 12.5 --end 18 \
  --text "the new window opens on the wrong monitor" --expected "it should open on the focused one"
komorebi-recorder annotations <session>
```

Times can be a video timecode (`12.5`, `1:02.5`), `logtime:<HH:MM:SS.mmm>`, `wall:<RFC 3339>`, `mach:<ns>`,
`event:<seq>`, `log:<line>`, `incident:<n>` or `bug:<id>` (`.start` or `.end`), each with an offset such as `+3s`. Every
command that takes a time (`at`, `range`, `frames`, `trace`, `diff`, `events`, ...) takes all of them.

`logtime:15:46:22.743` is a time of day as komorebi's log writes it (UTC), so a log line can be quoted as it is; the
date comes from the recording (the one on which the time falls inside it, if the recording crosses midnight).
`wall:15:46:22.743` means the same. It needs the prefix because a bare `1:02:03` is video time, and the two cannot be
told apart by their digits. `event:12` is when komorebi began handling event 12
and `event:12.end` when it finished.

## Commands

| Command | What it tells you |
| --- | --- |
| `brief` | The bug region(s) in the user's words, then what the recording shows around them: the events, what looked wrong, the state as it began and ended, what happened to the machine, and the events most worth tracing first, with why |
| `brief --compact` | The brief cut down to fit a small context window, about 40 lines: the user's words, one line on the state of the recording, the 3 highest-ranked findings (each with its id and the references to look at), and the exact commands to run next. `--json` gives `brief-compact/1` |
| `findings` | The detectors' findings and the findings an agent recorded, in one list (an agent's first, then the detectors' ranked as in `overview`). `--kind K` (`log-error`, or `invariant` for every `invariant:<kind>`), `--severity error\|warn\|info` (detector findings only), `--limit N`. `--json` gives `findings/1` |
| `overview` | The whole recording at a glance: what stands out, ranked (states that are wrong and for how long, windows that flap, event storms, slow handlers, errors, a display unplugged), anything near a bug region first |
| `state --at T` | What komorebi believed at a moment, and anything inconsistent in it: a window parked off screen on a workspace that is showing, a window in two places, a layout that was not applied |
| `diff A B` | What changed between two moments in terms a person would use: a window appeared, moved, was really somewhere else; focus moved; a monitor came or went |
| `events` | komorebi's events, classified (command, focus, lifecycle, drag, monitor, lock, and `echo`: the OS reporting back a move komorebi made itself), with the handler that ran and whether the state changed |
| `trace event:N` | One event end to end: what its handler logged, with the source statement for each line, what other threads logged meanwhile, and every change with who made it (see below). `--os` adds the OS trace around it: komorebi's own calls into the OS, the system and each application, split into while the handler ran and after |
| `window ID\|TEXT` | One window's life: when it appeared, moved, was parked off screen, was really somewhere else, and which event did each |
| `code event:N`, `code log:N` | The line of komorebi's source that wrote a log line, with the code around it |
| `logq QUERY` | Query komorebi's log by field, with no escape codes or span prefixes to strip: `level=ERROR and message~ghost`, `hwnd=0x77179C` (decimal and hex are the same window), `command=MoveWindow`; `--count`, `--group-by FIELD`, `--from`/`--to`, `--spans` |
| `search TEXT` | Where a word appears: komorebi's log, events, incidents, annotations (and `--oslog` for the macOS trace) |
| `changes` | When and where on each display the picture changed, and which events fall inside |
| `sheet` | A contact sheet image of frames with times, events and the bug region marked; `--windows` also draws each window's rect on the frames when the session has a window track |
| `motion [--region x,y,w,h]` | When, and how hard, a display or a region of it was moving: segments with start, duration, frame count, peak share of pixels changed and peak mean brightness delta (replaces scripting ffmpeg `tblend`) |
| `framediff A B [--region ..] [--png FILE] [--against SESSION]` | How two frames differ: changed share, mean and peak brightness delta, the box of change, and a PNG with the changed pixels in red |
| `blank --region x,y,w,h` | When a region was blank (one colour, named: `white`, `black`, a hex code) or frozen (identical to the frame before): "the window went white for 200 ms" |
| `measure [--region x,y,w,h] [--stat mean\|peak\|width] [--hz N] [--background 0-255]` | A statistic of a region on a fixed-rate timeline (default 30 Hz): each sample is the last frame shown at or before its time (the video has no fixed frame rate), with when that frame was shown. `mean` and `peak` are brightness 0 to 255; `width` is the *drawn width*: the horizontal extent, in display points, of the pixels that differ by more than 24 of 255 from the background (the most common brightness of the region, or `--background`), from the leftmost to the rightmost such column; 0 if none. Replaces ffmpeg scripts that crop and average. Samples with no frame (before the first one, or in a gap) are `null`. `--json` gives `measure/1` |
| `compare A B` | Two recordings side by side: duration, frames, effective fps, worst frame gap, motion, findings by kind, annotations, and the difference |
| `assert --check "KIND ARGS METRIC<=N"` | Pass/fail checks for a scripted scenario, repeatable. Kinds: `findings kind=K [severity=S] count<=N`, `log "<logq query>" count==N`, `motion [region=x,y,w,h] [from=T] [to=T] segments\|active_ms\|peak_share<=N`, `blank region=x,y,w,h [from=T] [to=T] [color=white] ms<=N`, `annotations kind=note\|bug\|finding count>=N`. Operators `==  !=  <=  >=  <  >`. Prints `PASS`/`FAIL` per check; exit 0 if all pass, 6 if one fails, 4 if one cannot be measured (no video), 2 if malformed. `--json` gives `assert/1` |
| `annotate`, `annotations` | Add, change and list bug regions, notes and findings |
| `mark TEXT` | While recording: drop a named point note (author `script`) onto the timeline from a script; it then appears in `annotations`, `brief` and the viewer like any note |
| `view [SESSION...] [--new-window]` | Open recordings in the window, each as a tab of the one window that is running (it is started if there is none and brought to the front if there is); `--new-window` makes a window of its own instead |

### Who changed it

komorebi sends the state _after_ each event, so what an event changed is the difference from the previous one. That
includes whatever background threads did since: the reaper (removes windows that no longer exist) and the workspace
reconciliator (moves windows between workspaces after a space change) change state **without sending a notification**.
`trace` therefore gives each change an _attribution_ with its evidence and how sure it is: `handler`, `reaper` (when its
log line names the window), `workspace-reconciliator`, or `unexplained`, which means state changed and nothing in the log
accounts for it. That is a finding in itself.

### From a log line to the source

komorebi's log says what happened but not where in its code. The recorder is built with an index of every `tracing`
statement in komorebi's source; `code` matches a recorded line to the statement that wrote it, says when several
statements read alike (`ambiguous`) or only print a value (`generic`), and flags the reaper and workspace reconciliator.

A mapping is only as good as the source it was made against, so every answer says whether the source the recorder was
built from is the komorebi that was recorded, by comparing the commit the recorded binary reports with the indexed
source and naming the files that differ. Lines in files that did not change are exactly as good as if the commits were
the same.

### What the data cannot tell you

- The event timestamp is when komorebi finished the handler. Time spent waiting for its lock or in accessibility calls
  before the handler's first log line is invisible, so `handler_ms` is a lower bound.
- The recorder can receive notifications long after komorebi sent them, and in clumps. komorebi's own send time is its
  `pushed notification to subscriber:` DEBUG line; `events --json` shows it as `pushed_video_s` next to the received
  `video_s`, with `delivery_ms` between them, and `overview` reports a `delivery-batching` finding (info) instead of a
  slow handler. A `slow-handler` finding in a log without push lines is marked as possibly delivery delay.
- The `events` listing's time is one **komorebi time** per event: `p` when it is komorebi's pushed time, `r` when the log
  has no pushed line and it is only the received time (the header says so; `--json` has `komorebi_time.source` and
  `komorebi_time_note`). The received time is still given as `video_s`. The pushed line is checked only for komorebi for
  Mac's log; for Windows and KDE it is unverified, so expect `r` there if the log lacks it.
- At the default `info` level there is no log line for the end of an event handler, and few for the decisions inside it.
  Start komorebi with `--log-level trace` before recording a bug you intend to investigate.
- The `rect` of a window in the state is read live from the accessibility API when the state is serialized: it is where
  the window really was, which is what makes comparing it with where komorebi's layout put it meaningful.
- An `AXError` in komorebi's log comes from the application or the OS, not from komorebi.

### Finding ids

Each detector finding has an id like `f-04dc11`: `f-` and six hex digits of a fixed hash (FNV-1a) of its kind, its first
reference (`event:17`, `log:345`) and the ids of the windows it is about; a finding with no reference uses the
five-second bucket of video time it began in. It does not depend on the score or on the bug regions, so the same finding
keeps its id between runs and as annotations are added, and `findings`, `brief --compact` and `overview` name it the
same way. Two different recordings share an id only where the same kind of finding is at the same reference, so use ids
to compare analyses of one session (before and after a change to the detectors), not to match a bug across recordings.
An agent's findings keep their annotation id (`a-3f9c1e02`).

## Recording what you conclude

```
komorebi-recorder annotate <session> add --kind finding --about <bug id> \
  --text "the Show handler tiles the window onto the display that is going away" \
  --ref event:12 --ref log:340 --ref code:komorebi/src/process_event.rs:512
```

A finding with no evidence is refused, and every reference is checked to exist: the event, the log line, the video time,
the line of komorebi's source. A log line can be named by what it says or when it was written instead of by number:
`--ref 'log:"could not place window"'` (the one line of `komorebi.log` that contains the text, ignoring case),
`--ref log:15:46:23.677700` (the line written at that UTC time, as precisely as you write it; `HH:MM:SS` alone matches the
whole second) or `--ref log:2.72` (the line nearest that time of the video). A text or time that matches several lines
is a usage error (exit 2) listing the first few as `log:N: text`; pick one with `log:N`. When a reference was resolved
this way the command prints the `log:N` it chose and the line's text (on standard error, and in `warnings` with
`--json`), so you can check it is the statement you meant. Findings are stored in `annotations.jsonl` beside the bug region, so the person who
recorded the bug can see what was concluded and why.
