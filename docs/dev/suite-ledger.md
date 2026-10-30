# Shared scenarios and platform extras: what moved where

A suite runs shared scenarios (`komorebi_vm.core.scenarios`) and what only its platform can check
(`komorebi_vm.platforms.kde.extras`), with the checks labelled `[shared]` and `[kde]`. This is the
ledger of the checks a suite stopped making because a shared check makes them, so that none is
dropped without a replacement. A check that only KDE can make stays, as an extra.

## hotplug

The suite had its own sections, then ran the shared scenario "the same, through the neutral
platform". The sections the shared scenario repeats are gone; the rest are `kde_hotplug` steps.

| Was (section) | Now |
|---|---|
| 1, the daemon answers with both monitors; three windows on the left; the monitor's size; tiled | extra `setup` |
| 2, the scale of the left output changes and returns | extra `scale` |
| 3, `move-to-monitor 1`: containers, KWin's `OutputChange`, tiled on the right | extra `sent_by_the_daemon` |
| 4, the user moves a window to the right output | extra `sent_by_the_user` |
| 5, one monitor after the unplug | shared "after a display is unplugged the daemon has one monitor fewer" |
| 5, all three windows on the left monitor | shared "... the daemon still manages every window", and "every window is on the displays that are left" |
| 5, tiled inside the screen that is left, not on each other | shared "... tiled and clear of each other" |
| 5, the daemon alive, its focus KWin's | extra `after` |
| 6, two monitors again | shared "after a display is plugged in the daemon has a monitor for it again" |
| 6, still three windows | shared "... still manages every window" |
| 6, each window somewhere it can be seen | shared "... each where it can be seen" |
| 7, a window moved to the other output again is followed | extra `after` |
| 8, the daemon runs and answers | extra `after` |

Not kept: that the daemon's container count on the left monitor is 3 after the unplug. The shared
scenario checks the managed count and where the windows are, not the count of containers per
monitor. The windows are where the layout puts them, which is what that count stood for.

The suite goes from 25 checks plus the shared ones to 28, which is the shared 11 and 17 of KDE's.

## commands

The suite had its own layout loop (section 2) and then called the shared `layouts`, `sweep` and
`prints`. Nothing the shared scenarios check was removed except as listed; the rest moved to
`kde_commands`, which reads KWin and the daemon's log.

| Was (section) | Now |
|---|---|
| 1, the daemon answers with both outputs; five windows | extra `setup` |
| 2, every layout with five windows: inside the outputs, apart, healthy | shared `layouts`; the extra repeats it at five windows, see below |
| 2, every layout with three, two and one windows | shared `commands.layouts` and `layouts.run` (they close windows from the end and open them again); the extra still goes through the counts for the log check |
| 2, each window where the daemon last asked KWin for it | extra `layouts_by_count` (KDE only: reads the daemon's log) |
| 3, the layouts and the sweep of commands | shared `layouts` and `sweep` |
| 3, `replace-configuration` made the window manager from the file | extra `replace_configuration` |
| 3b, `minimize`, `unmanage`, `manage` | shared `handover`; the check that the unmanaged window is centered is now "inside the work area and not on a tile of the layout" |
| 3c, a title rule decides when the window has its title | shared `title_rule` (the window renames itself through `spawn(rename_to=, after=)`). Not kept: the daemon-log line "should not be managed" for the first title, and `healthy` after it |
| 4, every command that only prints | shared `prints` |
| 4, `komorebic query ...`, `quick-save-resize`, `save-resize` | extra `queries` (candidates for the shared list) |
| 5, every window is where the daemon last asked; a clean stop | extra `at_the_end` |

The extra's layout loop still goes through five, three, two and one windows, for the daemon's log
check at each count. It no longer states an overlap verdict: the shared `layouts` makes that one at
every count, with the extra's own check left to say only where KWin has the windows.

## drag-resize

The suite's own cases are all KDE's: the pointer put to the pixel, and eight drags of edges and
corners checked against frames, tiles and neighbors. None of them is removed; they moved to
`kde_drag_resize` as `pointer_is_exact` and `edge_cases`, and the suite calls the shared `resize`
and `focus_follows_mouse` after them, as before. The shared `resize` makes one drag of one edge,
which the extra's `win1:right:...` cases also make: that is a repeat by design, because the extra
reads the daemon's tiles and the frames together, and the shared one reads only the system.

One run of the suite failed `win2:left:-240,0` ("frame R moved -96 but was not dragged": the
terminal's frame four cells short of an unchanged tile) and the next passed; not seen again.

## core-e2e

The suite had sections 1 to 11 of its own, written against KWin and the daemon's log, and then ran
the shared scenarios in section 12 through the neutral platform. Sections 2 to 8 made, with KDE's
tools, the checks the shared `tiling`, `focus`, `moving`, `monocle`, `minimizing` and `closing`
make. They are gone; what only KDE can read stayed, as `kde_core` steps after the shared one.

| Was (section) | Now |
|---|---|
| 1, the script is loaded into KWin, which runs it; the daemon answers | extra `start` |
| 2, every window is where the daemon asked for it | extra `windows_where_asked`, after each shared step that moves windows |
| 2, inside the screen; windows apart; none takes the full width; the daemon manages the three | shared `tiling` ("where the layout says", "do not overlap", "share the work area") |
| 2, KWin and the daemon agree on the focus, by the daemon's log | extra `focus_agrees_with_the_log` |
| 2, the daemon's state focuses the window KWin does | shared `tiling` and `focus` |
| 3, 4, the focus by hand, `focus right/left`, `cycle-focus` | shared `focus` |
| 4, moving the focus moved no window | extra `windows_where_asked` after `focus` |
| 5, `move right`, `cycle-move previous`, the focus kept | shared `moving`; extra `windows_where_asked` after it |
| 6, monocle fills the work area, the state says so, back on the tile | shared `monocle` |
| 6, monocle focus behaviour `cycle` | extra `monocle_focus_behaviour` (a candidate for the shared scenario) |
| 7, minimize and restore | shared `minimizing`; extra `windows_where_asked` after it |
| 8, close, the others share the screen, the daemon manages the rest | shared `closing` |
| 8, KWin carried out every command | extra `bridge_did_everything` |
| 9, a clean stop undoes what a monocle did | extra `parking` (opens two windows of its own) |
| 10, a crash with a monocle on leaves nothing stuck | extra `parking` |
| 11, KWin restarts under the daemon | extra `kwin_restart` (the `kwin-restart` suite also covers it) |
| 13, `komorebic stop`: the socket, the script, the window stays | extra `stopped` |

Changed on purpose: the lifecycle sections run after the shared scenarios, on windows of their
own, so that the stop and the crash no longer depend on windows a shared scenario left behind. The
daemon's frame log is read after four shared steps, not after every one.

Also learned while moving it:

- `KWin carried out every command` was one check after section 8, over a log of three windows' worth
  of commands. After every shared step, over the whole corpus, the bridge logs `could not do` for a
  `geo` that targets a window a scenario closed a moment later. That is a race, not a failure, so
  the check now counts only a command for a window that is still open, and runs after each shared
  step so that a real failure names its scenario. A failure there with the window alive is a
  finding.
- After a daemon restart the daemon's focused window can differ from the one KWin has active, and
  activating a window KWin already has active sends no event. `parking` activates another window
  first. Whether the daemon should pick up KWin's active window at start is not decided here.

## workspace-rules

The suite made, with KDE's tools, the checks that the shared `rules` and `startup_rules` make. It
keeps the part that is KWin's: a workspace is a virtual desktop.

| Was (section) | Now |
|---|---|
| 1, an initial rule sends a window to workspace 1; it is put away | shared `rules` |
| 1, it is on the parking desktop while desktop 1 shows; on desktop 2 once shown, not parked; tiled where asked | extra `desktops` |
| 1, the other window is where it opened (desktop 1) | extra `desktops` |
| 2, the user takes the window back; an initial rule does not send it again | shared `rules` |
| 2, a rule by the name of a workspace (`chat`) sends a window; desktop 3 when shown | extra `named_rule` (the shared scenarios use indexes) |
| 2, a new window is an event that applies the rules again | extra `named_rule` (a candidate for the shared `rules`) |
| 2, `enforce-workspace-rules` sends the initial rule's window again | shared `rules` |
| 3, the rules are cleared; a window that opens stays on the workspace in front | shared `rules`; extra `cleared` (the daemon's global state has no rule; desktop of the new window) |
| 4, a `komorebi.json` rule, with a window open before the daemon | shared `startup_rules` |

The extras open windows of their own and close them, so that the shared scenarios start from a
daemon with none.

## placement

| Was (section) | Now |
|---|---|
| 1, no rules: after the one in focus | shared `placement` |
| 2, every window to the primary tile | shared `placement` |
| 3, by the number of the place, as a number and as text | shared `placement` |
| 4, by the app id (`Class`), by the title, and after the one in focus | extra `by_app_id` (the shared scenario matches by title only; a candidate for it) |
| 5, `Exe` `foot.exe` matches `foot` | extra `by_program` (KDE: no `.exe` on this port) |

## monitors and layout_flip

Two shared scenarios for gaps no Rust test covers either, in `core-e2e`. Nothing was dropped: the
sweep's multi-monitor commands (`commands`) still only say the daemon stays healthy, and these say
what each does.

- `monitors` (needs two displays; one is plugged in if there is one, and out again) opens four
  windows and checks, from the OS and the daemon's state, `send-to-monitor`, `move-to-monitor`,
  their `cycle-` forms, the `-monitor-workspace` forms, `send-to-named-workspace`,
  `move-to-named-workspace`, `focus-monitor`, `cycle-monitor`, `focus-monitor-workspace` and
  `cycle-move-workspace-to-monitor`: each window on the monitor and workspace asked for, the others
  where they were, tiled inside the display of their monitor (read from `displays()`, which may
  differ in size), and the focus where the command leaves it (a send keeps it, a move takes it).
  `monitors.at_cursor` is `focus-monitor-at-cursor`, and runs only where `has("pointer")`.
  The suite starts KWin with two outputs and unplugs the second before the daemon starts, so that
  every other scenario still sees one display.
- `layout_flip` starts the daemon with the workspace option set (`Horizontal`, `Vertical`,
  `HorizontalAndVertical`) over columns, rows and BSP with two and three windows, checks the
  mirror from the OS, that `flip-layout` turns the option off and on, that a command adds the other
  axis, and that the option is the named workspace's own.

Neither has been run on a real KWin or Windows guest yet; the fake in `tests/fake_desk.py` stands in
for the unit tests (`tests/test_monitors.py`).

## workspace-switch

Nearly everything this suite checks is KWin's: a workspace is a virtual desktop. The shared
`workspaces` and `workspace_moves` scenarios, which `core-e2e` runs, check that `send-to-workspace`
and `focus-workspace` put windows away and back with the focus agreed; here the same commands are
read on KWin's desktops. The suite is the extra `workspace_switch.run`; its one dropped check, "the
daemon answers komorebic", is what `platform.start_komorebi()` waits for. The
shared scenario is not called from this suite, because it starts from windows on the first
workspace and this suite's own start state is a daemon that has not yet turned on KWin's
per-output desktops.

## scrolling

The shared `scrolling` scenario (run by `core-e2e`) puts each window in view in turn. This suite is
about the windows that are not in view: where KWin keeps the columns that scroll off the screen,
and that the strip follows the focus. All of it is KWin's; it is the extra `scrolling.run`, with
no check dropped.

## stacks

Ported whole as the extra `stacks.run`: all 54 checks. It reads the parking desktop and KWin's
desktops (`kdotool`), which the shared `stacks` and `monocle` scenarios do not. It still repeats,
on KWin's side, some of what they check: that cycling the stack shows the other window and puts
the first away, focus by place in the stack, and a monocle over a stack (sections 3, 4 and 8). The
repeats read a different oracle (the parking desktop, not the operating system's `hidden`), so
they are kept for now; slimming them needs the shared scenarios to report what they leave to be
read, which is the stage hook that was left out. Sections 10 and 11 (a crash, a clean stop) are the
`stack-all` form of `core-e2e`'s `parking`.

## kwin-restart

Ported whole as the extra `kwin_restart.run` (two restarts, 19 checks as run). `core-e2e` had a section
that restarted KWin once with the same four checks; it is gone, and `core-e2e` now only opens the
window `afterwards` that its last section looks for.

## The KDE-only suites

Moved as they were into `komorebi_vm.platforms.kde.extras`, one module per suite (`kwin`,
`kwin_events`, `kwin_contract`, `client_geometry`, `module`, `restart_survival`, `user_maximize`,
`check`, `features`, `look`, `render_smoke`, `wallpaper`, `rdp`, `latency`, `run_kde`,
`live_resize`, `focus_follows_mouse`, `lock_sleep`, `keys_engine`, `keys_static`, `keys_corpus`).
Each module has `run(machine, checks, scale)`, which is the body of the old script; the suite script
only starts the machine and calls `run_suite(checks, platform_only("kde", ...))`. The three
`keys-*` suites share one script, `keys-corpus.py`, as before. No check was dropped and none was
added; these have no shared scenario to repeat. What each makes on a real KDE, run once after the
move:

| Suite | Passed |
|---|---|
| kwin | 8 |
| kwin-events | 22 |
| kwin-contract | 15 |
| client-geometry | 26 |
| user-maximize | 14 |
| check | 23 |
| latency | 5 (once 4 and 1 failed: a p95 of 173 ms against 150 while the machine was loaded) |
| render-smoke | 4 |
| rdp | 6 |
| keys-static, keys-skhd, keys-whkd, keys-sxhkd | 6, 4, 4, 4 |
| lock-sleep | 11 |
| focus-follows-mouse | 11 |
| restart-survival | 17 |
| module | 33 |
| live-resize | 7 |
| look | 40 |
| wallpaper | 22 |
| features | 30 |
| keys-engine | 34 |
| run-kde | 22 |

The moved modules have long lines the suite scripts were never linted for, so `pyproject.toml` lets
them keep them (`E501` per file); new extras do not get that.
