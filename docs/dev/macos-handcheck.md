# macOS hand-check list

> **Status (wip-cand4):** OPEN, for the user on a real Mac; only compile and unit tests have run on a Mac Mini, no macOS daemon behaviour has been observed; commands for the Mac are in [build-and-test.md](build-and-test.md).

Status: written by the `kfm` agent for the user, 2026-10-31. Nothing here has been run: macOS cannot be built, linked or tested on the Linux host, only type-checked through a fake C compiler (`tools/fast-check`). Every revision below is `type-checks, not run`. Revision ids are jj change ids (stable) with the commit id they had when this was written (commit ids change when a revision is rewritten; use `jj log -r <change id>`).

How to use it: build the unified (or, until they exist, the per-tree) macOS `komorebi`, `komorebic` and `komorebi-bar`, run `cargo test --workspace` on the Mac first (the 399+ unit tests have never run against these changes), then walk the sections in order. For before/after capture use `komorebi-recorder` against a build of the baseline `wip-main` ancestor `ff3711dc` and the tip. A section is done when the "look for" column is true.

## Status of the Mini runs (2026-10-31)

The real-Mac runs so far (Mac Mini, rustc and clippy 1.93, tip a35573d8) are **compile and unit tests only**: `cargo test --workspace` in komorebi-for-mac passed (daemon 366 passed, 1 ignored; 368 total, 0 failed) and clippy `-D warnings` was clean apart from the two items fixed afterwards. Nothing was run as a daemon: no focus, tiling, borders, animations or bar rendering has been observed on the Mini. Every walk-through item below is still open.

## Prioritised checks from the independent review (H1-H12)

Source: the macOS review of the candidate (S/review/mac.md). It found no must-fix; the fixes it asked for
are the `wip-u-kfm3` revisions (inert application in the registry, register on observe, no forget in
`Window::drop`, `forget_pid` first, forget on reap). Prerequisites: build `komorebi-macos` from the
candidate, grant Accessibility and Screen Recording, run `komorebi` with `RUST_LOG=komorebi=debug`;
use `komorebic state` for state. Order is by risk to a user's day.

1. **H1 hide spot on the right display (HideSpot).** Two displays; one workspace with a window
   straddling the left edge of display 2 (drag it half off). Switch workspace on display 2 and back.
   Expect: the window is hidden in the `BottomLeft` corner of display 2 (not display 1, not left in
   view) and restored to the same frame. Repeat with `window_hiding_position: BottomRight` in the
   monitor config. Stacks: a 3-window stack (`komorebic stack`), `cycle-stack`: only the front window is
   visible, the others sit in the monitor corner.
2. **H3 event latency and stall.** A many-window app (Chrome, about 20 windows): restart komorebi and
   note the time to ready in the log against the original build (registering is now once per observed
   window, which removes the n x n cost of the first revision). Then `kill -STOP <pid>` an app with a managed
   window and press `komorebic focus right` 5 times: commands are handled again within the AX timeout
   after `kill -CONT`; a log line `invalidating application observer for process id` for a live app
   after the CONT is the bug.
3. **H7 native tabs.** Finder or Terminal with two native tabs: switch tab, close the first tab; the
   window stays managed, focus works, and `komorebic state` shows one window id per container with
   no duplicates.
4. **H2 floating and monocle on workspace switch.** `toggle-float` a window, switch workspaces twice:
   it hides and comes back at its old frame. `toggle-monocle`, switch workspace and back.
5. **H8 scrolling workspace.** `komorebic change-layout scrolling`, open 5 windows, pan with the
   gesture and with `focus left/right`; windows scrolled out of view are parked at the screen edge (not in
   the corner) and return on focus; switch workspace and back.
6. **H9 config reload visibility.** Set `default_container_padding`, `default_workspace_padding`,
   `floating_window_aspect_ratio`, `layout_defaults` and `display_index_preferences` in komorebi.json and
   apply with `komorebic replace-configuration` (or restart): padding on every workspace (not only the
   focused one), `toggle-float` aspect ratio; removing `layout_defaults` or `floating_window_aspect_ratio`
   and reapplying resets them (none, and Standard 4:3), while removing a padding key keeps the previous
   value.
7. **H10 old state and the bar.** `komorebic state > old.json` on the ORIGINAL build; start the new
   `komorebi-bar` against the new daemon and confirm it renders; the new daemon's `komorebic state` must
   parse in the bar (workspaces, monitors with `window_hiding_position`).
8. **H6 pause and quit.** `komorebic toggle-pause`, quit an app with managed windows, `komorebic
   toggle-pause`: its windows are gone from `komorebic state`. (That its registry entries are gone
   is covered by the unit test of `forget_pid` and visible only in a debug log.)
9. **H11 bar config.** Put a Keyboard, Media and Systray widget and a `mouse` block in
   komorebi.bar.json: the bar starts, logs "the Keyboard widget is not supported on this platform and is
   skipped" (three times) and the `mouse` warning, and renders the other widgets. Then break the file
   (`"mouse": {"on_scroll_up": 5}`, or an unknown widget name): the bar must refuse to load it with an error.
10. **H12 unsupported commands.** `echo '"ToggleMaximize"' | nc -U $DATA_DIR/komorebi.sock`: a
    not-supported reply, no state change, the daemon keeps running.
11. **H4 registry growth.** Note `lsof -p $(pgrep -x komorebi) | wc -l` and `lsmp -p $(pgrep -x
    komorebi) | tail -1` (Mach ports). In TextEdit open and close the Open and Save panels 200 times.
    Compare the counts: steady growth means entries (and observers) are retained; it would also answer
    whether an `AXObserver` holds a port (not verified).
12. **H5 hung subscriber (known, pre-existing).** `kill -STOP $(pgrep komorebi-bar)`; run `komorebic
    focus left` 10 times and watch the log for "could not acquire window manager lock". Expect the
    daemon to stop reacting once the socket buffer fills; `kill -CONT` recovers it. See the known issue
    below: this is how the original behaves and it is deliberately not changed.

### Known issue, not fixed: a subscriber that stops reading can stall the daemon

`notify_subscribers` writes to each subscriber socket under the window manager lock, and the macOS
connect closure sets no write timeout, so a subscriber that accepted the connection and does not read
(a stopped bar, a debugger) makes `write_all` block once the notification outgrows the socket buffer;
the event thread then holds the lock and every command logs "could not acquire window manager lock".
The original daemon behaves the same. A write timeout was considered and **not added**: the
Windows review found that a write timeout can truncate the JSON notification in the middle (the
subscriber then reads half a message) and does not bound the stall in the way the timeout suggests, so
the fix needs a design (a bounded queue per subscriber written off the lock) and not a one-line
timeout. Until then H5 documents the hang.

## A. Revisions that change what the daemon does (BEHAVIOUR)

| Revision | What changed | Look for on a Mac |
|---|---|---|
| `ummqkuvp` 01b9cbed `fix(mac): take the rule-list locks in one order...` | `window_is_eligible` (window.rs) and `apply_globals` (static_config.rs) now lock floating, ignore, manage, regex in that order; before they locked regex, ignore, manage and ignore, regex, manage. The order is documented at the statics in lib.rs. Same fix as the KDE `ef1d3ef0`. | Run `komorebic reload-configuration` (and `komorebic replace-configuration`) in a loop of 50 while opening and closing windows (new Finder windows, Terminal tabs) at the same time. Before the fix this could hang the daemon (the command is never answered, new windows are not tiled). Expected: no hang, every reload answered. Also check that float rules, ignore rules and manage rules from the config and from `komorebic float-rule`/`ignore-rule`/`manage-rule` still apply to new windows. |
| `nvwvptpv` 866b2840 `use the shared command listener` | The UDS command accept loop and per-client reader are the shared `komorebi-transport` code (`NoPolicy`: no read timeouts, as before); `WindowManager` is a `CommandTarget` (lock with `try_lock_for`). | Every `komorebic` command is answered; a command sent while the daemon is busy gets an answer or a clean error, not a hang; kill the daemon mid-command: the client returns. |
| `ywwwwpwm` 587defe3 `use the shared pause filter` | `allowed_while_paused` lists the commands allowed while paused. | `komorebic toggle-pause`; while paused only `toggle-pause`, `state`, `global-state`, `stop` work, everything else is ignored (and the connection ends); unpause and check windows tile again. |
| `kzynysto` b85827e6 `use the shared subscriber push` | Subscriber sockets are pushed to by the shared `notify_sockets`; the macOS override events are kept. | `komorebic subscribe-socket` (and `subscribe-socket-with-options`): events arrive for focus, move, workspace change, monitor change, theme and config reload; a subscriber that goes away is dropped without breaking the others. The bar is a subscriber, so the bar updating is the check. |
| `ovsplvmn` 5cc38da6 `use EventChannel for the window events` | The window-event channel (capacity 20, try_send, drop warning, `ResizeStep` claim flag) is `komorebi_transport::EventChannel`. | Drag a window edge continuously (live resize) and pan with the trackpad: no stalls, no repeated drop warnings in the log, the layout follows. Move many windows at once (workspace switch with 10+ windows). |
| `lrqkuuzq` a4996ac6 `use SocketMessage from komorebi-protocol` and `sowrttxs` 5c129ab5 `carry DebugWindow as i64` | One `SocketMessage` for all platforms; the wire format must be unchanged. `DebugWindow` takes an i64 (a negative id parses and matches nothing). | Every command of the `komorebic` help works against the daemon; `komorebic debug-window <id>` for a real id; `komorebic state` and `global-state` JSON parse. |
| `qqwoptqt` 975f007a, `yptnnkxv` eb903bcc (animation prefix, Bezier doc) | `AnimationPrefix` gains `Transparency` (ignored by the daemon); schema.json changed the shape of per-prefix animation settings. | Config with `animation.style` / `duration` per prefix (movement, workspace) still loads; animations behave as before. Run `komorebic static-config-schema` and diff with `komorebi-for-mac/schema.json` (two description-level changes and the per-prefix shape are intended). |
| `kvtvnouo` 93e4d511, `stxmqnop` dec91984 (StaticConfig from komorebi-config) | The config data types and their defaults come from `komorebi-config`; `komorebic static-config-schema` prints the macOS view. | A real `komorebi.json` of yours loads with no new warning and every key still takes effect (borders, padding, animations, scrolling, rules, workspaces); `komorebic static-config-schema` matches the checked-in `schema.json` up to the intended description lines. |
| `qukywpss` c46320ea, `zwkwptox` 1e902542, `sxptsttt` 0345fccb (komorebic shares clap definitions, launcher, pan) | `komorebic start` and `stop` use the shared Launcher skeleton (the odd `--config` arguments are kept; "Waiting for komorebi to start..." is flushed); pan commands use the shared module. | `komorebic start --bar`, `start --await-configuration`, `stop`, `stop --bar`; `komorebic pan-*` commands; `komorebic --help` and a few `<command> --help` texts (about 15 help texts changed to the Windows wording). |
| `oywqqkuy` f0b1a448 `register every Window in ax_registry and forget the dead` | `Window::new` registers the window in the id registry (`ax_registry`); entries are removed in `Window::drop` of a dead window, in `reap_window`, and (`forget_pid`) when an application terminates. No AX call is added; the table holds retained AX references and observers. | Open and close 50 windows (Finder, Terminal, Safari) and quit several apps, then check the daemon's memory in Activity Monitor does not climb and nothing behaves differently (focus, tiling, borders). If a quit app's windows were not forgotten the symptom is memory growth only. Run with `RUST_LOG=debug` and check no new errors at app quit. |
| `oqnvyqvw` e0687167 `keep ax_registry in step when a native tab changes a window's id` | The two places that swap a window's id and element in place for native tabs also update the registry (`sync`). | A native-tabbed app (Terminal, Finder, Safari tabs): switch tabs, close the first tab, open a new tab, focus the window with `komorebic focus ...`, move and resize it. Must behave as on the baseline (no window lost, no stuck border). |
| Bar: `nottqpms` 086833af, `zvuyspku` b8de056b, `uxzpkmnk` 7cd85bd4, `yopuwouu` 6da27dda (bar-core, platform hooks) | macOS now builds `sysinfo` 0.39 and `starship-battery` 0.11 (the Windows versions); cpu, battery, memory, date, update widgets are the shared code. | Bar renders; CPU %, memory, battery % and charging state show sane values; date and the calendar icon; clicking CPU, battery, network, update (see bug list D below: these click handlers are broken on macOS and stay broken). |
| 11(iii) Container: `refactor(mac): the daemon's Container is komorebi_core::Container<Window>` (see `jj log -r wip-u-kfm2`) | `Container` is the shared generic type over the macOS `Window` (its id, windows, focus, `remove_*`, `idx_*` functions are the shared ones). The four functions that need the monitor's hiding corner and can fail (`add_window`, `load_focused_window`, `hide`, `restore`) stay macOS's, renamed `add_window_at`, `load_focused_window_at`, `hide_on`, `restore_focused_window` (`ContainerExt`); `Window::hide/restore/minimize/unminimize` take `&self`. State JSON must not change (pinned by two tests in container.rs). | Run `cargo test` (the container, workspace and scrolling tests use these). Then by hand: stack windows (`komorebic stack`), unstack, cycle the stack (hidden members go to the monitor's corner, the front one is restored); switch workspaces and come back (restore of the front window of each stack); `komorebic state | jq '.monitors.elements[0].workspaces.elements[0].containers.elements[0]'` still has `id`, `locked`, `windows.{elements,focused}` and nothing else. Set `window_hiding_position` per monitor in the config and check each monitor hides into its own corner. |
| 11(iii) Workspace: `refactor(mac): the daemon's Workspace is komorebi_core::Workspace<MacosPlatform>` (see `jj log -r wip-u-kfm2`) | About 50 workspace functions (container and focus operations, placement, resize constraints, window lookups, floating helpers, the stack and monocle operations that hide windows) are now the shared ones; `hide`, `new_container_for_window` and the stack functions pass the monitor's corner through `MacosPlatform::hide_spot` (kept in `MacosWorkspaceState`, set by the monitor with the globals), and `hide` passes the display bounds for the containers through `hide_spot_on_display`. macOS's own `update`, `apply_wallpaper`, `restore`, `remove_window`, `load_static_config`, `prune_duplicate_windows`, `visible_window_details`, `reap_invalid_windows_for_application`, `restore_windows`, `calculate_layouts` and `set_scrolling_columns` are `WorkspaceExt`; the scrolling methods are `WorkspaceScrolling`. | `cargo test` first (the workspace, scrolling and window_manager tests). Then: stack and unstack, switch workspaces both ways (each stack shows its front window, the hidden ones are in their monitor's corner), toggle monocle and back, float and unfloat a window, cycle the monocle container, move a window to another workspace, move a container across monitors; with a different `window_hiding_position` on each monitor check each monitor's windows park in their own corner and that a floating window that overhangs the edge of a display is still parked on its own display. `komorebic state` keys of a workspace are unchanged (pinned by `state_pin`). Scrolling layout: focus, move, consume and expel, pan, resize a column. |
| 11(iii) Monitor: `refactor(mac): the daemon's Monitor is komorebi_core::Monitor<MacosPlatform>` (see `jj log -r wip-u-kfm2`) | The monitor is the shared generic type; the corner (`window_hiding_position`) is the platform's flattened `extra` (written last, as before), the constructors, `update_*_globals`, `load_focused_workspace`, `move_container_to_workspace(1)`, `add_container_with_direction`, `hide_unfocused_workspaces` and `restore_focused_workspace` are `MonitorExt`; `focus_workspace`, `add_container`, `remove_workspace*`, `ensure_workspace_count` and the name and layout lookups are the shared ones. The monitor gives its workspaces the corner through `workspace.state` when it updates their globals. | `cargo test` (monitor tests, `state_pin`). By hand: `komorebic state | jq '.monitors.elements[0] | keys_unsorted'` is the same list as before with `window_hiding_position` last; switch workspaces on each monitor (hide and restore of the whole workspace), focus-workspace by index and by name, move a container to another workspace and to another monitor, unplug and replug a display (the monitor and its workspaces are rebuilt through `Monitor::new` and `update_all_workspace_globals`), a config with `window_hiding_position` per monitor. |
| `wip-u-kfm3` registry fixes (see `jj log -r wip-u-kfm3`) | The registry keeps inert application copies (no blocking Accessibility call when an entry is dropped), a window is registered when its observer is attached, a clone being dropped no longer deregisters its window, `forget_pid` runs ahead of the pause and other-space returns, and the application reaper forgets what it reaps. Nothing reads the registry except the unused `MacosBackend`. | H3, H4, H6, H7 above, and the usual walk-through: no change in what the daemon does is expected. |

Revisions in section B change no behaviour on purpose (rename, move, route); they still need the sections C walk-through, because a mechanical mistake would show there.

## B. Mechanical macOS revisions of the `kfm` agent (Step 6 and 11(i))

| Revision | Change |
|---|---|
| `vzxkxzzy` 796c3832 | `Window::frame()` (renamed `window_rect` below) for the 28 sites that read a Window's frame through `MacosApi::window_rect(&w.element)` (window.rs, window_manager.rs, workspace.rs, process_event.rs, process_command.rs, settle.rs, macos_backend.rs) |
| `oootynxk` 7c2e1bea | `Window::is_valid` renamed `Window::alive` |
| `roxrxtru` 49a6dbda | `Window::set_position` renamed `Window::set_rect` (10 callers) |
| `vmnnuzzu` 803a1410 | `Window::set_messaging_timeout` (settle.rs: 3 raw AX sites) |
| `upylxxuw` ecaa3e15 | `Window::close` (the same close-button press as `AdhocWindow::close`) |
| `tsymmvll` d30a3eb6 | step 11(i): `MacosBackend` calls only `Window` methods (new `Window::minimized`); still unused by the daemon |
| `qsrxvsmv` 931b17d7 | `Window::frame` renamed `Window::window_rect` (the Backend and the Windows Window use that name) |
| `xrtqmxkp` 4c9094d7 | native: `komorebi_backend::Registry` (the rules of the id registry; 9 unit and 1 doc test run on Linux) |
| `tmmtuosy` 386c5a79 | `ax_registry` module; `MacosBackend` resolves ids through it (the adapter is still unused by the daemon) |
| `kvkxkqnx` ba22e8dc | `ax_registry::identify`: the 7 sites that turn a system-handed element into a window id (input_event_listener, process_command, process_event x5, window_manager) |
| `kpopuvoy` 01b3bd5c | live-resize step (window_manager.rs) reads the dragged window's frame through `Window::window_rect` |

Hand check: C below. If something breaks and is not the lock order, bisect among these seven first: they only rename and route, so a regression means a call was routed to the wrong receiver (type-check cannot tell a Window from another window of the same type).

## C. Walk-through (the list the plan names)

Do each with the baseline build and the tip build and compare (or record with `komorebi-recorder`).

1. **Focus**: `komorebic focus left|right|up|down`, `cycle-focus`, `focus-window <n>`; mouse-follows-focus on/off (the cursor centres in the window: uses the window frame query); focus a window of a tabbed app (Terminal, Finder): the focus uses `element_to_focus`, check the right tab.
2. **Move and resize**: `move`, `resize-axis`, `resize-edge` and a manual drag of a window; after a drag the layout puts the window back (settle reads the frame with a timeout); live resize of one edge moves the neighbour. A window that refuses a width (a fixed size app) is read back and the neighbours adapt.
3. **Stack**: `stack`, `unstack`, `cycle-stack`, `stack-all`; hidden stack members are parked off-screen (`hide`) and restored (`restore`) correctly; no window left at a hidden corner after unstack or quit.
4. **Workspaces**: switch (`focus-workspace`), send a window (`send-to-workspace`), `move-to-workspace`, with 2 monitors if possible; windows of the workspace you left are hidden, those of the new one restored to where they were; workspace rules after a monitor change.
5. **Scrolling layout**: the scrolling layout, `pan` commands, `scroll-*`; windows slide in from the edge (the arrival/slide code reads the frame).
6. **Pan** (trackpad gesture) and **live resize**: `komorebic pan-*`, three-finger style gestures as you configured them.
7. **Borders**: focused/unfocused/stack colours follow focus and follow animated windows; borders vanish and come back on workspace switch; no border left behind after closing a window (the window's close and borders share the observer code).
8. **Animations**: movement and workspace animations on and off; the ghost images; cancel an animation by issuing a new command in the middle.
9. **Display change**: plug or unplug a monitor or change the resolution: work areas update, workspaces go to the right monitors, workspace rules re-apply. (This is the thread that can deadlock, see bug list A.)
10. **Sleep and wake**: close the lid or sleep, wake: windows are where they were, the daemon answers, the bar returns.
11. **Minimize, unminimize, close**: `komorebic minimize`, `close` (the foreground window), un-minimize a window from the Dock; closing a window removes it from its container (the reaper uses `Window::alive`).
12. **Reload the config** repeatedly (see section A, first row).

## D. Bugs noticed, not fixed here

A. **Possible deadlock, main queue against the window-manager lock** (`docs/dev/seam.md` section 3.3, by reading only, never reproduced). A thread holding the WM lock calls a blocking main-queue dispatch (`destroy_all_borders` at process_event.rs lines about 128/139, process_command.rs, `retile_all`; `hide_borders_on` in window_manager.rs; `activate_finder` in process_event.rs) while the monitor reconciliator has dispatched a closure to main that itself takes the WM lock (monitor_reconciliator.rs about 106 and 383). Then the event thread waits for main, main waits for the lock. `parking_lot`'s deadlock detector cannot see it (the dispatch is a semaphore). Window of exposure: a display change at the instant of one of those events. Structural fix: no closure that runs on main may take the WM lock (or dispatch to main only after the lock is dropped). How to provoke it: spam `komorebic retile` / workspace switches (with ghosts) while plugging or unplugging a monitor; the symptom is a daemon that answers no command (`komorebic state` hangs).
B. **cmd.exe and explorer.exe in the macOS bar** (`komorebi-for-mac/komorebi-bar/src/os.rs`, `BarPlatform::open_system_tool`): the battery click spawns `cmd.exe` (power settings), the update click spawns `explorer.exe` with the release URL; neither exists on macOS. The same file's CPU (task manager) and network settings tools should be Activity Monitor and System Settings. The fix is one function in that file; it was preserved on purpose during the bar move.
C. **Update widget refresh thread polls the Windows repo's GitHub API** on macOS too (update.rs has a second URL that does not go through the hook), and the macOS update link is `.../releases/v..` built for the Windows repo.
D. **Other raw AX sites remain outside `Window`**: window.rs (38 inside `Window`/`AdhocWindow`, expected), application.rs 11 (`Application`, not a window), process_event.rs 7 (events that hand over a bare element: `window_id` of an element, the tab check `AccessibilityApi::window_id(&window.element.0)`), animation/animator.rs 6 (lane threads hold `WindowHandle` clones of the element), border_manager 7 (observers on tracked elements), window_manager.rs 4 (timeouts list holds elements, `window_id` of an element), reaper.rs 2 (`notify_if_gone` takes an element so lanes can use it), input_event_listener.rs 1, process_command.rs 1. They need either an id to AX registry (step 11(ii), not started) or a `WindowHandle` type; they were left alone because routing them changes who owns the element.
D2. **Step 11(ii) status.** Done: the id registry (`ax_registry`, over the tested `komorebi_backend::Registry`), populated at `Window::new`, kept in step for native tabs, emptied on death, reap and application quit; `MacosBackend` resolves ids through it. NOT done, on purpose, because each needs a decision that cannot be verified without a Mac: (a) the animator lanes (`WindowHandle`) keep their own snapshot of the element: a lane must keep writing to the element of the move it was given even if a tab swap replaced the registry entry, and replacing the snapshot by a lookup per write changes what a late write hits; (b) borders keep `tracking_element` (observer sources are added and removed on that exact element, on main); (c) `reaper::notify_if_gone` and the window manager's live-resize `timeouts` list take elements, including application elements, that are not registry entries; (d) `application.rs` is the application, not a window. (e) The model (`Window`, containers, workspaces) still holds `Window` values, not bare `WindowId`s: that is step 11(iii), after steps 8-9. A stale registry id answers `Error::WindowGone` (no panic); a window that was deserialised from state (default system-wide element) is never registered.
E. `MacosBackend` (step 7c/11(i)) needs 11(ii) before it can be used: `windows()` answers only what was `register`ed, `hide` needs the corner, `raise` also focuses, `monitors()` reads a cache nobody fills yet (seam.md section 9.2). `AdhocWindow` (bare foreground element) still has its own minimize, close, hide, restore, raise.
F. In `window.rs` `AdhocWindow::hide` / `Window::hide` duplicate the restore-position bookkeeping (`WINDOW_RESTORE_POSITIONS`), one on a `Window` and one on a bare element; they must stay in step.

## E. What is not checked at all

Everything above. In particular: no macOS test (399+ unit tests incl. those in window.rs for drag state) has run against any of these revisions; the unified binaries do not exist yet; no real Accessibility call was made; `komorebi-recorder` captures need a Mac.
