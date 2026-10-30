# What the shared scenarios say about kfde and kfw

[COMPARE.md](COMPARE.md) compares komorebi for KDE Plasma (kfde) with komorebi for Windows (kfw)
from their sources. This document compares them from running them: the scenarios in
`nix/tests/vm/lib/komorebi_vm/core/scenarios` are written once, against the `Platform` of
[vm-tests.md](vm-tests.md), and run on a real KWin and on a real Windows 11 desktop. A scenario that
passes on both says the two ports behave the same in that respect, as far as the scenario looks. A
scenario that fails on one says where they differ, and is a finding to look into, not an assertion
to loosen.

Everything here is a dated result, and says who got it. It is not generated: it is written from the
runs, and goes stale when the code changes.

## Where the results come from

- **kfde**: the suites of this repository, run on 2026-10-29 on a Windows 11 machine's WSL 2 (a
  Ryzen 9 5900X, nested KVM), at `scale` 1.5.
- **kfw**: a Windows 11 guest under QEMU on the same machine, running kfw's daemon, driven from
  kfw's tree with a copy of `core/` that `scripts/sync-e2e-core` keeps in step. These results were
  reported by the session that ran them, and are not repeated here. The guest is a Windows 11
  evaluation image. Its display driver was the basic one for the scenarios that have run; a second
  display, added and removed while it runs, has since worked with a QXL device and Red Hat's signed
  driver for it, which the current image carries. The windows are `komorebi-testwin`, a test window
  of its own, where kfde's are `foot` terminals.

## The scenarios

| Scenario | What it checks | kfde | kfw |
|---|---|---|---|
| `tiling` | three windows are tiled: where the layout says, inside the work area, clear of each other | pass | pass |
| `focus` | `focus` and `cycle-focus` move the focus, and the OS and the daemon agree on it | pass | pass |
| `moving` | `move` and `cycle-move` swap tiles, and the focus stays with the window | pass | pass |
| `stacks` | `stack`, `cycle-stack` and `unstack`: the window in view has the focus, the others are put away | pass | pass |
| `monocle` | `toggle-monocle` gives one window the work area and puts the others away | pass | pass |
| `workspaces` | `send-to-workspace` and `focus-workspace` put windows away and bring them back | pass | pass |
| `closing` | a closed window leaves the state, and the others take its room | pass | pass |
| `resize` | dragging the edge two tiles share moves it by what the pointer moved | pass | pass on kfw's `0ffc502c`, **fails** on its master |
| `commands` | every layout, and about 216 commands, leave the daemon healthy | pass, none skipped | pass with a second display, none skipped, when the host is quiet |
| `commands.unknown_monitor` | a window sent to a monitor that does not exist stays managed | pass (fixed) | **known failure** on `0ffc502c`; pass on kfw's `next` (`6d862a11`, 12 checks, no marker) |
| `rules` | initial, permanent and ignore rules by title, and that cleared rules stop applying | see below | pass |
| `rules_class` | a workspace rule and an ignore rule by class (the app id on KDE) send a window or leave it alone | pass | pass |
| `rules_exe` | the same by the program that opened the window; the rule's id is `platform.exe_id(program)`, as kfw's has the `.exe` | pass | pass |
| `config_reload` | rewriting the `komorebi.json` the daemon runs on changes its names, layout and rules | pass | pass |
| `locking` | the screen is locked, a window moved, and unlocked: every window is in its tile again (session suite `core-lock`) | pass | pass |
| `hotplug` | an unplugged display takes its windows to the other, a plugged one is a monitor | pass | pass |
| `placement` | the placement rules of the config put a window first, last, or at a number | pass | 4 of 5 cases pass, **known failure** on `0ffc502c`: the number 2; pass on kfw's `next` (5 of 5) |
| `startup_rules` | the workspace rules of the config apply to a window open before the daemon and to one after | pass | pass |
| `focus_follows_mouse` | the pointer over a window gives it the focus when it is on, not when it is off | pass | pass |
| `lifecycle` | a daemon that stops and starts again takes the windows that were in view up again | pass | pass |
| `lifecycle_hidden` | a window put away on another workspace is managed again after a restart, and those in view are tiled | pass | pass |
| `layouts` | eight layouts, with three and with five windows: each window where the layout says, none over another | pass | pass |
| `floating` | `toggle-float` takes a window out of the tiling, the others take its room, and again it is tiled | pass | pass |
| `minimizing` | a minimized window leaves its room to the others, and a restored one takes it back | pass | pass |
| `padding` | more workspace padding moves the windows in from the edge, more container padding shrinks them, and setting it back restores them | pass | pass |
| `maximizing` | `toggle-maximize` gives a window the work area and the daemon names it, the others stay put | pass | pass |
| `workspace_moves` | `move-to-workspace` takes a window and the focus to another workspace, `focus-last-workspace` goes back and forth | pass | pass |
| `resize_commands` | `resize-axis horizontal increase` and `decrease` widen the focused window and give the width back, with no pointer | pass | pass |
| `promoting` | `promote` puts the window on the first tile and keeps its focus | pass | pass |
| `flipping` | `flip-layout horizontal` mirrors every window's place, and flipping back restores it | pass | pass |
| `pausing` | a paused daemon leaves a new window alone, and an unpaused one takes the next | pass | pass |
| `tiling_off` | `toggle-tiling` leaves the windows where they are, a layout change moves none, and tiling again tiles them | pass | pass |
| `scrolling` | in the scrolling layout each window in turn has the focus and is inside the work area | pass | pass |
| `monitors` | the monitor commands (`send-to-monitor`, `move-to-monitor`, their cycle, workspace and named forms, `focus-monitor`, `cycle-monitor`, `focus-monitor-workspace`, `cycle-move-workspace-to-monitor`, `focus-monitor-at-cursor` where there is a pointer) put each window on the monitor and workspace asked for, tiled in its display, and the focus where each leaves it | not run | not run |
| `layout_flip` | `layout_flip` in the config (`Horizontal`, `Vertical`, `HorizontalAndVertical`) mirrors a workspace's layout from the start, `flip-layout` turns it off and on, and it is the named workspace's own | not run | not run |

`rules` passes on kfde in every check. One run of the suite that holds it also failed a check that
is not the scenario's, that the daemon had exited after `komorebic stop`; the next run passed it,
and the cause is not known.

## What the last runs said

The table above says what each scenario checks. This one is made from the runs themselves, by
`scripts/scenario-status`, from the logs of the suites on KDE and the result files of kfw
(`result.json` for each scenario), so it says what happened and not what somebody wrote. A scenario
a run did not include shows as `-`; kfw's counts are of the checks labelled `shared`. Regenerate
it after a run:

```sh
scripts/scenario-status --kde LOG... --windows DIR --update docs/dev/BEHAVIOUR.md
```

<!-- scenario-status:begin -->
| scenario | kde | windows |
|---|---|---|
| `closing` | pass 5 | pass 5 |
| `commands` | pass 237 | pass 237 |
| `config_reload` | pass 8 | pass 8 |
| `flipping` | pass 3 | pass 3 |
| `floating` | pass 7 | pass 7 |
| `focus` | pass 7 | pass 7 |
| `focus_follows_mouse` | pass 7 | pass 7 |
| `hotplug` | pass 9 | FAIL 1 of 9 |
| `layouts` | pass 50 | pass 50 |
| `lifecycle` | pass 7 | pass 7 |
| `lifecycle_hidden` | pass 6 | pass 6 |
| `locking` | pass 6 | pass 6 |
| `maximizing` | pass 5 | pass 5 |
| `minimizing` | pass 4 | pass 4 |
| `monocle` | pass 7 | pass 7 |
| `moving` | pass 4 | pass 4 |
| `padding` | pass 7 | pass 7 |
| `pausing` | pass 4 | pass 4 |
| `placement` | pass 5 | pass 4, 1 known |
| `promoting` | pass 4 | pass 4 |
| `resize` | pass 8 | pass 8 |
| `resize_commands` | pass 4 | pass 4 |
| `rules` | pass 9 | pass 9 |
| `rules_class` | pass 5 | pass 5 |
| `rules_exe` | pass 5 | pass 5 |
| `scrolling` | pass 4 | pass 4 |
| `stacks` | pass 13 | pass 13 |
| `startup_rules` | pass 6 | pass 5 |
| `tiling` | pass 10 | pass 10 |
| `tiling_off` | pass 6 | pass 6 |
| `unknown_monitor` | pass 0, 2 known | pass 0, 2 known |
| `workspace_moves` | pass 8 | pass 8 |
| `workspaces` | pass 8 | pass 8 |

Only on one port:

| extra | kde | windows |
|---|---|---|
| `commands` | pass 33 | - |
| `core_e2e` | pass 63 | - |
| `drag_resize` | pass 9 | - |
| `hotplug` | pass 19 | - |
| `placement` | pass 5 | - |
| `windows_cloaking` | - | pass 4 |
| `windows_config_file` | - | pass 3 |
| `windows_display_change` | - | pass 7 |
| `windows_padding` | - | pass 2 |
| `windows_second_display` | - | pass 6 |
| `windows_tracking` | - | pass 3 |
| `workspace_rules` | pass 10 | - |
<!-- scenario-status:end -->

## What they have found

**`ignore-rule` drops a rule whose id an earlier rule has, whatever the kind.** `komorebi`'s
`IgnoreRule` handler compares only the ids, so `ignore-rule exe e2e-left` after `ignore-rule
class e2e-left` adds nothing and the window is managed. Seen on kfde; the scenarios use different
names (`rules_exe`). kfw does the same.

**A window sent to a monitor that does not exist was lost, on both ports. Fixed.** After
`komorebic send-to-monitor 7` (or `move-to-monitor 7`) on a desktop with one monitor, the daemon
stopped managing the focused window and left it on the screen with nothing tiling it. On kfw this
was seen on a Windows guest: three windows managed and present, then two managed and three
present, with the command exiting 0 and printing nothing. On kfde `core-e2e` failed the scenario's
check on a real KWin, and the windows after it. The cause:
`WindowManager::move_container_to_monitor` took the focused container out of its workspace with
`remove_focused_container()` and only then looked the monitor up, so the container was gone when
the error was returned; `move_workspace_to_monitor` did the same with the workspace. kfw fixed it
upstream (`837ca257`: check the index first, so the command fails and nothing moves) and the same
fix is in kfde (`017edb40`, with a unit test for each function). On a real KWin `core-e2e` passes
`commands.unknown_monitor` with no known-failure marker (292 checks, 0 failed). On kfw it is fixed on
the `next` branch and a Windows run there passes it (12 checks, no marker); the Windows runner
drops the known-failure marker by itself when the commit under test has the fix, and records it as
a known failure on `0ffc502c`, which does not. `commands.unknown_monitor` can still take a
`known_failure` reason, which `Checks.known_failure` records: such a check does not fail the suite,
and one that holds does, to say the marker can go. The sweep of `commands` is not affected on kfde,
which has two outputs, and on kfw the sweep plugs in a second display first.

**`placement` on kfw.** The rule `initial_window_placement_rules: 2`, with the place as a JSON
number, makes `komorebi.exe` exit at start with `data did not match any variant of untagged enum
InitialWindowPlacementRules`, while the string `"2"` works, and kfde accepts both. kfw's own docs
for the type say an integer is accepted, but its `PlacementTarget` deserializes with
`String::deserialize`, nothing in kfw tests the integer form, and its `schema.json`, generated
from the enum rather than the hand-written deserializer, describes the type as objects
(`{"Placement": ...}` or `{"Index": n}`), which matches neither form. It is a known failure to follow up on later: `placement.run` takes the
description of the case and the reason, and records the case that way, whether the daemon does not
start or puts the windows elsewhere. The other four cases give the same order on both ports.
Fixed upstream in kfw (`3b424dd7`: accept an integer as well as a string, and a schema written by hand);
kfde already had the same fix, and its unit test is in `komorebi/src/core/mod.rs`. A Windows run
of kfw's `next` passes all 5 cases.

**`hotplug` on kfw (fixed upstream).** After a display is unplugged, Windows reports one display within a second,
but kfw's `komorebic state` can keep listing two monitors for as long as it is watched (90 s,
every round of a probe, on master as well as on `0ffc502c`). The daemon's log loops: "potential
monitor removal detected (2 vs 1), verifying in 3s", "verified monitor removal", "removing
disconnected monitors", and again, about every 3 seconds, and the removal never takes effect.
The kfw session's reading, not proven: in `monitor_reconciliator/mod.rs` a monitor counts as
still attached when `serial_number_id` or `device_id` is equal, and a virtual display has no
EDID serial, so `None == None` makes the removed one match the primary; the loop before it
guards with `if let (Some, Some)` and this one does not. Real monitors have serial numbers, so it
may show only with virtual displays. `hotplug` passed 19/19 in several runs and failed that one
check (18/19) in two of three runs of a five-run series, so it is not marked a known failure: a
known failure that holds fails the suite, and this one holds only sometimes. kfde is not
affected: it has no reconciliator loop of this kind, and `hotplug` passes 34/0 there. The kfw
session's reading was right: `None == None` made a display without a serial number match the one
that stayed (and the shared `UNKNOWN` device id did the same for displays with no device path).
Fixed upstream in `5c8e4770`: a serial number identifies a display only when both have one and it
is not empty, a display with no device path is told apart by its name, and the removal goes by index.
kfde needs none of it: its displays are told apart by the serial number when KWin gives one and by
the output's own name when it does not, so two outputs never match each other without a name.
The fix is on kfw's `next`: `hotplug` passed 19/0 in five of the six runs there that completed
(before the fix it failed in about two of three); the sixth crashed with a connection reset to the
guest agent before any check, which is being looked into and is not a check failure. One green run
was thin evidence, so this stands on those five.

**`focus_follows_mouse` on kfw.** The scenario's last check failed on kfw five times in five, and
a replica of its steps passed. The scenario was at fault: it moved the pointer over the window that
already had the focus, waited for a focus that was already there, and moved on at once, so that a
platform that follows the pointer by a hover delay, as Windows' active window tracking does,
never saw the pointer rest there. With the pointer resting for a moment it passes, three times in
three on Windows. A scenario that waits for a state that already holds waits for nothing.

**`resize` on kfw.** Dragging the edge between two tiles by 30, 60 and 120 pixels moved it by 27,
53 and 96 on kfw's master, and by 27, 51 and 106 on its animation branch (`0c9acdf4`), where kfde
moves it by exactly the pointer's distance. The pointer is not the cause: the same drag of the
border of a window that nothing manages moved it by 30, 60 and 120 exactly, with the cursor where
it was asked to be. The loss is in what kfw does with the drag.

A recording of a 60-pixel drag, made with komorebi-recorder on the Windows guest, shows where.
The window grew steadily from 596 to 656 pixels, exactly the distance dragged, which is why the
unmanaged window followed the pointer. kfw's `MoveResizeStart` handler (`process_event.rs`)
reads the rectangle the drag started from with `WindowsApi::window_rect` when it handles the
event, which the recording puts about 100 ms after the drag began, when the window was already
603 to 608 pixels wide. The distance kfw measured was 656 - 603 = 53, and its trace log shows the
resize it asked for with a delta of 106, twice that. A longer drag moves further before the event
is handled, so the loss grows with the distance, as measured. A fix that reads the starting
rectangle from the tile in `latest_layout`, or in the event callback before the event is queued,
was tried afterwards: kfw's `0ffc502c` ("measure a drag from where the tile was") passes the scenario, a 60 pixel drag widening the left tile from 596 to 656 and nothing else moving. kfde reads nothing of the kind and passes. The recording is on the machine that
made it (`~/e2e-w0/rec1/resize/session/rec`), and the write-up is `W1.md` in kfw's tree.

Nothing else moved: no tile went up
or down, none overlapped, and every window stayed inside the work area.

**The padding the state says is not the padding the frames show.** On kfde the state says a
workspace padding of 0 and a container padding of 0, while the windows sit 10 px in from the work
area; on kfw the state says 10 and 10, and the frames of three tiled windows sit 27 px in, 7
more than the 20 the state predicts, on every edge. The kfw 7 px is explained by the kfw session:
it is the room for the borders (a border width of 8 and an offset of -1), taken off the layout when
borders are on, so windows sit padding + container padding + 7 px from each edge and twice
(container padding + 7) apart; a Windows extra checks it. The kfde offset is not explained.
`padding` is not affected: it measures from the padding it sets itself.

**A title change does not make the daemon look at a window again. Decided: kfw's behaviour.** A
window an ignore rule matched when it opened stays unmanaged when it gives itself another title,
and a managed window stays managed. kfw's `TitleUpdate` arm is empty; kfde used to announce a
window it did not have again as a `Show` on a title change, so that a rule on the title it has now
decided (windows are often announced before their title is set), and a window that renamed itself
away from an ignore rule was tiled. The `title_rule` scenario found the difference (kfw's first
Windows run: the window was never managed). The decision is that the ports match, and kfde changed:
`WindowManagerEvent::TitleUpdate` does nothing, the unit tests in `komorebi/tests/rule_commands.rs`
say so, and `title_rule` asserts it on both ports. Passes on a real KWin (5 checks). The price on
KDE: a window that opens with no title and gets one later is decided by the rules on the empty
title only.

## What the scenarios had to allow for

These are differences between the ports that a scenario does not hide, and are not failures:

- **A terminal's size.** A `foot` window sizes itself to whole cells of its font, so its frame is
  up to a cell short of the tile (924 by 504 for 925 by 505 in a recorded state). kfw's test window
  has no granularity. `Platform.tolerance` is a cell on kfde and 2 pixels on kfw, for the rounding between the DWM
  frame and the layout (whether 0 would pass there has not been tried).
- **What "put away" is.** A stack's other windows, the windows of another workspace and those
  behind a monocle are on a parking desktop on kfde, and cloaked on kfw. `Platform.hidden` says it
  for both.
- **The work area.** A kfw screen of 1280 by 800 has a work area of 1280 by 752, because of the
  taskbar. A scenario asks the platform for it.
- **Identity.** A window is told by the title the scenario gave it. kfde's state has its own ids
  for windows and monitors, and kfw's has handles, so a scenario never compares them.
- **State shape.** The two `komorebic state` outputs differ in how a window is written and in a few
  fields each. `core/snapshot.py` reads both, and its tests read a state recorded from each.
- **What a restart takes up.** `lifecycle` expects every window that was in view to be managed
  again. On kfw `komorebic stop` uncloaks a window that was on another workspace, and a new daemon
  manages it on the first workspace, but a minimized window stays minimized and is not taken up.
  The scenario counts the windows that were in view, so it does not ask for the minimized one.

## Is it stable

On kfw, five consecutive runs of 14 scenarios each (the 13 without `hotplug` and
`focus_follows_mouse`, plus `startup_rules`) on restored guests, komorebi `0ffc502c`, with the
golden image built from scratch by the repo's scripts, had no failure but the two known-failure
markers. `hotplug` is outside that set, because of the monitor-removal finding above. An earlier
set of five had one run lose its agent for over a minute, mid-sweep, which nothing explains.

## Does a broken daemon turn them red

On kfde, with a real KWin: `komorebi/src/workspace.rs` was changed, for one run and never
committed, to make every tile 200 pixels too wide after the layout is calculated, so that tiles
overlap. `core-e2e` failed 7 checks and passed 139: the overlap checks of the suite's own first
tiling and of the `tiling` scenario (two each), the one in the suite that says two windows share
the screen again after a close, and the "not on each other" checks of `stacks` and of `closing`
or `lifecycle`. Nothing else turned red, so the checks name what
is wrong.

The same mutation on kfw, in a scratch worktree of `0ffc502c`: all 14 scenarios ran on restored
guests and 65 checks failed, every one of them about geometry: windows inside the work area, not
on each other, tiles sharing an edge (the precondition of `resize`), and each of the eight
`change-layout` lines of the `commands` sweep. `rules` (9) and `placement` (4) stayed green,
because they only check the order of windows. Nothing else turned red.

## What is not covered

- Pointer input on a second display, on Windows (kfw). `has("pointer")` is true, but the pointer
  is QMP's absolute tablet, which Windows maps onto the first display only: with a second display
  at (1280, 0) the whole tablet range gave cursor x 0..1279. A scenario that drags across monitors,
  or acts on a second display with the pointer, cannot run there; none exists. Source: kfw's
  `e2e/platforms/qemu_windows/W1.md`, "The pointer with two displays".
- Suspend and resume, which no guest can test.
- `commands` on kfw under load: it passed 247/0 in the two of four repeat runs that had the host
  to themselves, and the two that ran beside a VM suite and a build failed, with `komorebic state`
  replies slowing from milliseconds to about a second. Load is the likely cause, not shown.
- Most of what is particular to one port: kfw's cloak and borders, kfde's parking desktop and
  KWin effects. Those stay in the suites of each port.
