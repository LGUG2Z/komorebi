# Investigating a komorebi recording

You have a recording of komorebi misbehaving: the screen, komorebi's log, every event it sent with
its full state, and (on a Mac) what macOS did to the machine meanwhile, all on one clock. It is
komorebi for Mac or komorebi for Windows: `overview` says which, and what a handler, a window id
and a hidden window look like differ between them (a window is a window id and a process on a Mac,
a handle and a program on Windows; a hidden window is parked in a corner on a Mac and cloaked in
place on Windows, so its position says nothing there). Your job is to find
what *produces* the bug the user described, from the recording, and then propose a fix that rests on
what it shows. Every command takes `--json`; its output is an envelope
`{ok, schema, data, warnings, truncated, next_cursor, error}` whose `data` is described by
`komorebi-recorder schema <schema>`. Exit codes: 0 ok, 1 error, 2 usage, 3 not found, 4 unusable
session, 5 over budget.

## 1. Start with the brief

    komorebi-recorder brief <session>

The user marked a **region** of the video where the bug was visible and described it. That
description is the ground truth of *what* went wrong. Do not reinterpret it; explain it. If no region
was marked the brief says so: ask the user when it happened and what they saw, then record it with

    komorebi-recorder annotate <session> add --kind bug --start <t> --end <t> --text "<what is wrong>"

The brief then gives, for each region: the events in it (echoes of komorebi's own moves are counted,
not listed), what looked wrong and for how long, the state as the bug began and as it ended and
what differs, what happened to the machine nearby (a display unplugged, a sleep), and the events most
worth tracing first with the reason each was chosen.

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
- Video time `00:00.000` is the first frame of any display. A display that was unplugged, asleep
  or whose capture failed has a gap, and `incident` records say why.

## 4. Record what you conclude, with evidence

    komorebi-recorder annotate <session> add --kind finding --about <bug id> \
      --text "<the cause, in one or two sentences>" \
      --ref event:<n> --ref log:<n> --ref code:<path>:<line>

A finding with no evidence is refused, and every reference is checked to exist. Cite evidence by
event number, log line, video time and state difference. Never claim a cause without a reference,
and say what you could not verify.

## 5. Propose the fix

Name the file and function from `code`. Explain the chain from what the user saw to the state to the
event to the statement. Propose the smallest change that breaks the chain, say what would confirm it
(which command shows the difference once the fix is in; re-record the same scenario), and say what
the recording cannot show.
