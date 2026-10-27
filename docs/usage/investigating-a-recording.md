# Investigating a Recording

A [recording](recording-a-bug-repro.md) answers "what was komorebi doing when this went wrong?" if you can read it.
`komorebi-recorder` has commands that do the reading, built so that a person or an agent can go from "the user says
this is broken" to "this is the line of komorebi that causes it" using what was recorded rather than guessing.

```
komorebi-recorder brief <session>     # start here
komorebi-recorder guide               # the playbook, for an agent
```

Every command takes `--json`, which prints an envelope `{ok, schema, data, warnings, truncated, next_cursor, error}`;
`komorebi-recorder schema` lists the schemas and `komorebi-recorder schema brief/1` prints one. Exit codes: 0 ok, 1
error, 2 usage, 3 not found, 4 the session cannot answer (still recording, or the recorder died: `reindex` it), 5 over
budget.

Everything a recording holds is available from the command line, so an agent can investigate headlessly:

## What the user said

The first thing in a [brief](#brief) is the **bug region**: the stretch of the recording the user marked, with their
description of what is wrong and what they expected.
Add one with `annotate`:

```
komorebi-recorder annotate <session> add --kind bug --start 12.5 --end 18 \
  --text "the new window opens on the wrong monitor" --expected "it should open on the focused one"
komorebi-recorder annotations <session>
```

Times can be a video timecode (`12.5`, `1:02.5`), `wall:<RFC 3339>`, `event:<seq>`, `log:<line>`, `incident:<n>` or
`bug:<id>` (`.start` or `.end`), each with an offset such as `+3s`. `event:12` is when komorebi began handling event 12
and `event:12.end` when it finished.

## Commands

| Command | What it tells you |
| --- | --- |
| `brief` | The bug region(s) in the user's words, then what the recording shows around them: the events, what looked wrong, the state as it began and ended, what happened to the machine, and the events most worth tracing first, with why |
| `overview` | The whole recording at a glance: what stands out, ranked (states that are wrong and for how long, windows that flap, event storms, slow handlers, errors, a display unplugged), anything near a bug region first |
| `state --at T` | What komorebi believed at a moment, and anything inconsistent in it: a window parked off screen on a workspace that is showing, a window in two places, a layout that was not applied |
| `diff A B` | What changed between two moments in terms a person would use: a window appeared, moved, was really somewhere else; focus moved; a monitor came or went |
| `events` | komorebi's events, classified (command, focus, lifecycle, drag, monitor, lock, and `echo`: the OS reporting back a move komorebi made itself), with the handler that ran and whether the state changed |
| `trace event:N` | One event end to end: what its handler logged, with the source statement for each line, what other threads logged meanwhile, and every change with who made it (see below). `--os` adds the OS trace around it: komorebi's own calls into the OS, the system and each application, split into while the handler ran and after |
| `window ID\|TEXT` | One window's life: when it appeared, moved, was parked off screen, was really somewhere else, and which event did each |
| `code event:N`, `code log:N` | The line of komorebi's source that wrote a log line, with the code around it |
| `search TEXT` | Where a word appears: komorebi's log, events, incidents, annotations (and `--oslog` for the macOS trace) |
| `annotate`, `annotations` | Add, change and list bug regions, notes and findings |

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
- At the default `info` level there is no log line for the end of an event handler, and few for the decisions inside it.
  Start komorebi with `--log-level trace` before recording a bug you intend to investigate.
- The `rect` of a window in the state is read live from the accessibility API when the state is serialized: it is where
  the window really was, which is what makes comparing it with where komorebi's layout put it meaningful.
- An `AXError` in komorebi's log comes from the application or the OS, not from komorebi.

## Recording what you conclude

```
komorebi-recorder annotate <session> add --kind finding --about <bug id> \
  --text "the Show handler tiles the window onto the display that is going away" \
  --ref event:12 --ref log:340 --ref code:komorebi/src/process_event.rs:512
```

A finding with no evidence is refused, and every reference is checked to exist: the event, the log line, the video time,
the line of komorebi's source. Findings are stored in `annotations.jsonl` beside the bug region, so the person who
recorded the bug can see what was concluded and why.
