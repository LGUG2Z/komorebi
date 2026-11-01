# From e2e to unit: what the shared crates made testable natively

> **Paths (cutover stack):** this document was written before the directories were renamed and keeps the paths of its time. Now: `komorebi/` is `komorebi-windows/`, `komorebi-for-kde/komorebi/` is `komorebi-linux/`, `komorebi-for-mac/komorebi/` is `komorebi-macos/`, `komorebi-for-kde/komorebi-hotkeys/` is `komorebi-hotkeys/`, `komorebi-for-kde/nix/` and `komorebi-for-mac/nix/` are `nix/linux/` and `nix/macos/`, `komorebi-for-kde/docs/` and `komorebi-for-mac/docs/` are `docs/linux/` and `docs/macos/`, `schema*.json` are in `schemas/{windows,linux,macos}/`, and every package is at version 0.2.0 (the 0.1.x numbers below are history).

Base: `wip-cand5` plus the revisions that add the tests named here. Written 2026-10-31.

Before the shared crates, the Windows daemon could only be tested through a Windows guest (its crate
does not compile on Linux) and the macOS daemon through nothing. Now Windows, KDE and macOS call the
same `komorebi-core` (`Container`, `Workspace`, `Monitor`, `update_shared` and the platform hooks),
`komorebi-layouts`, `komorebi-scrolling`, `komorebi-settings`, `komorebi-protocol`,
`komorebi-config`, `komorebi-transport` and `komorebic-core`, so a native test of that code, on the
stub platform (`crates/komorebi-core/tests/common`) or `FakeBackend`, protects all three. This
document lists every e2e scenario, says whether that is now done, and ranks what is left.

## What stays out of reach

The daemons' own `process_command` / `process_event` are per platform (Windows: Win32 types; macOS:
Mac types), only the KDE one runs on `FakeBackend`. So a native test of "send this command, then the
state is that" exists for KDE only; for Windows and macOS it holds as far as the command's work is
done by shared `Workspace`/`Monitor` methods. Rule matching (`should_act_individual`) exists three
times (Windows `komorebi/src/window.rs:1178`, KDE `window.rs:661`, macOS `window.rs:1864`), not in
`komorebi-config`. Every row below says which of these a test reaches.

## Classes

- A: already covered natively. B: coverable natively with the shared API, not yet. C: needs a real OS
  (compositor, Win32, timing, pointer, rendering, several processes).
- "Now" marks what this work added (revisions on `wip-u-e2e2unit`).
- Value: share of the gate it could stand in for. Effort S/M/L.
- Check counts: dynamic numbers where the gate printed them (layouts 111, commands 265), else the
  number of check call sites in the scenario file (static, lower than the runs).

## Windows-origin scenarios (first: only e2e-testable on Windows, now native)

| Scenario (checks) | Behaviour | Class | Native test (cited, or proposed) | Eff. | Value |
|---|---|---|---|---|---|
| layouts (111) | 8 layouts x counts: each window where the layout says, inside the work area, not over another | A now (arithmetic); C (OS frame equals tile) | `crates/komorebi-core/tests/e2e_layouts.rs` `every_layout_tiles_every_container_inside_the_work_area_and_off_the_others` (shared `update_shared`), `komorebi-layouts/tests/layouts_everywhere.rs` (calculate, 3 areas, 1-12 windows) | S | high: 3 min, can shrink to 1 layout smoke |
| flipping (3) | flip-layout mirrors every place, flip back restores | A now | `e2e_layouts.rs` `a_flip_mirrors_the_tiles_of_the_workspace_as_a_set`; `layouts_everywhere.rs` flips | S | med |
| layout_flip (10) | `layout_flip` config mirrors from the start; flip-layout toggles; per workspace | A now for the maths; B for config key to `Workspace.layout_flip` | same tests; proposed: `komorebi-settings` or core test that loads a workspace config with `layout_flip` and checks the field | S | med |
| padding (7) | workspace/container padding moves and shrinks tiles, setting back restores | A now | `e2e_layouts.rs` `the_workspace_padding_moves_the_tiles_in...`; existing `update_shared.rs` `the_padding_of_the_workspace_wins_over_the_monitors_even_when_it_is_zero` | S | med |
| tiling (6), closing (5), minimizing (4), floating (7) | windows share the area; a leaving container (closed/minimized/floated) gives the others room | A now (room) + A (floating/removal): `workspace_layers.rs` floating_* , `workspace_containers.rs` removing_*; C: the OS event | `e2e_layouts.rs` `removing_a_container_gives_the_others_more_room` | S | med |
| stacks (23) | stack/cycle-stack/unstack: one tile per stack, window in view has focus, others away | A: core `workspace_containers.rs` `stacking_*`, `unstacking_*`, `container_behaviour.rs` hide/restore; now `e2e_layouts.rs` `a_stack_is_one_tile`. B: `cycle-stack` has no shared method (daemon-side) | S | med (23 checks, 3 min) |
| tiling_off (6) | toggle-tiling leaves windows, tiling again tiles | A now | `e2e_layouts.rs` `a_workspace_with_tiling_off_places_nothing...`; `update_shared.rs` `a_workspace_that_is_not_tiling_prunes_and_places_nothing` | S | low |
| monocle (12) | monocle fills area, others away, toggle back; focus behaviours | A: `workspace_layers.rs` `toggling_monocle_*`, `reintegrating_*`, `cycling_monocle_*`; `update_shared.rs` `a_monocle_container_gets_the_work_area_less_every_padding`. Focus behaviour is daemon-side (B, needs seam) | - | - | med |
| maximizing (29) | toggle-maximize, unmaximize on float, monocle wins | A: `workspace_layers.rs` `floating_the_maximized_window_*`, `update_shared.rs` `the_monocle_container_wins_over_a_maximized_window`; user-maximize event C | - | - | med |
| promoting (4), moving (4), focus (7) | promote first tile, swap/cycle-move, focus order | A: `workspace_containers.rs` `promoting_*`, `swapping_containers_*`, `cycling_wraps_*`, `directions_follow_the_layout_and_the_flip`; focus OS agreement C | - | - | low |
| workspaces (8), workspace_moves (8) | send/move/focus workspace, last-workspace | A: `monitor_workspaces.rs` (focus, last focused, ensure, remove); the OS hide/show is C | - | - | med |
| commands (265, 7.6 min) | every komorebic command leaves the daemon alive, answers state, manages every window | A now for command line (`crates/komorebic-core/tests/e2e_sweep.rs`: every SWEEP/PRINTED line is a Windows command; macOS lacks exactly 8) and for the KDE model (`komorebi-linux/tests/command_sweep.rs`, about 185 messages, 0.2 s); B: same sweep for the Windows and macOS daemons (needs seam, below); C: stall-family (daemon busy 8 s), socket answering | S done / L for Win+mac | highest: biggest, flakiest scenario |
| unknown_monitor (12) | a window sent to a monitor that does not exist stays managed | B: shared `Monitor`/`Workspace::add_container` errors are covered (`monitor_workspaces.rs` `adding_a_container_to_a_workspace_that_is_not_there_is_an_error`); the daemon handler is per platform: needs seam | M | med (known flaky marker) |
| rules (12), rules_class (5), rules_exe (5), manage_rules (4), title_rule (5), startup_rules (5) | which window a rule catches (kind, strategy, composite, last wins, manage over ignore); the workspace it goes to | A now for matching, once for all three daemons: `crates/komorebi-config/tests/rule_matching.rs` (16 tests, KDE/macOS and Windows input shapes; the matching moved to `komorebi_config::rules`, see `docs/dev/rule-matching.md`); KDE keeps the manage-rule rescue test. Workspace of a rule: A in core: `workspace_containers.rs` `initial_placement_rules_choose_where_a_new_window_goes`. When-evaluated (open vs rename), hide/show: C | M to move the function into `komorebi-config` and test once for three daemons (recommended seam) | high |
| state_fidelity (5) | state names title/exe/class/path as opened | A: `state_roundtrip.rs` (kfw/kfde fixtures, `the_fixtures_are_the_ones_the_e2e_library_has`, `a_windows_tiled_state_is_written_as_it_was_read`); what the OS reports C | - | - | low |
| queries (3) | `komorebic query X` answers from the state, resize save/load round trip | B: state answers per query are `StateQuery` handling in the daemon; resize round trip needs `process_command` (KDE: `resize.rs` `sizes_saved_to_a_file_are_loaded_back_from_it` A) | M | low |
| config_reload (8) | file change reloads names, layout, rules | A for the merge/reset semantics: `crates/komorebi-settings/tests/reload.rs` (`a_reload_merges_what_the_file_gives_and_resets_what_it_resets`), `src/handle.rs`; C: the file watch and the reload message dropped while the lock is held (the NEW failing check in gate5 is that mechanism) | - | - | med |
| placement (1 x 5 cases) | initial_window_placement_rules first/last/Nth | A: `workspace_containers.rs` `a_one_based_placement_index_is_zero_based_and_out_of_range_falls_back_after_the_focused`, `initial_placement_rules_choose_where_a_new_window_goes`. The known Windows failure (number 2) is a daemon event-path matter | - | - | low |
| scrolling (46) | strip, edges, columns, two monitors | A: `komorebi-scrolling/tests` (rules 80, tiles 40, heights 20, pan 16) and `update_shared.rs` `a_scrolling_workspace_*`; the resting window at an edge on the OS is C | - | - | med |
| resize_commands (7), resize (26) | resize-axis widens and gives back; edge drag | A: `resize_constraints.rs`, `update_shared.rs` `the_resize_constraints_are_enforced_before_the_layout_is_calculated`; B (S): `DefaultLayout::resize` increase then decrease is the identity (no test in `komorebi-layouts`); the pointer drag and client rounding C | S | med |
| pausing (4) | paused daemon ignores new windows | A: `komorebi-transport/src/pause.rs` (the message filter); the event filter is per daemon (B needs seam) | - | - | low |
| hotplug (11), monitors (13) | unplug/plug a display, windows move, send-to-monitor | A for the model in `monitor_workspaces.rs`; the reconcile is per platform (Windows `monitor_reconciliator`, macOS own): B needs seam; the display itself C | L | med |
| lifecycle (7), lifecycle_hidden (13), locking (7), handover (6), fullscreen (6), display_change (4), focus_follows_mouse (7) | stop/start restores windows, screen lock, minimize/unmanage/manage, fullscreen, mode change, pointer focus | A for hide/restore: `container_behaviour.rs` `restoring_*`, `workspace_layers.rs` `hiding_a_workspace_*`; the process, session, OS events and pointer are C | - | - | - |
| Windows extras: bar (21), windows_padding, cloaking, config_file, tracking, second_display, display_change | bar process and window, 7 px border, cloaking, file watch, system focus setting, QXL display | bar schema: A (`crates/komorebi-bar-core/tests/schema_oracle.rs`); rest C | - | - | - |

## macOS-origin behaviour (never e2e-tested): what the shared tests now give

macOS has no e2e. The shared-crate tests that exist protect its code path as far as the hooks do:
`state_roundtrip.rs` `a_macos_shaped_state_is_written_as_it_was_read_with_the_corner_last`,
`a_macos_state_cannot_be_read_without_its_corner`, the `StubWindow` that is `Clone` but not `Copy`,
and now `e2e_layouts.rs` (the tiles every macOS tiling goes through). Proposed B tests, platform
dependent and meant for the Mini: the hide-spot corner per monitor (`hide_spot` hook is
`StubState { spot }`) with `hide`/`restore` on a stack, and `komorebic-core` macOS command set
(`e2e_sweep.rs` pins which sweep commands macOS lacks).

## KDE VM suites (33 suites with a test.py)

All of them need a headless or Plasma KWin: C. The model parts of them are already native through
the KDE tests (`komorebi-linux/tests`, formerly `komorebi-for-kde/komorebi/tests`, about 880): `core-e2e` and `hotplug`, `placement`,
`workspace-rules`, `scrolling`, `stacks`, `user-maximize`, `commands`, `core-lock`,
`focus-follows-mouse`, `wallpaper` have a native model twin; `kwin*`, `check`, `module`, `keys-*`,
`look`, `features`, `rdp`, `render-smoke`, `latency`, `client-geometry`, `drag-resize`,
`live-resize`, `lock-sleep`, `run-kde`, `restart-survival`, `kwin-restart` are the compositor, the
session, keys, pixels or timing (C). The `commands` suite's sweep now has a native twin
(`command_sweep.rs`), so the suite can run a shorter sweep.

## Counts

Scenario rows: Windows/shared 43 scenarios + 7 Windows extras = 50. Classified by what is left to do
after this work: A (fully native for the shared logic, e2e only needed as a smoke) 22; A now (added by
this work) 9 of those (layouts, flipping, layout_flip, padding, tiling, closing, minimizing, floating,
tiling_off, stacks in part); B open 7 (commands for Windows/macOS daemons, unknown_monitor, queries,
hotplug reconcile, pausing event path, rules for Windows/macOS copies, resize identity); C only 21
(lifecycle*, locking, display*, bar, extras, pointer, fullscreen, wallpaper rendering and the KDE
suites). KDE VM: 0 A fully, 33 C (about 10 with a native model twin).

## Time saved

The Windows gate is about 2.3 min per scenario for 35 scenarios (about 80 min), the KDE suites 15
x 1-13 min. Honest estimate:

- Today (what this work added): the arithmetic of layouts, flipping, layout_flip, padding, tiling
  and the room checks is native. A guest still has to show the daemon hands the tiles to the OS, so
  these scenarios shrink to one smoke each, or are merged into `tiling`: layouts (111 checks, about
  3 min) to one layout and 3 windows saves about 2 min; flipping, layout_flip, padding,
  tiling_off, minimizing, closing, floating merged in `tiling` save about 7 x 2 = 14 min. About 16
  min of 80 (20%) on Windows, plus commands: the 7.6 min scenario keeps 20 commands as a smoke and
  loses about 5 min once the Windows daemon is reachable by the sweep.
- If the B items existed (daemon model reachable natively for Windows and macOS, rule matching in
  one shared function, config key tests): another 12 scenarios (rules, rules_class, rules_exe,
  manage_rules, title_rule, startup_rules, state_fidelity, queries, config_reload, placement,
  workspaces, workspace_moves) become one smoke each: about 25 min. Total about 45 min of the 80 min
  gate (55%), and almost all the flaky checks (stall-family markers sit in commands, config_reload,
  resize, maximizing, floating, which are the ones with timing).
- KDE VM: the model twins let `commands`, `placement`, `workspace-rules`, `hotplug`, `scrolling`
  run with fewer cases; this is about 20-30 VM minutes of the 15 suites, not measured.

Not saved: boot and golden image (3 s from a state), the C scenarios.

## Seams that are missing (report only, nothing added)

1. The daemon's `process_command` and `process_event` are per platform. A model-level
   `CommandHandler<P>` in `komorebi-core` (taking `SocketMessage`, mutating `Monitor`/`Workspace`,
   calling hooks) would let one sweep (`command_sweep.rs`) protect Windows and macOS: `commands`,
   `unknown_monitor`, `queries`, `pausing`, `handover`, `moving`, `focus`, `monocle` focus behaviour.
2. `should_act` / `should_act_individual` three times: move into `komorebi-config` (it already has
   `MatchingRule`, `MatchingStrategy`): one test file would replace the rule matching logic of rules,
   rules_class, rules_exe, manage_rules, title_rule, startup_rules for all daemons.
3. Monitor reconciliation (Windows `monitor_reconciliator`, macOS own, KDE `reconcile.rs`) is not in
   `komorebi-core`: a pure `reconcile(old: &[MonitorSnapshot], new: &[MonitorSnapshot]) -> Plan`
   would make `hotplug` and `unknown_monitor` native.
4. `cycle-stack`, `cycle-stack-index`, `focus-stack-window` are daemon methods, not `Container`
   methods; moving them into `komorebi-core::Container` would make the stacks scenario native
   beyond the existing stack/unstack tests.
5. Event filtering while paused (`allowed_while_paused` is shared, the event path is not).

## Tests added, with the e2e check each mirrors

| Test file | Tests | Mirrors |
|---|---|---|
| `komorebi-layouts/tests/layouts_everywhere.rs` | 8 | layouts, flipping, layout_flip, padding, tiling, closing |
| `crates/komorebi-core/tests/e2e_layouts.rs` | 6 | layouts, flipping, layout_flip, padding, closing, minimizing, floating, stacks, tiling, tiling_off |
| `komorebi-linux/tests/rule_matching.rs` | 13 | rules, rules_class, rules_exe, manage_rules, title_rule |
| `komorebi-linux/tests/command_sweep.rs` | 1 (185 messages) | commands |
| `crates/komorebic-core/tests/e2e_sweep.rs` | 3 | commands |

Each test says what stays e2e: the OS frame equals the tile, borders, event timing, the daemon's own
hooks.
