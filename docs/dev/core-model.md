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

## 7. Classification of the `workspace.rs` and `monitor.rs` differences

Method: each file is cut into functions (impl-level and free `fn`s, tests apart), comments and blank
lines are dropped, and each function is compared with its namesake on the other side (`diff`; a
changed line counts on both sides). The earlier whole-file figures (workspace 2314, monitor 657)
come from one `diff` of the stripped files, which misaligns after every insertion; per function the
picture is much smaller. Classes: **(a)** cosmetic or ordering drift, **(b)** same behaviour written
differently, **(c)** a real platform difference (a hook is needed), **(d)** a feature one side lacks.
Measured at the tip of `wip-u-core` (Container already moved; the Windows tree is kfw's `wip-u-kfw`).

### `workspace.rs`

| | functions | changed lines |
|---|---|---|
| identical on both sides (non-test) | 36 | 0 |
| (a) drift | 1 | 31 |
| (b) same behaviour, different text | 15 | 196 |
| (c) platform difference | 6 | 356 |
| (d) feature on one side only | 14 | 270 |
| tests present on both sides | 26 | 71 (all (b): the id rename) |
| tests only in KDE | 20 | 275 |

(c) is `update` (161: Windows adds title bars, stackbars, the animation batch, the scrolling
`adopt_actual_widths` pass and `live_resize::is_stepping`; KDE clamps layouts to the work area, has the
percentage override, `scrolling_placed` and `has_settled`), `apply_wallpaper` (110: Windows writes
the wallpaper through `WindowsApi` and builds the palette theme inline, KDE asks the backend and
calls `theme_manager::wallpaper_palette`), `remove_window` (41: Windows deletes the border and
returns `()`; KDE returns the `Window` and unmaximizes it), `hide` (28: KDE re-runs `update` for a
scrolling workspace whose resize dimensions are set), `reintegrate_maximized_window` (12: KDE
unmaximizes and adds through `add_window`; Windows loads the focused window) and
`new_floating_window` (4: KDE unmaximizes).

(d) Windows: `border_kinds_on_show` (34), `windows_on_show` (10), `hwnd_from_exe` (26), custom layouts
in `load_static_config` (23), `promote_container` (5), `effective_layout_options` (1). KDE:
`adopt_maximized_window` (60), `prune_duplicate_windows` (40), `reap_invalid_windows` (33),
`remove_floating_window` (19), `focus_floating_window` (4), `focus_previous_floating_window` (4),
`percentage_within_horizontal_bounds` (11), and the `scrolling_placed` field.

(b) is almost all of: `hwnd: isize` against `window_id: u32` (`contains_window`,
`contains_managed_window`, `container_idx_for_window`, `container_for_window`,
`focus_container_by_window`, `is_focused_window_monocle_or_maximized`, `move_container_to_workspace`
of the monitor), `WindowsApi::foreground_window()` / `cursor_pos()` against `backend()`,
`let _ = backend().hide(WindowId::from_hwnd(..))` against `window.hide()` (also restore, focus,
maximize), `Option` against `Result` for the title, exe and class (`resolve_placement_index`,
`location_from_exe`), `try_into` against `into` (`visible_window_details`),
`new_container_for_window(Window)` against `(&Window) -> Result<()>`, and `by value` against
`&self.maximized_window` patterns.

### `monitor.rs`

| | functions | changed lines |
|---|---|---|
| identical (non-test) | 8 | 0 |
| (b) | 2 | 18 (`move_container_to_workspace`, `add_container`) |
| (c) | 3 | 40 (`load_focused_workspace`, `update_workspaces_globals`, `update_workspace_globals`) |
| (d) | 9 | 113 (the Windows `name`, `device_id` and `workspace_names` fields: `new`, `placeholder`, `From`, `focus_workspace`, the id serde adapter; Custom layouts in `add_container_with_direction`; KDE `assign_windows_to_workspaces`) |

### What each remaining cluster needs

KDE-side converge revisions (this stack): the (a) and (b) clusters where the Windows form is the
better one. What stays, and what kfw does (the list to hand over):

1. **id parameter name and type** (b): KDE keeps `window_id`; kfw renames `hwnd` to `window_id` and
   takes the id as the `ContainerWindow::Id` (`isize`) so the text is the same.
2. **Backend calls inline** (b): kfw's `backend().hide(WindowId::from_hwnd(..))` and KDE's
   `window.hide()` both become `ContainerWindow::hide_window()` / `restore_window()` (already in the
   crate); focus and maximize become `Window::focus` and `Window::maximize` on both. kfw: Windows `Window`
   needs `focus`, `maximize`, `unmaximize` that call `backend()`; the Windows result handling
   (`let _ =`) moves inside them.
3. **`Option` against `Result`** for title, exe, class, path (b): kfw's `Window::title/exe/class/path`
   return `Option` (as KDE) or both return `Result`; recommended `Option`, since Windows never
   distinguishes the failures.
4. **`WindowDetails`** (b): `From` (KDE) against `TryFrom` (Windows); kfw makes Windows' infallible
   (the `Err` arms were never distinguished).
5. **`new_container_for_window(Window)`** (b): KDE adopts the Windows form in this stack.
6. **`remove_focused_floating_window`** (b): kfw adopts KDE's two-line form over
   `remove_floating_window` (which Windows lacks: add it, it is 19 lines).
7. **Error strings** of `reintegrate_maximized_window` (b): KDE's ("no maximized restore index") are
   right; Windows' copy the monocle ones. kfw adopts KDE's.
8. **(c) hooks**, to be a `WorkspacePlatform` trait the workspace is generic over: `update` needs
   `before_layout` / `after_layout` (Windows: animation batch, `INITIAL_CONFIGURATION_LOADED`, title
   bars, stackbar; KDE: clamp, `scrolling_placed`), `place_window(window, rect)` (set_rect with the
   top flag against `set_position`), and `apply_wallpaper`; `remove_window` needs `on_window_removed`
   (Windows: `border_manager::delete_border`) and a decision on the return value (KDE's `Window` is
   what callers need: kfw returns it too); `hide` needs the scrolling step as a hook
   (`after_hide`) or moves into KDE only; `reintegrate_maximized_window` and `new_floating_window`
   need `unmaximize_window`, a no-op on Windows if its native maximize is not undone there (kfw to
   state which).
9. **(d)** Windows-only features become optional hooks or extension traits (`border_kinds_on_show`,
   `windows_on_show`, custom layouts) and KDE-only ones likewise (`adopt_maximized_window`,
   `prune_duplicate_windows`, `reap_invalid_windows`, floating-window focus helpers,
   `scrolling_placed`). Custom layouts exist in `komorebi-layouts` for both; KDE ignores them today
   (`Layout::Custom` arms): the question for the owner is whether KDE gains them (a behaviour change)
   or the shared code keeps `#[cfg]`-free arms that KDE never reaches.
10. **Monitor fields** (d): `Monitor<P>` with `#[serde(flatten)] platform: P` (decided: the schemars
    `required` order of the runtime notification and socket schemas changes, nothing checked in
    does).

## 8. Design: the platform hooks of a shared `Workspace` (not implemented)

Status: design from kfw (Windows side), written against `wip-u-core` plus the revisions of
`wip-u-kfw2` (items 1 to 7 of section 7 are done on Windows: `window_id`, `hide_window`/`restore_window`,
`Window::focus`/`maximize`/`unmaximize` over `backend()`, `Option` lookups, `WindowDetails: From`,
`remove_floating_window`, the shared error strings). Nothing below is code; no trait exists yet.

### 8.1 Shape

`Workspace<P: WorkspacePlatform>` lives in `komorebi-core`, like `Container<W>`; `P::Window` is the
`ContainerWindow` (`Copy`, with the `Id`). Windows and KDE implement `WorkspacePlatform` on a unit
struct; the daemon uses the alias `type Workspace = komorebi_core::Workspace<WindowsPlatform>`. Every
hook has a default that does nothing (or the generic behaviour), so a platform lists only its
differences. The trait is a generic parameter, not `dyn`: the calls are in the layout hot path and the
platform is fixed at compile time. All hooks take `&mut Workspace<Self>` or the relevant parts, never
the `WindowManager`, so the layer below `window_manager` stays independent of it.

The window-facing operations are NOT hooks: `focus`, `maximize`, `unmaximize`, `hide_window`,
`restore_window`, `title`/`exe`/`class`/`path` (all `Option`) are methods the `ContainerWindow` bound
(extended with `focus`, `maximize`, `unmaximize`, `title`, `exe`, `class`, `path`) requires, which both
`Window` types now have under these names. Hooks are only for behaviour the shared code cannot express
with window operations.

### 8.2 The (c) hooks

| Where | Hook | Windows does | KDE does | Default |
|---|---|---|---|---|
| `update` start | `fn begin_update(&mut self_ws) -> Flow` | returns `Skip` until `INITIAL_CONFIGURATION_LOADED`; opens the `AnimationBatch` (a guard returned in `Flow::Run(guard)`, dropped at the end of `update`, so every move of the pass animates together) | nothing | `Run(())` |
| `update` per layout | `fn adjust_layout(&self_ws, layouts: &mut [Rect], work_area: &Rect)` | nothing (the borders/padding are already applied by `calculate`) | clamps every layout to the work area, applies the percentage override | no-op |
| `update`, a window gets its rect | `fn place_window(&self_ws, window: &P::Window, rect: &Rect, first: bool) -> eyre::Result<()>` | title-bar removal or restore (`REMOVE_TITLEBARS` and the per-window rules), `set_position(rect, top)` with the top flag set for the maximized and monocle windows, stackbar update for stacks | `set_rect`, `scrolling_placed` bookkeeping | `backend().set_rect` |
| `update` end | `fn end_update(&mut self_ws)` | the scrolling `adopt_actual_widths` pass when animations are on and the user is not stepping a live resize (`live_resize::is_stepping`); stackbar redraw | `has_settled` | no-op |
| `apply_wallpaper` | `fn apply_wallpaper(&self_ws, monitor: &MonitorId, wallpaper: &Wallpaper) -> eyre::Result<()>` | `WindowsApi::set_wallpaper` plus the palette theme built inline | asks the backend and calls `theme_manager::wallpaper_palette` | `Ok(())` (wallpaper is KDE/Windows only: the trait has no wallpaper operation, D36) |
| `remove_window` | `fn on_window_removed(&mut self_ws, id: P::Window::Id)` | `border_manager::delete_border(id)` | nothing | no-op |
| `remove_window` result | none: the shared version returns `eyre::Result<Option<P::Window>>`; Windows callers that used `()` ignore it (KDE's callers need the window to unmaximize it) | | | |
| `hide` | `fn after_hide(&mut self_ws)` | nothing | re-runs `update` for a scrolling workspace whose resize dimensions are set (the windows would otherwise stay where the hidden layout left them) | no-op |
| `reintegrate_maximized_window`, `new_floating_window` | `fn unmaximize_window(w: &P::Window)` | none needed: `Window::unmaximize` is `ShowWindow(SW_NORMAL)`, and `update` re-places the window afterwards; KDE needs the compositor told. Statement for the owner: **Windows does not need to unmaximize there, and does not today** (`reintegrate_maximized_window` loads the focused window; `new_floating_window` does not unmaximize), so the hook is a no-op on Windows and the shared code calls it in both | `window.unmaximize()?` | no-op |

`Flow` for `begin_update` is `Option<Guard>`-like: `enum Flow<G> { Skip, Run(G) }` with
`type UpdateGuard` on the trait, `()` by default. `place_window` carries the whole per-window part of
`update`; the shared `update` computes the layouts, walks containers, asks `place_window` for each
window and nothing else. That is where most of the 161 changed lines of `update` go.

Order of the calls inside `update`: `begin_update` -> (maximized/monocle/floating first: `place_window`) ->
for each container: layouts from `adjust_layout`, `place_window` per visible window ->
`end_update`. Both platforms already order it this way; the Windows `AnimationBatch` guard is dropped
after `end_update` (so `end_update` still sees the batch open, as `adopt_actual_widths` does today).

### 8.3 The (d) features

| Feature | Where | Becomes |
|---|---|---|
| `border_kinds_on_show`, `windows_on_show` (Windows) | `workspace.rs`, read by the border manager and `update` | an extension trait `WorkspaceWindowsExt for Workspace<WindowsPlatform>` in the Windows crate: they only read fields (`containers`, `monocle_container`, `maximized_window`, `floating_windows`), so they need those to be `pub`, which they are |
| `hwnd_from_exe` (Windows) | `ContainerExt` today | stays an extension trait in the Windows crate (`ContainerExt`, `WorkspaceExt::window_id_from_exe`) |
| custom layouts in `load_static_config` and `add_container_with_direction` (Windows) | `Layout::Custom` arms | stay in shared code behind the `Layout` enum, which already has `Custom` in `komorebi-layouts`; KDE's arms for it return the default layout. Open question for the owner (section 7 item 9): whether KDE should gain custom layouts (a KDE behaviour change) |
| `promote_container`, `effective_layout_options` (Windows, 5 and 1 lines) | | shared as they are; KDE gains them unchanged (they are generic) |
| `adopt_maximized_window`, `prune_duplicate_windows`, `reap_invalid_windows`, `percentage_within_horizontal_bounds`, the `scrolling_placed` field (KDE) | | a KDE-only extension trait `WorkspaceKdeExt`; `scrolling_placed` moves into `P::WorkspaceState`, an associated type with `Default + Serialize`-skipping (`#[serde(skip)]`) that the platform owns |
| `focus_floating_window`, `focus_previous_floating_window`, `remove_floating_window`'s focus step (KDE) | | the focus step of `remove_floating_window` is a hook `fn after_floating_removed(&mut self_ws)` (no-op on Windows, which does not refocus), the two helpers become shared methods (they are generic) |
| the Windows-only `Monitor` fields (`name`, `device_id`, `workspace_names`) | `monitor.rs` | section 7 item 10: `Monitor<P>` with `#[serde(flatten)] platform: P`; `P` is a `MonitorExtra`, `()` on KDE |

### 8.4 What this needs first

1. `ContainerWindow` (or a `WorkspaceWindow: ContainerWindow` supertrait) grows `focus`, `maximize`,
   `unmaximize`, `title`, `exe`, `class`, `path`. Both `Window`s have them with the same signatures
   after the revisions above (`focus` returns `eyre::Result<()>`, the others `()`/`Option`).
2. `WindowDetails` and `WindowKind`-keyed border data are already plain data; `Wallpaper` and
   `WorkspaceLayer` are in `komorebi-protocol`.
3. The statics that `update` reads (`REMOVE_TITLEBARS`, `INITIAL_CONFIGURATION_LOADED`, the padding and
   border globals) are read inside the hooks, not in shared code. `WorkspaceGlobals` (padding, border
   offset/width, work-area offset) is plain data and is the only global state the shared `Workspace`
   takes, through `update_globals` as today.
4. `border_manager` and `stackbar_manager` stay in the Windows crate and are reached only from
   `place_window`, `on_window_removed` and `end_update`.

### 8.5 Order of work and gates

(1) the `ContainerWindow` extension (pure), (2) move the identical and (b) functions of `workspace.rs`
into the crate generic over the window, keeping the hooks as direct calls under `#[cfg]` for one
revision each (pure move), (3) introduce `WorkspacePlatform` with the defaults and implement it for
both platforms (behaviour-preserving on both), one hook per revision in the order of the table:
`on_window_removed`, `apply_wallpaper`, `place_window`, `begin_update`/`end_update`, `after_hide`,
`unmaximize_window`. Gates: Windows `commands`, `workspaces`, `stacking`, `scrolling`, `resize`,
`borders`, `animation` scenarios per hook; KDE fast tier and VM for `adjust_layout`, `after_hide`,
`unmaximize_window`.

## 9. Review of section 8 against KDE's code, and the order that follows from it

Read against `komorebi-for-kde/komorebi/src/workspace.rs` (`update`, `hide`, `remove_window`) and the
Windows `update`. Section 8 holds, with these corrections and additions; none changes the idea, but
three change a signature.

1. **`begin_update` must be fallible and may change the workspace.** KDE's `update` starts with
   `self.prune_duplicate_windows()?` and then clamps `focused_container_idx` to the container count.
   Both are before anything is laid out, so they belong in `begin_update(&mut ws) ->
   eyre::Result<Flow<G>>`, not in the shared `update`. (Windows has no such step.)
2. **`end_update` must say whether to run the pass again.** The Windows `update` is a loop: after the
   placement pass, `if pass == 0 && is_scrolling && !animations_enabled() && adopt_actual_widths(..)
   { pass += 1; continue; }`. A hook that returns nothing cannot express that; it must be
   `end_update(&mut ws) -> eyre::Result<Pass>` with `enum Pass { Done, Again }`, and the shared `update`
   loops while it answers `Again` (at most one repeat, as Windows does; KDE always says `Done`).
3. **There is one more condition read in the middle of `update`, a hook of its own.** Where the scrolling
   layout decides whether the pass is animated, Windows reads `crate::scrolling::animations_enabled() &&
   !live_resize::is_stepping()` and KDE reads `scrolling_placed.is_some_and(Placed::has_settled) &&
   !live_resize_in_progress()`. Section 8 files `has_settled` under `end_update`, but KDE reads it before
   the layouts are computed. Add `fn scrolling_in_motion(&ws) -> bool` (default `false`); `scrolling_placed`
   is set at the end of KDE's pass, in `end_update`.
4. **Error policy of `place_window` differs** and the hook returns `Result` to carry both: Windows
   propagates a failed `set_rect` with `?`, KDE logs it (`tracing::warn!("failed to set window
   position")`) and goes on, while `center(..)?` in the percentage override does propagate. The KDE
   `place_window` swallows exactly the `set_position` error; the shared caller always uses `?`.
5. **`adjust_layout` is not enough for Windows' stackbar**: it subtracts the stackbar height from the
   layout of a container with more than one window (`should_have_stackbar(window_count)`). That is a
   per-container adjustment, so `adjust_layout` is called per container with its index,
   `adjust_layout(&ws, idx: usize, layout: &mut Rect, work_area: &Rect)`, not once with the slice; KDE's
   clamp is per layout as well and only when the workspace is not scrolling.
6. **KDE wraps the placement in a closure** (`let result = (|| { .. })(); result?;`) with nothing between the closure and the `?`, so it is equivalent to no closure (class (a) drift: the closure only keeps the `?`s of the placement local). The shared `update` needs no such shape; a Windows `AnimationBatch` guard drops on any exit and is compatible with either.
7. **Orphan rule:** a method only one platform has cannot be an inherent method of the generic
   `Workspace<P>` in the crate. `adopt_maximized_window`, `prune_duplicate_windows` and
   `reap_invalid_windows` (KDE), `border_kinds_on_show`, `windows_on_show` (Windows) become extension
   traits, and each file that calls them imports the trait (6 call sites of the KDE three outside
   `workspace.rs` today). The extension traits are the price of keeping the `Workspace` fields `pub`.
8. **`WorkspaceConfig` and `Wallpaper` are already shared** (`komorebi-config`, `komorebi-protocol`), so
   `load_static_config` is not blocked by the daemon's `static_config.rs`; it can move into the crate
   (the Windows custom-layout arms stay in it).
9. **`Monitor` cannot move before `Workspace`.** The shared Monitor code uses about 25 members of its
   workspace (`layout`, `globals`, `tile`, `layer`, `containers_mut`, `add_container_to_back`,
   `insert_container_at_idx`, `floating_windows_mut`, `reintegrate_monocle_container`, ...). Once
   `Workspace<P>` exists those are inherent members and `Monitor<P>` takes `Ring<Workspace<P>>`
   directly; before that, `Monitor` would need the 25-member trait that was ruled out. The order is
   therefore: `WorkspaceGlobals` (plain data), `Workspace<P>`, `Monitor<P>`; the request to do `Monitor`
   first is only possible after the first.
10. **Decision for the owner, `visible-windows`.** Windows' `visible_window_details` used `try_into` and
    skipped a window whose fields could not be read; the shared `visible_window_details` uses `into`, so
    after kfw's revision 4 Windows lists such a window with empty strings in `komorebic visible-windows`.
    The shared code depends on the infallible form; the alternative is a `WindowDetails: TryFrom`
    shared with a skip for `Err`, which KDE (always `Ok`) would not notice. Recommended: keep `into`.

### Order of the code that remains (all gated like section 8.5)

1. `WorkspaceGlobals` into `komorebi-core` (identical plain data; this stack does it).
2. `Workspace<P>` struct and the 50 identical functions, `P::Window: WorkspaceWindow`
   (`ContainerWindow` plus `focus`, `maximize`, `unmaximize`, `title`, `exe`, `class`, `path`, `rect`,
   `center`, `set_position`), the differing functions as hooks or extension traits as in sections 8
   and 9. About 2000 lines move; every call site that used an inherent method of a one-platform
   function imports the extension trait. This is one revision per cluster of functions, and it is
   the first step that cannot be verified on Windows without the e2e scenarios.
3. `Monitor<P>` with `#[serde(flatten)] platform: P`.

## 10. Status of the Workspace move (after D-coord-5)

Landed on `wip-u-core`: `WorkspaceGlobals` in the crate; the nine id/focus/cursor functions of
`workspace.rs` have the same text on both sides (`PlatformWindowId`, `window.id()`,
`foreground_window_id()`, `cursor_position()` as per-daemon helpers in `window.rs`, which are the
future `WorkspacePlatform::foreground_window` and `cursor_pos` hooks).

Measured now (per-function, same method as section 7): 58 functions of `workspace.rs` are
identical on both sides, about 530 lines outside the tests, and they use only the `Container`, `Ring`,
layout and `ContainerWindow` API (the one window call is `window.exe()`). The only identical function
that calls a differing one is `new_container_for_window`, which calls `resolve_placement_index`.
So the first move is: the struct, the 58 identical functions and `resolve_placement_index`, with
every other differing function an extension trait per daemon. What still differs, function by
function, and what removes the difference:

| function | differs by | converge step |
|---|---|---|
| `resolve_placement_index` (25) | `should_act` takes `&[class]` on KDE and `&class` on Windows; `path` is a `PathBuf` on KDE and a `String` on Windows | one `should_act` signature and one `path()` type on both sides (KDE's slice form, `String`) |
| `default` (31) | field order; Windows seeds `workspace_padding`/`container_padding` from the `DEFAULT_*` statics; KDE has `scrolling_placed` | `P::WorkspaceState` for `scrolling_placed`; the padding seed as a `WorkspacePlatform::default_padding` hook |
| `new_maximized_window`, `new_floating_window`, `reintegrate_maximized_window`, `remove_floating_window` | the `unmaximize_window` and `after_floating_removed` hooks of section 8; Windows' `load_focused_window` after reintegration | the hooks |
| `update`, `apply_wallpaper`, `hide`, `restore`, `remove_window` | the (c) hooks of sections 8 and 9 | the hooks, one per revision |
| `load_static_config`, `effective_layout_options`, `promote_container` | the Windows `Layout::Custom` arms | decision of the owner: KDE gains the arms (they are unreachable on KDE today: it cannot load a custom layout) or they stay Windows-only through a hook |
| `visible_window_details` | `into` on both after the Option change; KDE iterates by reference | none beyond a text pass |
| `border_kinds_on_show`, `windows_on_show`, `hwnd_from_exe` (Windows); `adopt_maximized_window`, `prune_duplicate_windows`, `reap_invalid_windows`, `focus_*_floating_window` (KDE) | one-sided | extension traits (section 9, item 7) |
| the unit tests | use the fake backend (KDE) or Win32 (Windows) | stay in each daemon over the alias |

Not done: the move itself and the hook revisions; each needs `P::Window` to carry `title`, `exe`,
`class`, `path`, `focus`, `maximize`, `unmaximize`, `rect` and `set_position` on `ContainerWindow` (or a
`WorkspaceWindow` supertrait) before the first generic function compiles, which is itself a revision
with a stub window in the crate's tests.

## 11. The Workspace move: what landed and where the sharing stops (semantic differences)

Landed on `wip-u-core`: `komorebi-core` holds `Workspace<P: WorkspacePlatform>` with its 50 or so
shared functions (container and focus operations, placement, the resize constraints, the window
lookups, `is_scrolling`, the floating helpers), `WorkspaceGlobals`, `WorkspaceLayer`,
`WorkspaceWindowLocation`, `WorkspaceWindow`, and the hooks `default_padding`, `foreground_window_id`,
`cursor_position`, `should_act_window(_individual)`, the three scrolling lookups and
`after_floating_removed`. KDE (`KdePlatform`, state `scrolling_placed`) and Windows
(`WindowsPlatform`, no state) both use it through `type Workspace = komorebi_core::Workspace<..>`; every
other method of the old impl is a method of `WorkspaceExt` (and `WorkspaceScrolling` for the glue of
`scrolling.rs`), unchanged in body.

The functions left in the extension traits cannot be shared by a converge that keeps behaviour. Read
side by side they differ in what they DO, not in how they are written:

| function | the difference in behaviour |
|---|---|
| `remove_window` | Windows deletes the window's border, removes a floating window itself (`retain`) and returns `()`; KDE has no floating branch (its callers use `remove_floating_window`) and returns the removed `Window`. They test the maximized and the monocle window in the opposite order, and Windows ignores the result of `unmaximize`. |
| `restore` | with a monocle container that has no focused window, Windows carries on to restore the containers, the floating windows and the maximized window; KDE restores the monocle container, applies the wallpaper and returns. |
| `hide` | KDE tiles a scrolling workspace again, without its resize adjustments, before hiding (the `after_hide` hook of section 8); Windows does not. This one is a hook and can be shared once that hook exists. |
| `new_maximized_window` | Windows takes the floating window at the focus index of the ring, KDE the one that has the foreground (`remove_focused_floating_window`); KDE propagates a failed `maximize`, Windows ignores it. |
| `reintegrate_maximized_window` | KDE unmaximizes the window and adds it with `add_window` (which hides the others of the container); Windows pushes it back and calls `load_focused_window`. |
| `new_floating_window` | KDE unmaximizes the window first (a hook, `unmaximize_window`, a no-op on Windows by section 8). |
| `apply_wallpaper` | Windows sets the wallpaper through Win32, builds the palette itself, sends the theme over the daemon's socket and passes the stackbar colours of the wallpaper's theme options; KDE asks the backend, uses `theme_manager::wallpaper_palette` (another cache) and passes `None` for the stackbar colours. |
| `update` | the (c) list of section 7: title bars, stackbars, the animation batch, `adopt_actual_widths` and its second pass on Windows; the work-area clamp, the percentage override and `scrolling_placed` on KDE; and the placement error policy of section 9. |
| `load_static_config`, `effective_layout_options`, `promote_container` | the Windows `Layout::Custom` arms (D-coord-6: a Windows hook). |

Making any of the first five identical means choosing one platform's behaviour for the other, which
is a decision about the daemon, not a refactor. Hooks cover `hide`, `new_floating_window` and the
custom layouts without changing either daemon; `update` and `apply_wallpaper` need a hook for each
platform-specific step plus an agreed shared skeleton, which is a design the owner has to confirm
against the Windows e2e (the first is the lengthiest function in the file). Until then they stay the
daemons' own methods.

`Monitor<P>` is not started. After this move its shared half is the functions that touch only the
core `Workspace` API (`add_container`, `remove_workspace_by_idx`, `ensure_workspace_count`,
`remove_workspaces`, `new_workspace_idx`, `focused_workspace_layout`, `focused_workspace_name`); the
platform half is the constructors (the Windows `name`, `device_id` and `workspace_names`),
`load_focused_workspace`, the two globals updates (`BORDER_ENABLED` against `BORDER_DRAWN`),
`focus_workspace` (the names), `move_container_to_workspace` (the foreground window) and the
Windows-only custom-layout arms of `add_container_with_direction`.

## 12. Proposal: the shared `update` skeleton (not implemented)

Written from the two bodies as they are in `wip-u-core` after the Workspace move (the Windows one is
`WorkspaceExt::update` in `komorebi/src/workspace.rs`, the KDE one in
`komorebi-for-kde/komorebi/src/workspace.rs`). The unified diff of the two, Windows against KDE, is at
the end of this section: every hunk of it is either a line of the skeleton below that both daemons
have (so no hunk), or the body of one of the hooks. Differences that are only the order of two `let`s
(`window_based_work_area_offset*`, `managed_maximized_window`) have no effect and are dropped.

### The skeleton (what moves into `komorebi-core`)

```rust
pub fn update(&mut self) -> eyre::Result<()> {
    let _guard = match P::begin_update(self)? {          // Windows: Skip until INITIAL_CONFIGURATION_LOADED,
        Flow::Skip => return Ok(()),                     //   else Run(AnimationBatch::begin());
        Flow::Run(guard) => guard,                       // KDE: prune_duplicate_windows()?, Run(())
    };

    let focus_before = self.focused_container_idx();
    self.containers_mut().retain(|c| c.is_preselect() || !c.windows().is_empty());
    P::containers_pruned(self, focus_before);            // KDE: if focus_before >= len, focus len-1 (saturating);
                                                         // Windows: nothing (it must NOT clamp)

    // paddings, work area, work-area offset rules, window-based offset, layout rules,
    // window-container-behaviour rules: identical on both daemons, move as they are

    if self.tile {
        if let Some(container) = &mut self.monocle_container {          // identical shape
            if let Some(window) = container.focused_window_mut() {
                /* the three add_padding calls */
                P::place_monocle(window, &adjusted_work_area)?;          // Windows: set_position(rect, top = true)
            }                                                            // KDE: set_position(rect)
        } else if let Some(window) = &mut self.maximized_window {
            P::maximize_in_layout(window)?;                              // Windows: window.maximize() (result ignored
        } else if !self.containers().is_empty() {                        //   inside), KDE: window.maximize()?
            let around_window = container_padding + border_offset + border_width;
            if self.is_scrolling() && P::adopt_widths_before_layout(self) {
                self.adopt_actual_widths(around_window, adjusted_work_area.right);
            }                                                            // Windows: animations_enabled() && !is_stepping()
                                                                         // KDE: scrolling_placed.has_settled() && !live_resize_in_progress()
            let mut pass = 0;
            loop {
                let effective_layout_options = self.effective_layout_options();
                let mut hidden = vec![];
                let mut layouts = if self.is_scrolling() {
                    let others = scrolling::other_monitors(&self.globals.work_area, &P::monitor_rects());
                    /* layout_scrolling + scrolling::rest, as both have it */
                } else {
                    /* layout.as_boxed_arrangement().calculate(..), as both have it */
                };

                for (i, container) in self.containers_mut().iter_mut().enumerate() {
                    if let Some(layout) = layouts.get_mut(i) {
                        P::adjust_layout(container, i, layout, &Placement { adjusted_work_area, work_area,
                            container_padding, border_offset, border_width, is_scrolling });
                        // Windows: add_padding(border_offset), add_padding(border_width), then the stackbar height
                        // KDE: the clamp to the work area when not scrolling, then the two add_padding calls
                        P::place_container(container, i, layout, hidden.get(i).copied().unwrap_or_default(),
                            &Placement { .. })?;
                        // Windows: per window: title bar rules, unminimize after toggle-maximize, hide or
                        //   (restore if should_restore_when_placed, backend().set_rect)
                        // KDE: hide all if hidden, restore the front if should_restore_when_placed, then per window
                        //   the percentage override (center) or set_position, logging a failed set_position
                    }
                }
                self.latest_layout = layouts;

                match P::end_update(self, pass, around_window, adjusted_work_area.right)? {
                    Pass::Again => { pass += 1; continue }   // Windows: pass == 0 && is_scrolling &&
                    Pass::Done => break,                      //   !animations_enabled() && adopt_actual_widths(..)
                }                                             // KDE: records scrolling_placed, always Done
            }
        }
    }

    /* the resize_dimensions resize: identical */
    Ok(())
}
```

### The hooks, with the exact bodies the daemons keep

| hook | Windows body | KDE body |
|---|---|---|
| `begin_update(ws) -> Result<Flow<G>>` | `if !INITIAL_CONFIGURATION_LOADED.load(SeqCst) { return Ok(Flow::Skip) }`; `Ok(Flow::Run(AnimationBatch::begin()))` | `ws.prune_duplicate_windows()?; Ok(Flow::Run(()))` |
| `containers_pruned(ws, focus_before)` | nothing | `if focus_before >= ws.containers().len() { ws.focus_container(focus_before.saturating_sub(1)); }` |
| `place_monocle(window, rect)` | `window.set_position(rect, true)?` | `window.set_position(rect)?` |
| `maximize_in_layout(window)` | `window.maximize()` (it is `backend().maximize` ignoring the result today) then `Ok(())` | `window.maximize()?` |
| `adopt_widths_before_layout(ws) -> bool` | `crate::scrolling::animations_enabled() && !crate::live_resize::is_stepping()` | `ws.state.scrolling_placed.as_ref().is_some_and(Placed::has_settled) && !crate::window::live_resize_in_progress()` |
| `monitor_rects()` | `WindowsApi::monitor_rects()` | `backend().monitors().map(\|m\| m.iter().map(\|m\| m.size).collect()).unwrap_or_default()` |
| `adjust_layout(container, i, layout, placement)` | the two `add_padding` calls, then `if stackbar_manager::should_have_stackbar(window_count) { layout.top += tab_height + container_padding; layout.bottom -= tab_height + container_padding }` | `if !is_scrolling { clamp left/right/top/bottom to adjusted_work_area }`, then the two `add_padding` calls |
| `place_container(container, i, layout, hidden, placement)` | the loop over `container.windows()` in the diff below, with `managed_maximized_window` read from the workspace | the hide / restore-front / per-window loop in the diff below |
| `end_update(ws, pass, around_window, right) -> Result<Pass>` | `if pass == 0 && ws.is_scrolling() && !animations_enabled() && ws.adopt_actual_widths(around_window, right) { Pass::Again } else { Pass::Done }` | `if ws.is_scrolling() { ws.state.scrolling_placed = Some(Placed::new(..)) } Ok(Pass::Done)` |

Constraints that make this safe to implement, all checked against the diff:

- The order of effects is the same on both daemons: begin, prune, work area, layout rules, then the
  tile branch. KDE's closure wrapper (`let result = (|| { .. })(); result?;`) has nothing between the
  closure and the `?`, so it does not need a counterpart (section 9, item 6).
- `Windows` must not get KDE's focus clamp, and KDE must not get the `INITIAL_CONFIGURATION_LOADED`
  check; both are hooks that the other daemon leaves empty.
- `place_container` receives `hidden` already looked up for the container; KDE's `continue` after
  hiding is the end of that hook.
- The Windows `AnimationBatch` guard must live until the end of `update`, which `_guard` does.
- Tests to run before and after: the KDE `update` tests of `workspace.rs` and `scrolling.rs`
  (`update_puts_each_window_on_its_tile`, the scrolling tests) and, for Windows, the e2e sweeps of
  commands, stacking, scrolling, resize, borders and animation named in the Windows revision of the
  Workspace move; the e2e must run before the Windows revision of this step is accepted.

### The bodies, as a diff (Windows `-`, KDE `+`)

```diff
@@ -1,15 +1,15 @@
     fn update(&mut self) -> eyre::Result<()> {
-        if !INITIAL_CONFIGURATION_LOADED.load(Ordering::SeqCst) {
-            return Ok(());
-        }
-
-        // every window that moves in this pass is animated together
-        let _animations = AnimationBatch::begin();
-
         // make sure we are never holding on to empty containers
+        self.prune_duplicate_windows()?;
+        let focused_container_idx = self.focused_container_idx();
         self.containers_mut()
             .retain(|c| c.is_preselect() || !c.windows().is_empty());
 
+        // make sure we are never focused on a no longer existent final empty container
+        if focused_container_idx >= self.containers().len() {
+            self.focus_container(focused_container_idx.saturating_sub(1));
+        }
+
         let container_padding = self
             .container_padding
             .or(self.globals.container_padding)
@@ -21,8 +21,6 @@
         let border_width = self.globals.border_width;
         let border_offset = self.globals.border_offset;
         let work_area = self.globals.work_area;
-        let window_based_work_area_offset = self.globals.window_based_work_area_offset;
-        let window_based_work_area_offset_limit = self.globals.window_based_work_area_offset_limit;
         let mut rules_work_area_offset = None;
 
         if !self.work_area_offset_rules.is_empty() {
@@ -42,6 +40,8 @@
         let work_area_offset = rules_work_area_offset
             .or(self.work_area_offset)
             .or(self.globals.work_area_offset);
+        let window_based_work_area_offset = self.globals.window_based_work_area_offset;
+        let window_based_work_area_offset_limit = self.globals.window_based_work_area_offset_limit;
 
         let mut adjusted_work_area = work_area_offset.map_or_else(
             || work_area,
@@ -55,6 +55,7 @@
                 with_offset
             },
         );
+
         if (self.containers().len() <= window_based_work_area_offset_limit as usize
             || self.monocle_container.is_some() && window_based_work_area_offset_limit > 0)
             && self.apply_window_based_work_area_offset
@@ -102,33 +103,37 @@
             self.window_container_behaviour = updated_behaviour;
         }
 
-        let managed_maximized_window = self.maximized_window.is_some();
-
         if self.tile {
-            if let Some(container) = &mut self.monocle_container {
-                if let Some(window) = container.focused_window_mut() {
-                    adjusted_work_area.add_padding(container_padding);
-                    adjusted_work_area.add_padding(border_offset);
-                    adjusted_work_area.add_padding(border_width);
-                    window.set_position(&adjusted_work_area, true)?;
-                };
-            } else if let Some(window) = &mut self.maximized_window {
-                window.maximize();
-            } else if !self.containers().is_empty() {
-                // windows that were not what they were told to be since the last layout, which
-                // is something that animations only show once they are done
-                let around_window = container_padding + border_offset + border_width;
-                // neighbours of a window that is dragged are not yet the size they are being
-                // made while it is followed: that is not what their columns are to be
-                if self.is_scrolling()
-                    && crate::scrolling::animations_enabled()
-                    && !crate::live_resize::is_stepping()
-                {
-                    self.adopt_actual_widths(around_window, adjusted_work_area.right);
-                }
+            // the backend batches the commands this produces and sends them together
+            let result = (|| -> eyre::Result<()> {
+                if let Some(container) = self.monocle_container.as_mut() {
+                    if let Some(window) = container.focused_window_mut() {
+                        adjusted_work_area.add_padding(container_padding);
+                        adjusted_work_area.add_padding(border_offset);
+                        adjusted_work_area.add_padding(border_width);
+                        window.set_position(&adjusted_work_area)?;
+                    };
+                } else if let Some(window) = self.maximized_window {
+                    // the compositor's own maximize, which is the work area and not the padded one
+                    window.maximize()?;
+                } else if !self.containers().is_empty() {
+                    let is_scrolling = self.is_scrolling();
+                    let around_window = container_padding + border_offset + border_width;
+
+                    // a window that would not get as narrow as its column was learned of once the
+                    // windows have had time to draw at the sizes that they were sent, and not
+                    // while one is being dragged, whose neighbours are on their way
+                    if is_scrolling
+                        && self
+                            .state
+                            .scrolling_placed
+                            .as_ref()
+                            .is_some_and(crate::scrolling::Placed::has_settled)
+                        && !crate::window::live_resize_in_progress()
+                    {
+                        self.adopt_actual_widths(around_window, adjusted_work_area.right);
+                    }
 
-                let mut pass = 0;
-                loop {
                     let effective_layout_options = self.effective_layout_options();
 
                     tracing::debug!(
@@ -138,12 +143,15 @@
                         self.layout_options,
                         self.layout_options_rules.len(),
                     );
+                    // the containers that rest where another monitor is seen from, which are hidden
                     let mut hidden = vec![];
-                    let mut layouts = if self.is_scrolling() {
-                        let others = crate::scrolling::other_monitors(
-                            &self.globals.work_area,
-                            &WindowsApi::monitor_rects(),
-                        );
+                    let mut layouts = if is_scrolling {
+                        let monitors: Vec<Rect> = backend()
+                            .monitors()
+                            .map(|monitors| monitors.iter().map(|monitor| monitor.size).collect())
+                            .unwrap_or_default();
+                        let others =
+                            crate::scrolling::other_monitors(&self.globals.work_area, &monitors);
 
                         self.layout_scrolling(&adjusted_work_area, container_padding)
                             .into_iter()
@@ -173,69 +181,87 @@
                         )
                     };
 
-                    let should_remove_titlebars = REMOVE_TITLEBARS.load(Ordering::SeqCst);
-                    let no_titlebar = NO_TITLEBAR.lock().clone();
-                    let regex_identifiers = REGEX_IDENTIFIERS.lock().clone();
+                    let resize_dimensions_is_empty = self.resize_dimensions.is_empty();
 
                     let containers = self.containers_mut();
 
                     for (i, container) in containers.iter_mut().enumerate() {
-                        let window_count = container.windows().len();
-
                         if let Some(layout) = layouts.get_mut(i) {
+                            // The scrolling layout is a strip that is meant to run past the edges of
+                            // the work area, and says itself where each window rests when it is out
+                            // of view (KWin does not constrain where a window is put). Everything
+                            // else is clamped to the work area so it cannot bleed onto an adjacent
+                            // monitor.
+                            if !is_scrolling {
+                                // horizontal
+                                if layout.left < adjusted_work_area.left {
+                                    layout.right -= adjusted_work_area.left - layout.left;
+                                    layout.left = adjusted_work_area.left;
+                                }
+                                let max_right_edge =
+                                    adjusted_work_area.left + adjusted_work_area.right;
+                                let current_right_edge = layout.left + layout.right;
+                                if current_right_edge > max_right_edge {
+                                    layout.right = max_right_edge - layout.left;
+                                }
+
+                                // vertical
+                                if layout.top < adjusted_work_area.top {
+                                    layout.bottom -= adjusted_work_area.top - layout.top;
+                                    layout.top = adjusted_work_area.top;
+                                }
+                                let max_bottom_edge =
+                                    adjusted_work_area.top + adjusted_work_area.bottom;
+                                let current_bottom_edge = layout.top + layout.bottom;
+                                if current_bottom_edge > max_bottom_edge {
+                                    layout.bottom = max_bottom_edge - layout.top;
+                                }
+                            }
+
                             layout.add_padding(border_offset);
                             layout.add_padding(border_width);
 
-                            if stackbar_manager::should_have_stackbar(window_count) {
-                                let tab_height = STACKBAR_TAB_HEIGHT.load(Ordering::SeqCst);
-                                let total_height = tab_height + container_padding;
-
-                                layout.top += total_height;
-                                layout.bottom -= total_height;
+                            if hidden.get(i).copied().unwrap_or_default() {
+                                // it is seen on another monitor wherever it rests here
+                                for window in container.windows() {
+                                    window.hide();
+                                }
+                                continue;
                             }
 
-                            for window in container.windows() {
-                                if container
-                                    .focused_window()
-                                    .is_some_and(|w| w.hwnd == window.hwnd)
-                                {
-                                    let should_remove_titlebar_for_window = should_act(
-                                        &window.title().unwrap_or_default(),
-                                        &window.exe().unwrap_or_default(),
-                                        &window.class().unwrap_or_default(),
-                                        &window.path().unwrap_or_default(),
-                                        &no_titlebar,
-                                        &regex_identifiers,
+                            // a window that was hidden for resting past an edge with another
+                            // monitor next to it is shown before it is put where the strip says
+                            let front = container.focused_window().map(|window| window.id);
+                            let restore = is_scrolling
+                                && front.is_some_and(|front| {
+                                    crate::scrolling::should_restore_when_placed(
+                                        front,
+                                        Some(front),
+                                        &HIDDEN_WINDOWS.lock(),
                                     )
-                                    .is_some();
-
-                                    if should_remove_titlebars && should_remove_titlebar_for_window
-                                    {
-                                        window.remove_title_bar()?;
-                                    } else if should_remove_titlebar_for_window {
-                                        window.add_title_bar()?;
-                                    }
-
-                                    // If a window has been unmaximized via toggle-maximize, this block
-                                    // will make sure that it is unmaximized via restore_window
-                                    if window.is_maximized() && !managed_maximized_window {
-                                        let _ =
-                                            backend().unminimize(WindowId::from_hwnd(window.hwnd));
-                                    }
-                                }
-                                if hidden.get(i).copied().unwrap_or_default() {
-                                    // it is seen on another monitor wherever it rests here
-                                    window.hide_window();
-                                } else {
-                                    if crate::scrolling::should_restore_when_placed(
-                                        window.hwnd,
-                                        container.focused_window().map(|w| w.hwnd),
-                                        &HIDDEN_HWNDS.lock(),
-                                    ) {
-                                        window.restore_window();
-                                    }
+                                });
+                            if restore && let Some(window) = container.focused_window() {
+                                window.restore();
+                            }
 
-                                    backend().set_rect(WindowId::from_hwnd(window.hwnd), layout)?;
+                            for window in container.windows_mut() {
+                                let current_rect = window.rect().unwrap_or_default();
+                                let current_percentage =
+                                    work_area.percentage_within_horizontal_bounds(&current_rect);
+                                let proposed_percentage =
+                                    work_area.percentage_within_horizontal_bounds(layout);
+
+                                // the strip puts a window where it goes, however far
+                                let percentage_override = !is_scrolling
+                                    && proposed_percentage == 0.0
+                                    && current_percentage > 0.1
+                                    && current_percentage < 100.0
+                                    && resize_dimensions_is_empty;
+
+                                if percentage_override {
+                                    window.center(&work_area, false)?;
+                                } else if let Err(error) = window.set_position(layout) {
+                                    tracing::warn!("failed to set window position: {error}")
                                 }
                             }
                         }
@@ -243,22 +269,19 @@
 
                     self.latest_layout = layouts;
 
-                    // with animations off every window that changed width has been asked what it
-                    // is by now, so a window that refused is known, and everything after it is
-                    // placed again with room for it. Once is enough: a layout is never made to
-                    // chase the sizes that are read back this time round.
-                    if pass == 0
-                        && self.is_scrolling()
-                        && !crate::scrolling::animations_enabled()
-                        && self.adopt_actual_widths(around_window, adjusted_work_area.right)
-                    {
-                        pass += 1;
-                        continue;
+                    if is_scrolling {
+                        self.state.scrolling_placed = Some(crate::scrolling::Placed::new(
+                            self.containers()
+                                .iter()
+                                .zip(self.latest_layout.iter())
+                                .map(|(container, layout)| (container.id.clone(), layout.right)),
+                        ));
                     }
-
-                    break;
                 }
-            }
+                Ok(())
+            })();
+
+            result?;
         }
 
         // Always make sure that the length of the resize dimensions vec is the same as the
```
