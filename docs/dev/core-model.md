# The core model: KDE and Windows compared, and the split into `komorebi-core`

Status: measurement and proposal (plan step 9 preparation, KDE side). Measured at the tip of
`wip-u-kfde` (based on `wip-main` 633e7453); the Windows tree is `komorebi/src`, KDE is
`komorebi-for-kde/komorebi/src`, macOS (`komorebi-for-mac/komorebi/src`) is listed only where it
helps. Nothing here has been run on Windows or macOS.

## 1. Measured differences

"Code lines" drop `//` lines and blank lines and strip indentation, so KDE's much heavier doc
comments do not count; the inline `#[cfg(test)]` modules are included. "Changed" is the number of
`<`/`>` lines of `diff` between the two stripped files (an insertion of n lines counts n, a
replacement of n lines counts 2n). "Identical" is the number of stripped lines present in both in
the same order (a common subsequence, so it is a lower bound on shared text).

| file | code lines Windows / KDE / macOS | changed W vs K | changed K vs M | changed W vs M | identical W&K |
|---|---|---|---|---|---|
| `container.rs` | 230 / 254 / 241 | 196 | 119 | 185 | 144 |
| `monitor.rs` | 643 / 666 / 675 | 613 | 205 | 616 | 348 |
| `workspace.rs` | 2203 / 2491 / 2196 | 2314 | 1097 | 2113 | 1190 |
| `window.rs` | 1134 / 916 / 1641 | 1168 | 1413 | 1767 | 441 |
| `window_manager.rs` | 4716 / 4023 / 4896 | 4543 | 1597 | 5434 | 2098 |

Whole-file raw `diff` sizes (comments included): container 271, monitor 752, workspace 3167,
window 1520, window_manager 6358 changed lines (Windows vs KDE). `ring.rs` is already shared
(`crates/komorebi-collections`, `Ring`, `Lockable`).

Two things stand out. First, every file KDE and macOS share is much closer to each other than
either is to Windows (K vs M changed is a third to a half of W vs K for monitor, workspace,
window_manager, static_config, state). Windows is the odd one out, mostly because it carries older
and larger machinery (see section 2). Second, the structs themselves are almost the same:
`Workspace` has 31 fields on both sides and they differ only in the order of
`work_area_offset_rules`/`work_area_offset`; the differences are in the methods around them.

## 2. Platform-specific fields and methods, per layer

### container (smallest)

- Same fields (`id`, `locked`, `windows: Ring<Window>`). Windows' `windows` is private and
  `locked` has `#[serde(default)]`; KDE's `windows` is `pub`.
- Window id type: `hwnd: isize` (Windows) vs `id: u32` (KDE) in `contains_window`, `idx_for_window`,
  `hide(omit)`.
- Windows only: `hwnd_from_exe` (KDE keeps only `idx_from_exe`), `Window::restore_with_border` /
  `hide_with_border` in `load_focused_window` (Windows' border and stackbar are separate windows).
  `exe()` returns `Result` on Windows, `Option` on KDE.
- After the converge revisions of this stack the signatures are the same (`add_window(Window)`,
  `hide(&self, Option<id>)`, `restore(&self)`, `load_focused_window(&mut self)`, no `Result`) and the
  item order is the same. What is left is the id type and the three Windows-only lines above.

### monitor

- Windows only fields: `name`, `device_id`, `workspace_names: HashMap<usize, String>`;
  `serial_number_id` is `Option<String>` (KDE: `String`); `id` is `isize` (KDE `u32`).
- Windows only methods: none in the public API; KDE has `assign_windows_to_workspaces` (it pushes the
  workspace of each window to the compositor's desktops, `Backend::set_window_workspace`).
- Behaviour that differs: Windows `load_focused_workspace` restores/hides workspaces itself, KDE asks
  the compositor to switch desktops (`Backend::switch_workspace`) and tiles what is in front; the
  Windows `update_workspaces_globals` pushes padding/border/work area into every workspace
  (`WorkspaceGlobals`), KDE calls `update_workspace_globals(idx, offset)` for the focused one. KDE
  has a `MonitorInformation` with 4 fields, Windows 6.
- `BORDER_ENABLED` (Windows) vs `BORDER_DRAWN` (KDE): the compositor draws borders.

### workspace

- Windows only methods: `windows_on_show`, `border_kinds_on_show`, `hwnd_from_exe`.
- KDE only methods: `adopt_maximized_window`, `focus_floating_window`,
  `focus_previous_floating_window`, `prune_duplicate_windows`, `reap_invalid_windows`,
  `remove_floating_window` (my reading, not verified: KDE learns of closes and moves through the bridge snapshot, so the
  workspace reaps and prunes).
- 42 of 45/48 `pub fn` names are common. Windows `restore`/`hide` take the monitor handle and
  wallpaper and do the showing and hiding (`WindowsApi`, 8 mentions in the file); KDE leaves showing
  to the compositor (5 `backend()` calls).
- Scrolling layout glue (`scrolling.rs`): 1328 lines KDE, 1315 macOS, ~2200 Windows; measured first,
  not part of this comparison.

### window (not shared: the platform handle)

- `Window { hwnd: isize }` (Windows, methods that call `WindowsApi`, 1134 code lines) vs
  `Window { id: u32 }` (KDE, `Copy`, every method calls `backend()`, 916 code lines) vs macOS
  (AX element, application, observer; not `Copy`, 1641 code lines).
- Both Windows and KDE `Window` are `Copy` handles, which is what lets the container and workspace
  code hold windows by value. macOS is not (clone).
- `Window` is where the split goes: the core must not call a method of `Window` that does not exist
  on all three.

### window_manager

- Windows only fields: `known_hwnds`, `virtual_desktop_id`, `uncloack_to_ignore`,
  `has_pending_raise_op`, `pending_move_op` with a handle and rect, `already_moved_window_handles`
  of `isize`.
- KDE only fields: `opacity_applied`, `minimized_windows`, `pending_resize_op`, `live_resize`,
  `known_window_ids` (a `u32` map); `pending_move_op` has no rect.
- 43 `WindowsApi` mentions in Windows' `window_manager.rs`, 15 `backend()` in KDE's. Every one of them
  becomes a `Backend` call or a `Window` method in the core.
- `process_command` and `process_event` are not in this table (see section 4 and the plan).

## 3. Proposed split: `komorebi-core`

```
komorebi-core                 (new crate, no platform dependency)
  ring, container, monitor, workspace, window_manager      (generic)
  trait Platform: Backend + { type Window; hooks }         (what is not in Backend)
komorebi-backend              (exists: Backend, WindowId, MonitorId, WindowInfo, Error, Capability)
komorebi (windows) / (kde) / (mac)
  Window (the platform handle), the Backend implementation, event sources, borders, ...
```

Generic over what:

- `WindowManager<P: Platform>`, with `Platform::Window: Copy + Eq + Hash + Serialize + Deserialize` and
  `fn id(&self) -> WindowId`. The three id types (`isize`, `u32`, `u32`) disappear into `WindowId`
  (`WindowId::from_hwnd`/`from_u32`, already in `komorebi-backend`). macOS needs `Clone` instead of
  `Copy`: the generic code has to take `&P::Window` and clone, or macOS keeps a `Copy` handle and an
  id registry as the seam doc proposes (docs/dev/seam.md section 7, consequence 1).
- `Backend` is a trait object or a type parameter; `docs/dev/seam.md` and the KDE conversion show it
  can be the shared trait unchanged, plus KDE's extension (`drag_*`, look, wallpaper) behind
  `Platform` hooks.
- Methods that are still platform behaviour (`Window::hide_with_border`, showing and hiding a
  workspace on Windows vs asking for a desktop on KDE) move behind two hooks:
  `Platform::show_workspace(monitor, idx)` and `Platform::hide_workspace(...)`, with the Windows
  implementation hiding/restoring windows and the KDE one calling `Backend::switch_workspace`.

### Exact order of layers (bottom-up, as in the plan)

1. **ring** is done (`komorebi-collections`).
2. **container** (230 lines): needs `Window` generic and `id()`. Smallest; do first. Windows must
   rename `hwnd` parameters to `WindowId` or an `id()` accessor and give `exe()` the KDE return type
   (`Option<String>`) or both use `Result`.
3. **monitor** (643): needs `Workspace` generic, `MonitorId`, and the `Platform` show/hide hooks.
   Windows' extra fields (`name`, `device_id`, `workspace_names`) stay in the shared struct as
   `Option`/default fields or become a `P::MonitorExtra` associated type; KDE `serial_number_id` becomes
   `Option<String>` with `""` mapped to `None`.
4. **workspace** (2203): by far the biggest; the platform split is `restore/hide/windows_on_show/
   border_kinds_on_show` (Windows) and `reap/prune/adopt_maximized` (KDE). Scrolling glue is measured
   before it moves.
5. **window_manager** (4716): after the first three are shared. The platform fields (section 2) go
   in a `P::State` associated type.
6. the flat router and `process_event` (separate steps in the plan).

## 4. What KDE has done (this stack) and what Windows must do before the crate exists

Landed on `wip-u-kfde` (all native tests green, none of it run on a VM):

- KDE's `Backend` extends the shared trait; ids, errors and info types are the shared ones at the
  trait; the core still holds `Window { id: u32 }` and converts at the call.
- `container.rs` and `monitor.rs` are in the same order as Windows', and `Container` has the Windows
  signatures (`add_window(Window)`, infallible `hide`, `restore`, `load_focused_window`).
- All `process_command` arms longer than 20 lines are named methods (list in section 5).

I did not create the shared crate: the Windows tree still has `isize` handles in `Container`,
`Monitor::id: isize`, and calls `WindowsApi` directly in 43 places of `window_manager.rs`.
Windows (kfw) has to:

1. Finish adopting `Backend` in `window_manager.rs`, `workspace.rs`, `monitor.rs` (the 8 + 43 + 2
   `WindowsApi` call sites there), so no core file names `WindowsApi`.
2. Give `Container` the same API KDE now has: accept a `WindowId` (or `Window::id()`), return `Option`
   from `exe()`, and drop `hwnd_from_exe` or move it to a platform extension.
3. Make `Monitor.id` and `serial_number_id` the shared types (`MonitorId`, `Option<String>`), which
   changes what `komorebic state` prints for Windows (a BEHAVIOUR change for the JSON; a test must pin
   it).
4. Convert its own multi-line `process_command` arms (the plan counts 22-23 per platform) into
   named methods with the same names KDE used (section 5), so the router can later be shared.

## 5. `process_command`: arms over 20 lines (KDE, before conversion)

| arm | lines | now |
|---|---|---|
| ToggleWorkspaceLayer | 90 | `toggle_workspace_layer()` |
| EagerFocus | 75 | `eager_focus(exe)` |
| ResizeWindowAxis | 60 | `resize_window_axis(axis, sizing)` |
| IgnoreRule | 55 | `add_ignore_rule(identifier, id)` |
| Query | 51 | `answer_query(query, reply)` |
| SessionFloatRule | 39 | `add_session_float_rule()` |
| ReplaceConfiguration | 34 | `replace_configuration(config)` |
| CycleFocusEmptyWorkspace | 32 | `cycle_focus_empty_workspace(direction)` |
| CloseWorkspace | 29 | `close_workspace()` |
| MoveContainerToLastWorkspace | 28 | `move_container_to_last_workspace(follow)` |
| SendContainerToLastWorkspace | 27 | same method, `follow = false` |
| CycleFocusWorkspace | 26 | `cycle_focus_workspace(direction)` |
| LayoutRatios | 23 | `set_layout_ratios(columns, rows)` |
| BorderColour | 21 | `set_border_colour(kind, r, g, b)` |
| NamedWorkspaceRule | 21 | `add_named_workspace_rule(identifier, id, workspace)` |
| InitialNamedWorkspaceRule | 21 | `add_initial_named_workspace_rule(identifier, id, workspace)` |
| ToggleWorkspaceWindowContainerBehaviour | 21 | `toggle_workspace_window_container_behaviour()` |

All are `pub fn name(&mut self, ...) -> eyre::Result<()>` in `impl WindowManager`
(`process_command.rs`), except `set_border_colour`, which cannot fail.

## 6. First layer moved: `Container` is in `komorebi-core` (wip-u-core)

Done, on a throwaway merge of `wip-u-kfde` and `wip-u-kfw` (bookmark `wip-u-core`):

- `crates/komorebi-core` (root workspace member; KDE takes it by path) holds `Container<W>`, moved
  from the KDE file and made generic, and the trait `ContainerWindow` that says what a container
  asks of a platform window: `type Id` (`isize` on Windows, `u32` on KDE), `id`, `exe_name`,
  `hide_window`, `restore_window`, and the pair `hide_stacked` / `restore_stacked` that
  `load_focused_window` uses (on Windows they leave the border alone, on KDE they are the plain
  ones). The container makes no `Backend` call itself.
- KDE and Windows each have `type Container = komorebi_core::Container<Window>` plus the
  `ContainerWindow` impl for their `Window`. Windows keeps `hwnd_from_exe` as the extension trait
  `ContainerExt` (workspace.rs imports it). The macOS tree does not use the crate yet and still
  type-checks.
- The JSON of a container is pinned by a test in each tree and in the crate (`id`, `locked`,
  `windows: {elements, focused}`); `locked` may be missing when read (KDE gained that from Windows);
  the schemars schema of `Container<W>` is renamed to `Container`, with a test.
- The Windows converge is marked BEHAVIOUR because it was only type-checked (clippy) and the
  crate's tests run natively; the Windows e2e scenarios it needs are in that revision's body.

### Where it stops: `Monitor`

After kfw's convergence the Windows and KDE `monitor.rs` still differ by 657 changed code lines
(710 vs 675 lines), and the difference is not mechanical:

- **Fields**: Windows has `name`, `device_id`, `workspace_names: HashMap<usize, String>`;
  `serial_number_id` is `Option<String>` (KDE `String`); `id` is a `MonitorId` that the Windows file
  serialises through `hmonitor_json` as an `i64` (KDE: a plain `u32`). Making them the same changes
  what one daemon's `komorebic state` prints, so it is a decision, not a refactor: either a
  `Monitor<P>` with an associated `MonitorExtra` (Windows: the three fields) and `MonitorId` with a
  serde adapter per platform, or the extra fields as `Option`s that KDE leaves `None` and skips when
  serialising.
- **Methods**: `MonitorInformation` has 6 fields on Windows and 4 on KDE; Windows has the two serde
  helpers, KDE has `assign_windows_to_workspaces`. The 16 other public methods are common, but
  `load_focused_workspace` (Windows hides and restores the workspaces itself, KDE asks the compositor
  for the desktop) and `update_workspaces_globals` / `update_workspace_globals` (Windows pushes the
  globals into every workspace, KDE into the focused one) differ in behaviour, and need the
  `Platform::show_workspace` / `hide_workspace` hooks of section 3.
- `BORDER_ENABLED` (Windows) vs `BORDER_DRAWN` (KDE) are separate statics.

### What remains, in order

1. `Monitor`: decide the field-set question above, add the two workspace hooks to a `Platform`
   trait (the first thing in the crate that is a `Platform` rather than a window), converge the
   method bodies, then move. The diff after that decision should be a few dozen lines.
2. `Workspace`: 2314 changed code lines. Needs, besides the hooks, the Windows-only
   `windows_on_show` / `border_kinds_on_show` and the KDE-only `reap_invalid_windows`,
   `prune_duplicate_windows`, `adopt_maximized_window`, `focus_floating_window`,
   `focus_previous_floating_window`, `remove_floating_window` as optional `Platform` hooks or
   extension traits; the scrolling glue is measured before it moves.
3. `WindowManager`, then the router and `process_event` (unchanged from section 3).
