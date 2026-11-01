# macOS hand-check plan: one runbook for the Mac pass

Status (wip-cand8, b6f4c2ae): OPEN. This is the ordered runbook that replaces reading
[macos-handcheck.md](macos-handcheck.md) top to bottom; that document stays as the source (rows A1-A..., B, C, D,
E, F, G, H and the review's H1-H12), and every item below says which row it came from. **Nothing here has been
run on a Mac as a daemon.** What did run, on a Mac Mini: `cargo build`, `clippy -D warnings`, `cargo test` (665
passed, 0 failed, 1 ignored at candidate 8) and the nix builds of `komorebi`, `komorebic`, `komorebi-bar` and the
darwin checks. No window has been tiled, hidden, focused or bordered by these builds.

How to read an item: **Steps**, **Expect**, **Stop / roll back**, **Introduced by** (jj bookmark or change id;
`jj log -r <id>` in the repository, commit ids move when a revision is rewritten). Items marked `[2 displays]`,
`[trackpad]`, `[physical]` need a second screen, a trackpad gesture, or someone to plug cables; `[2nd person]` is
useful where you cannot watch the screen and type at the same time. Block times are for a person who has the build
ready.

Order is by what could make the daemon unusable first. If a block fails, stop there, write down the block, item,
macOS version and the log lines, and do not continue to later blocks that depend on it (blocks 3 onwards assume
hiding and the event loop work).

## Do NOT run these on a Mac that has a live komorebi

The owner's Mac Mini runs their own komorebi. This runbook is for another Mac, or for a clean run on the Mini
after quitting the live daemon on purpose. Reasons are in the code: a second `komorebi` process exits with
"komorebi is already running" (it counts processes named `komorebi`), and on exit or Ctrl-C a daemon deletes
`$DATA_DIR/komorebi.sock`; all `komorebic` commands talk to the socket in the user's data directory, so they
reach whichever daemon owns it.

- Do not run `komorebic stop`, `komorebic stop --bar`, `komorebic start`, `komorebic quickstart`,
  `komorebic enable-autostart` or `disable-autostart` (they run `launchctl load/unload`), `pkill`/`killall`
  `komorebi` or `komorebi-bar` (`stop --bar` runs `pkill komorebi-bar`), or Ctrl-C the test daemon while the live
  one shares the data directory.
- Do not run any `komorebic` command that changes state (`replace-configuration`,
  `ignore-rule`, `session-float-rule`, `toggle-pause`, `focus-*`, `stack`, ...) unless you know which daemon it
  reaches. `komorebic state` and `komorebic --help` are read-only.
- Do not `kill -STOP` or `kill -CONT` a pid you did not just start; items 8.2 and 8.3 are the only ones that use
  signals and must use the pid of the test daemon or test bar, taken from `pgrep` right after you start them.
- Do not `cargo install`, `brew install/upgrade komorebi*` or overwrite the installed binaries; run from
  `target/debug`.
- Do not use your real `~/.config/komorebi`: use the clean config in 0.2.
- Do not test on the live Mac's Accessibility grant by removing and re-adding it in System Settings; grant the
  terminal that runs the test build.

A clean run on the Mini needs: quit the live daemon and bar in the way you normally do, confirm with
`pgrep -l komorebi` that nothing is left, run the runbook, then start your own again. `launchd` will restart an
auto-started daemon unless you disabled autostart yourself first.

## Block 0. Prerequisites and a build you can trust (about 25 min)

**0.1 Build and test.** From the repository root, with a `.git` checkout (or `COMMIT_HASH=dev`):

```sh
cargo build --locked -p komorebi -p komorebic -p komorebi-bar
cargo test  --locked -p komorebi-macos -p komorebic -p komorebi-client -p komorebi-bar-core -p komorebi-bar -p komorebi-core -p komorebi-config -p komorebi-protocol
cargo clippy --locked -p komorebi -p komorebi-macos -p komorebic -p komorebi-client -p komorebi-bar -p komorebi-bar-core --all-targets -- -D warnings
```

Never name `-p komorebic-macos` together with `-p komorebic` (same binary name). Expect: 0 failures (the Mini
saw 665 passed, 0 failed, 1 ignored). Roll back: nothing to do; if a test fails, stop and report its name.

**0.2 A clean config and data directory.** Make an empty directory for the run, put a minimal
`komorebi.json` in it (copy `komorebi-for-mac/docs/komorebi.example.json`), and set `KOMOREBI_CONFIG_HOME` to it.
The data directory (socket, logs) is the user's `~/Library/Application Support/komorebi`, which this variable does
not change; use another Mac or a throwaway macOS user if you cannot guarantee the live daemon is not running.
Expect: `pgrep -l komorebi` prints nothing before you start.

**0.3 Permissions.** System Settings, Privacy and Security: grant Accessibility and Screen Recording to the
terminal you start the daemon from. Start the daemon in a visible terminal so you can Ctrl-C it:
`RUST_LOG=komorebi=debug target/debug/komorebi 2>&1 | tee run.log`.

**0.4 Baseline for comparison (optional).** Build the baseline ancestor `ff3711dc` of `wip-main` in another
checkout; a recording with `komorebi-recorder` on both builds makes before/after comparisons exact. The items
below say "as before" where this matters.

## Block 1. Startup, event loop and the command path (about 40 min)

The riskiest changes: the daemon now starts from a shared `daemon::run`, the command listener, pause filter,
subscriber push and event channel are the shared transport, and `SocketMessage` is the shared enum.

**1.1 Start and stop.** Steps: start the daemon (0.3); in another terminal `target/debug/komorebic state | head`,
`komorebic global-state | head`, `komorebic --version`, `target/debug/komorebi --version` (expect `komorebi 0.1.0`
with the macOS version crate). Then Ctrl-C the daemon. Expect: state is JSON; the daemon logs the restore of all
windows and exits (code 130), all windows are back where they were, `komorebi.sock` is gone. Stop: Ctrl-C; if the
daemon does not exit, `kill` the pid you started (not `pkill`). Introduced by: `wip-u-pkg2` (one `komorebi`
package, `1326f041`) and the shared transport revisions `nvwvptpv`, `ywwwwpwm`, `kzynysto`.

**1.2 Every command is answered.** Steps: run one command of each family: `focus left`, `move left`,
`resize-axis horizontal increase`, `stack`, `unstack`, `toggle-float`, `toggle-monocle`, `focus-workspace 1`,
`cycle-workspace next`, `toggle-pause` twice. Expect: each returns promptly; the log shows no
"could not acquire window manager lock". Introduced by: `lrqkuuzq`, `sowrttxs` (one `SocketMessage`), `nvwvptpv`.
Also `komorebic debug-window <id>` for a real window id from `state`, if your build lists it in `komorebic --help`.

**1.3 Pause filter.** `komorebic toggle-pause`; while paused only `toggle-pause`, `state`, `global-state`, `stop`
work, everything else is ignored; unpause and check that windows tile again. Introduced by: `ywwwwpwm`.

**1.4 Reload loop (the lock order).** Steps: in one terminal run `for i in $(seq 50); do komorebic
replace-configuration clean/komorebi.json; done` while you open
and close Finder windows and Terminal tabs. Expect: every call answered, no hang, new windows still tile; float,
ignore and manage rules from the config and from `komorebic session-float-rule`, `ignore-rule`, `manage-rule` still apply.
Stop: if the daemon stops answering, copy the log, `kill` the test daemon. Introduced by: `ummqkuvp` 01b9cbed
(one lock order; before it the order could deadlock) and `wip-u-rulematch` (the shared rule matcher: also try a
regex rule and a composite rule).

**1.5 Event channel under load.** Drag a window edge continuously (live resize), pan with the trackpad
`[trackpad]`, and switch workspaces with 10 or more windows. Expect: no stalls, no repeated "drop" warnings, the
layout follows. Introduced by: `ovsplvmn` (shared `EventChannel`).

**1.6 Subscribers.** `komorebic subscribe-socket mytest` in a terminal (and with options): events arrive for focus,
move, workspace change, monitor change, theme and config reload; kill the subscriber (Ctrl-C) and confirm the
daemon keeps answering. Introduced by: `kzynysto`.

Block done when: 1.1 to 1.6 hold. Do not go on if 1.1 or 1.4 fail.

## Block 2. Hiding and restoring windows (about 45 min; `[2 displays]` for 2.1)

The daemon's `Container`, `Workspace` and `Monitor` are now the shared `komorebi-core` types and hiding takes a
`HideSpot`. A bug here leaves windows stuck in a corner or lost.

**2.1 Hide spot on the right display (review H1) `[2 displays]`.** Steps: a window straddling the left edge of
display 2 (drag it half off). Switch workspace on display 2 and back. Repeat with `window_hiding_position:
"BottomRight"` in the monitor config (then `komorebic replace-configuration`). Stacks: `stack` three windows,
`cycle-stack`. Expect: the window is hidden in the `BottomLeft` corner of display 2 (not display 1, not left in
view) and restored to the same frame; only the front window of a stack is visible, the others sit in the corner.
Stop: Ctrl-C the test daemon (it restores every window on exit). Introduced by: `wip-u-kfm2` (Container, Workspace,
Monitor), the `HideSpot` revisions `ywowyyrz` and `losvwsls`, `wip-u-mac-deep` (`wlmxvuso`, `pulywuwk`, BEHAVIOUR:
the corner now comes from the workspace, not the monitor).

**2.2 Stacks, monocle and floating (H2 and the deep-dedup rows).** `toggle-float` a window, switch workspaces
twice: it hides and returns at its old frame. `toggle-monocle`, switch and back; with `window_hiding_position:
"BottomRight"` every other container parks in that corner while monocle is on; `cycle-focus` while monocle is on; `stack-all`;
change `window_hiding_position` in the config, reload and toggle monocle at once: the
corner must be the new one. Introduced by: `wip-u-mac-deep`.

**2.3 Workspaces.** `focus-workspace`, `send-to-workspace`, `move-to-workspace`, `cycle-move-to-workspace`,
`cycle-move-to-monitor` `[2 displays]`: windows of the workspace you left are hidden, those of the new one
restored where they were; focus-workspace by name too. Introduced by: `wip-u-kfm2`, `wip-u-cmd` (the moves are the
shared `komorebi-core::command` since candidate 7).

**2.4 Router arms moved to methods.** `toggle-workspace-layer` twice; `resize-axis horizontal|vertical
increase|decrease`; `session-float-rule` on a window; `ignore-rule exe <name>` and `title`/`class` (matching
windows leave the workspaces); `replace-configuration <komorebi.json>` (windows re-laid out under the new
configuration; with a broken file the daemon is left alone). Introduced by: `wip-u-mac-deep` (`tkpoootr`).

**2.5 Minimize, unminimize, close.** `minimize`, `close`, un-minimize from the Dock; closing removes the window
from its container. Introduced by: `Window` methods taking `&self` (`nrsnmwmo`).

Block done when: no window is ever left in a corner after unstack, quit or Ctrl-C, and `komorebic state` keys of a
container, workspace and monitor are unchanged (a container has `id`, `locked`, `windows.{elements,focused}`; a
monitor ends with `window_hiding_position`).

## Block 3. The window registry and leaks (about 40 min)

The daemon keeps a table from window id to Accessibility element (`ax_registry`) with retained references and
observers. Failures here show as memory growth or a hang, not as a wrong tile.

**3.1 Registry growth (H4).** Note `lsof -p $(pgrep -x komorebi) | wc -l` and `lsmp -p $(pgrep -x komorebi) |
tail -1` (Mach ports). In TextEdit open and close the Open and Save panels 200 times. Expect: no steady growth
(it also answers whether an `AXObserver` holds a port). Introduced by: `oywqqkuy`, `wip-u-kfm3`.

**3.2 Quit apps (H6).** `toggle-pause`, quit an app with managed windows, `toggle-pause`: its windows are gone from
`komorebic state`; with `RUST_LOG=debug` no new errors at quit; open and close 50 windows (Finder, Terminal,
Safari) and quit several apps: the daemon's memory in Activity Monitor does not climb. Introduced by: `wip-u-kfm3`
(`forget_pid`, forget on reap).

**3.3 Native tabs (H7).** Finder or Terminal with two native tabs: switch tab, close the first tab, open a new
tab, focus the window with `komorebic focus`, move and resize it. Expect: the window stays managed, focus works,
`komorebic state` shows one window id per container with no duplicates, no stuck border. Introduced by:
`oqnvyqvw` e0687167, `wip-u-kfm3`.

**3.4 A many-window app and a stopped one (H3).** Chrome with about 20 windows: restart the test daemon and note
the time to ready in the log against the baseline build. Then `kill -STOP <pid of one app with a managed window>`
and press `komorebic focus right` 5 times, then `kill -CONT <pid>`: commands are handled again within the AX
timeout after the CONT; a log line `invalidating application observer for process id` for a live app after the CONT
is the bug. Stop: always `kill -CONT` the app. Use the pid of a throwaway app (TextEdit), never of a system process.

## Block 4. Pointer picks the monitor (about 10 min, `[2 displays]`)

**4.1** An empty workspace on the second display with the pointer over it while the first display is focused:
`komorebic focus-workspace 1`, `focus-workspaces 1` and `focus-last-workspace` first focus the display under the
pointer, then switch its workspace (before, `focus-workspace` and `focus-workspaces` acted on the focused display).
With the pointer on the focused display nothing changes. Stop: move the pointer back and `focus-workspace` again.
Introduced by: `wip-u-ptr` (BEHAVIOUR, owner decision; shared `focus_workspace_number(s)`; macOS check H1 of
section H of the old list).

## Block 5. Settings and configuration reload (about 30 min)

Run settings are one snapshot (`komorebi-settings`) instead of statics. Test with `replace-configuration` or a
restart.

**5.1 Paddings, aspect ratio, layout defaults, display preferences (H9, G.1).** `default_workspace_padding` 20 and
`default_container_padding` 15: tiles 20 px from the work area and 15 apart on every workspace, not only the
focused. Drop the key and reload: padding stays; set 5: 5. `layout_defaults`: set, reload, workspaces use it; remove
and reload: reset. `floating_window_aspect_ratio`: `toggle-float` with centring uses it; remove and reload: 4:3.
`display_index_preference 0 <serial>`: monitor order follows it. Reload during a drag or workspace switch: no
one-frame flash of half-new padding. Introduced by: `wip-u-settings2`, `wip-u-settings3` (`qsxslsmy`).

**5.2 Animation (G.2, BEHAVIOUR).** `komorebic animation true`, switch workspace: a transition plays. During one,
`animation-duration 1000`, `animation-style`, `animation false`: the running transition is not torn, the next one
follows the new values. `animation true --movement` then `animation false`: movement is off again. Reload a file
with `"animation": {"enabled": {"movement": true}}`: only movement animates; with `"enabled": true` and no fps: fps
60; with no animation block: values keep. `animation-fps 30`. `stack` a window: no animation during the stack, back
after it. Introduced by: `mquxryqt` (settings S5).

**5.3 Schema and config loading (H-rows).** A real `komorebi.json` of yours loads with no new warning and every key
works (borders, padding, animations, scrolling, rules, workspaces); `komorebic static-config-schema` equals
`komorebi-for-mac/schema.json`. Introduced by: `kvtvnouo`, `stxmqnop` (StaticConfig from `komorebi-config`).

## Block 6. The rest of the daemon walk-through (about 60 min)

Compare with the baseline build where you can. Source: section C of the old list, with the Introduced-by rows A
and B for bisecting a regression (the B rows only rename and route: a failure there means a call went to the wrong
receiver).

6.1 Focus: `focus left|right|up|down`, `cycle-focus`, `focus-stack-window <n>`, mouse-follows-focus on and off, a tabbed
app (right tab focused). 6.2 Move and resize: `move`, `resize-axis`, `resize-edge`, a manual drag (the layout
puts the window back), live resize of one edge moves the neighbour, a fixed-size app. 6.3 Scrolling workspace (H8)
`[trackpad]`: `change-layout scrolling`, 5 windows, pan with the gesture and with `focus left/right`; scrolled-out
windows park at the screen edge (not the corner) and return on focus; switch workspace and back; consume, expel,
resize a column. 6.4 Borders: colours follow focus and animated windows; vanish and return on workspace switch;
none left behind after closing. 6.5 Animations on and off, ghost images, cancel by a new command mid-way. 6.6 Display
change `[physical]`: plug or unplug a monitor or change the resolution: work areas update, workspaces go to the
right monitors, rules re-apply (this is the thread that may deadlock, see 8.4). 6.7 Sleep and wake `[physical]`:
windows where they were, daemon answers, bar returns. 6.8 Start and stop: `komorebic start --bar`, `start
--await-configuration`, `stop`, `stop --bar` **only on a Mac with no live komorebi**, and `komorebic --help` plus a
few `<command> --help` texts (about 15 help texts now use the Windows wording). Introduced by: rows B and the
`komorebic` revisions `qukywpss`, `zwkwptox`, `sxptsttt` (shared launcher and pan).

## Block 7. The bar (about 25 min)

The bar is now the `komorebi-bar` package (module `macos`) over `komorebi-bar-core`, with `sysinfo` 0.39 and
`starship-battery` 0.11.

**7.1 Renders.** `target/debug/komorebi-bar` against the running test daemon: workspaces, layout, CPU %, memory,
battery % and charging, date and the calendar icon, all sane. `komorebi-bar --schema | cmp - komorebi-for-mac/schema.bar.json`
is silent. Introduced by: `wip-u-pkg-mig` bar revisions (`7d4ea893`, `bdd811f2`), `nottqpms` and the bar-core hooks.

**7.2 Old state and the bar (H10).** `komorebic state > old.json` on the ORIGINAL build; start the new bar against
the new daemon and confirm it renders; the new daemon's `state` parses in the bar.

**7.3 Bar config (H11).** A Keyboard, Media and Systray widget and a `mouse` block in `komorebi.bar.json`: the bar
starts, logs "the Keyboard widget is not supported on this platform and is skipped" (three times) and the `mouse`
warning, and renders the other widgets. Break the file (`"mouse": {"on_scroll_up": 5}` or an unknown widget): the bar
refuses to load it with an error. Introduced by: bar decision (macOS accepts and skips Windows-only keys).

**7.4 Clicks (old M1-M3).** Battery click opens System Settings on the Battery pane (macOS 13 or later; write down
the macOS version and what opened if wrong); CPU and memory click opens Activity Monitor; network opens the
Network pane; storage opens the folder in Finder; update widget (installed version older than the latest release)
shows the new version and the click opens `https://github.com/LGUG2Z/komorebi-for-mac/releases/tag/v<version>`;
after 12 hours (or a small `data_refresh_interval`) it still compares against komorebi-for-mac releases.
Introduced by: `tpwlsxpm`, `tsxllpxz`.

## Block 8. Known issues to confirm (about 20 min; these are expected to misbehave)

**8.1 Unsupported commands (H12).** `echo '"ToggleMaximize"' | nc -U "$HOME/Library/Application Support/komorebi/komorebi.sock"`:
a not-supported reply, no state change, the daemon keeps running.

**8.2 Hung subscriber (H5, known, not fixed).** `kill -STOP $(pgrep komorebi-bar)` (the bar you started in 7),
then `komorebic focus left` 10 times and watch the log for "could not acquire window manager lock": the daemon is
expected to stop reacting once the socket buffer fills. Roll back: `kill -CONT` that bar pid. See
[known-issues.md](known-issues.md) item 4.

**8.3 Deadlock window (old bug A, by reading only).** Spam `komorebic retile` and workspace switches (with
animations) while plugging and unplugging a monitor `[physical]`; the symptom is a daemon that answers no command
(`komorebic state` hangs). Stop: `kill` the test daemon; windows may stay hidden, so quit it with Ctrl-C if it still responds (the daemon
restores windows on exit), otherwise `kill` it and move the hidden windows back by hand.

**8.4 Other bugs on the old list (D, B, C)** that remain by design: raw Accessibility calls outside `Window`, the
update widget second URL (now fixed by `tsxllpxz`), the `cmd.exe`/`explorer.exe` clicks (fixed by `tpwlsxpm`).

## Time and what to report

| Block | Needs | Time |
|---|---|---|
| 0 prerequisites | build, a spare Mac | 25 min |
| 1 startup and command path | 1 display | 40 min |
| 2 hiding and restoring | 2 displays for 2.1, 2.3 (monitor move) | 45 min |
| 3 registry and leaks | Activity Monitor, `lsof`, `lsmp` | 40 min |
| 4 pointer picks the monitor | 2 displays | 10 min |
| 5 settings and reload | none | 30 min |
| 6 walk-through | trackpad, display cables, lid | 60 min |
| 7 bar | none | 25 min |
| 8 known issues | cables | 20 min |
| total | | about 5 hours |

Report per item: block and item number, macOS version, pass or fail, and for a failure the `run.log` lines and
`komorebic state`. A failure in Block 1 or 2 blocks landing any further macOS change; failures in later blocks are
bugs to fix in the revision named under Introduced by.
