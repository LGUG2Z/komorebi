# Investigating a komorebi recording

You have a recording of komorebi misbehaving: the screen, komorebi's log, every event it sent with
its full state, and what the OS did to the machine meanwhile (macOS's log, Windows events, the journal), all on one clock. It is
komorebi for Mac, for Windows or for KDE Plasma: `overview` says which, and what a handler, a window id
and a hidden window look like differ between them (a window is a window id and a process on a Mac,
a handle and a program on Windows, a counter on KDE; a hidden window is parked in a corner on a Mac,
cloaked in place on Windows, so its position says nothing there, and moved to the `komorebi-park` desktop on KDE). Your job is to find
what *produces* the bug the user described, from the recording, and then propose a fix that rests on
what it shows. Every command takes `--json`; its output is an envelope
`{ok, schema, data, warnings, truncated, next_cursor, error}` whose `data` is described by
`komorebi-recorder schema <schema>`. Exit codes: 0 ok, 1 error, 2 usage, 3 not found, 4 unusable
session, 5 over budget, 6 an `assert` check failed.

## 1. Start with the brief

    komorebi-recorder brief <session>

The user marked a **region** of the video where the bug was visible and described it. That
description is the ground truth of *what* went wrong. Do not reinterpret it; explain it. If no region
was marked the brief says so: ask the user when it happened and what they saw, then record it with

    komorebi-recorder annotate <session> add --kind bug --start <t> --end <t> --text "<what is wrong>"

`brief` (and `overview`) also say **what this recording has**: events, komorebi's log, video, the window track, window probes
and the OS trace, each with the commands that read it and, for a part that is missing or limited, why and what that
rules out (`session.available` in `--json`; `brief --compact` has a `not recorded` line). A command whose data is
missing has nothing to say: do not run it, and do not read its silence as "nothing was wrong".

With little context to spare, `komorebi-recorder brief <session> --compact` is the same start in about 40 lines: the
user's words, one line on the state, the top 3 findings with their references and the exact next commands.
`komorebi-recorder findings <session>` lists everything the detectors found and any findings already recorded (`--kind`,
`--severity`, `--limit`); each detector finding has a stable id, `f-xxxxxx`.

The brief then gives, for each region: the events in it (echoes of komorebi's own moves are counted,
not listed), what looked wrong and for how long, the state as the bug began and as it ended and
what differs, what happened to the machine nearby (a display unplugged, a sleep), and the events most
worth tracing first with the reason each was chosen.

Notes by the author `script` are markers a script dropped while recording (`komorebi-recorder mark`),
naming the steps of the repro; read them as the timeline of what was being done.

Check `skew` before trusting any source location: it says whether komorebi's source is the commit
that was recorded. If not, check out the recorded commit and rebuild the recorder, or treat source
locations as hints.

## 2. Work from effect to cause

1. **What was wrong.** `state --at bug:<id>` and `state --at bug:<id>.end` show what komorebi
   believed, with `violations` for anything inconsistent (a window parked off screen on a workspace
   that is showing, a window in two places, the layout not applied). A window's `rect` is where it
   really was, read live from the accessibility API; `layout_rect` is where komorebi meant it to be.
2. **What changed.** `diff bug:<id> bug:<id>.end`, or `diff event:N event:N.end` for one event.
3. **What happened.** `events --from bug:<id> --to bug:<id>.end --hide-echo`. Classes: `command`,
   `focus`, `lifecycle`, `drag`, `monitor`, `lock`; `echo` is the OS reporting back a move komorebi
   made itself, never user input.
4. **Who did it.** `trace event:N` shows what the handler logged (each line with the source
   statement that wrote it), what other threads logged meanwhile, and every change with an
   **attribution**: `handler`, `reaper`, `workspace-reconciliator`, or `unexplained`. The reaper and
   the workspace reconciliator change state *without a notification*, so their changes surface in
   the next event. `unexplained` means state changed and nothing in the log accounts for it: that is
   a finding in itself (komorebi changed state silently, or its log level hides the cause).
   `trace event:N --os` adds what the **OS trace** recorded around it: komorebi's own calls into the
   OS while the handler ran, then the system and each application reacting, in time order. Records
   in that window are not proof the event caused them (other events' handlers may overlap it, and
   the output lists them), but it is where to look for "what did the system do when komorebi did
   this".
5. **One window.** `window <id|text>` follows a window through every recorded state.
6. **The code.** `code event:N` or `code log:N` maps log lines to the statement that wrote them,
   with the code around it. `ambiguous` and `generic` matches say so: do not pin a cause on one.
7. **Anything else.** `search <text>` across the log, events, incidents and annotations
   (`--oslog` adds the OS trace, where the recording has one); `at`, `range` and `summary` read the raw timeline.
   To query komorebi's log by field instead of grepping it, use `logq <session> 'level=ERROR and hwnd=0x77179C'`
   (`and`/`or`, `= != ~ !~`; `command`, `event` and any span field; `--count`, `--group-by <field>`);
   each hit's `log:N` goes to `at`, `trace` and `annotate --ref`.

## 3. Facts about the data that change what it means

- An event's receive time is when komorebi **finished** handling it: the notification is sent after
  the handler body. `event:N` as a time is the moment the handler began; `event:N.end` the moment
  it was received.
- komorebi logs no handler *end* line for events at INFO, and cannot show time spent waiting for the
  lock or in accessibility calls before the handler's header. `handler_ms` is a lower bound.
- A receive time is also when the recorder *got* the notification, which can be well after komorebi
  sent it: notifications are sometimes delivered in clumps. komorebi's own send time is its DEBUG log
  line `pushed notification to subscriber:`; where the log has it, an event's handler gives
  `pushed_video_s` and `delivery_ms` beside the received `video_s`, and `handler_ms` ends at the
  push. A large `delivery_ms`, or a `delivery-batching` finding, means delay between komorebi and
  the recorder, not komorebi being slow: go by the pushed time. A `slow-handler` finding without a
  push time may be the same thing, and says so.
- The `events` listing's time is one **komorebi time** per event: `p` when it is komorebi's pushed time, `r` when the log
  has no pushed line and it is only the received time (the header says so; `--json` has `komorebi_time.source` and
  `komorebi_time_note`). The received time is still given as `video_s`. The pushed line is checked only for komorebi for
  Mac's log; for Windows and KDE it is unverified, so expect `r` there if the log lacks it.
- Every command that takes a time takes the same forms: video time (`12.5`, `1:02.5`, `1:02:03`),
  `logtime:15:46:22.743` (a UTC time of day as komorebi's log writes it, on the session's date;
  `wall:15:46:22.743` is the same), `wall:<RFC 3339>`, `mach:<ns>`, `event:N`, `log:N`, `incident:N`,
  `bug:<id>`, each with an offset like `+3s`. A bare `15:46:22` is video time, never a log time:
  the prefix is what makes it one.
- The state in a notification is the state *after* the event, plus anything background threads did
  since the last one. States can be stale: `age_ms` in `origin` says how long.
- An `AXError` in komorebi's log comes from the application or the OS, not from komorebi.
- komorebi for Windows hides a window by **cloaking it where it is**, so its `State` cannot say
  whether a window is really hidden, and the position-based hidden-window checks do not apply. A
  recording made on Windows (unless with `--no-probe-windows`) stores, with each window event, what Windows said about that
  window just after it (`exists`, `visible`, `cloaked`, `minimized`, `rect`, and now and then
  komorebi's own `DebugWindow` verdict with the rules that matched); `overview` then reports
  `visible-window-hidden` (komorebi has it in front, Windows has it cloaked or minimized),
  `hidden-window-visible` (komorebi has it put away, Windows has it on screen) and
  `unmanaged-window` (on screen and not managed: usually right, and komorebi's explanation is
  quoted), and `trace event:N` shows the facts. Without the probe none of that is knowable, and
  `overview` says nothing about it.
- To measure the picture instead of scripting `ffmpeg`: `motion` (when a display or `--region x,y,w,h` moved,
  and how hard), `framediff <a> <b>` (what changed between two moments; `--png` shows where),
  `blank --region x,y,w,h` (when a region went one colour, such as white, or froze),
  `measure [--region x,y,w,h] [--stat mean|peak|width] [--hz N]` (mean or peak brightness, or drawn width, of a region on a fixed-rate timeline: the frame on screen at each sample time, so the numbers can be plotted or compared though the video has no fixed frame rate) and
  `compare <a> <b>` (two recordings' frames, motion and findings side by side). Regions are in display
  points, the same as the `bbox_points` of `changes`. They need a host that can decode the video.
- A session recorded with the window track (on by default; `--no-window-track` turns it off) has `windows.jsonl`: every top-level window's rect,
  visibility and minimized state (and `cloaked` on Windows) sampled at video rate, front to back,
  so "which window was where in this frame" has an answer. `sheet --windows` draws the rects on
  each frame (`windows` in the answer names the ones drawn). Rects are in the display's own units
  (pixels on Windows, points on a Mac). On Windows komorebi's border windows are in the track too, marked `border`
  (`border_of` names the window for a floating window's border; a tiled one is named for its container in the
  `title`, to be joined with `State`; pairing by rect is a heuristic, not recorded). Without it (`--no-window-track`, or a host that cannot list windows) only the sparse per-event probes exist.
  `lag <session>` (also the `geometry-lag:window` and `geometry-lag:border` findings) compares that track
  with komorebi's layout rects: per window, each stretch it was off its layout rect, start, end, duration, and
  the largest and last offset on each edge, on the master clock; with border windows in the track (rows marked
  `border`) it says whether the border or the window lagged. Offsets within `--tolerance` (default 8) and the
  first `--grace-ms` (default 500) after a layout change, when komorebi may be animating, are not flagged.
  On KDE the track is komorebi's own state, so `lag` says it is not independent and reports nothing.
- Video time `00:00.000` is the first frame of any display. A display that was unplugged, asleep
  or whose capture failed has a gap, and `incident` records say why.

## 4. Record what you conclude, with evidence

    komorebi-recorder annotate <session> add --kind finding --about <bug id> \
      --text "<the cause, in one or two sentences>" \
      --ref event:<n> --ref log:<n> --ref code:<path>:<line>

A finding with no evidence is refused, and every reference is checked to exist. Cite evidence by
event number, log line, video time and state difference. A log line can be cited by its text,
`--ref 'log:"some text"'`, or its time, `--ref log:HH:MM:SS.mmm` (UTC, as the log has it) or `--ref log:12.5` (video):
the command prints the line it resolved to, so check it is the statement you meant; an ambiguous
text lists the candidates and is refused. Never claim a cause without a reference,
and say what you could not verify.

### Scripted checks

To turn a scenario into pass or fail, `komorebi-recorder assert <session> --check "<check>" ...` evaluates checks with
the same analyses and answers with its exit code (0 all pass, 6 one failed, 4 one could not be measured, 2 malformed).
A check is `<kind> <args> <metric><op><number>`, with `==  !=  <=  >=  <  >`:

    findings kind=layout-mismatch count<=0
    log "level=ERROR and message~\"ghost\"" count==0      (the logq language)
    motion region=0,0,800,600 from=bug:a-1.start segments<=2     (also active_ms, peak_share)
    blank region=0,0,800,600 color=white ms<=100
    annotations kind=note count>=3                            (did the script's `mark`s land?)

## 5. Propose the fix

Name the file and function from `code`. Explain the chain from what the user saw to the state to the
event to the statement. Propose the smallest change that breaks the chain, say what would confirm it
(which command shows the difference once the fix is in; re-record the same scenario), and say what
the recording cannot show.
