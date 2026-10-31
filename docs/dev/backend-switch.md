# Windows: which call sites go through `backend()`

Step 8 of the unified-binaries plan. `komorebi::windows_backend::backend()` returns the shared
`komorebi_backend::Backend` (a stateless `WindowsBackend` over `WindowsApi` and `Window`). The daemon
is switched one cluster at a time; each switch is its own revision marked BEHAVIOUR.

What every switched call changes in the Windows daemon: `WindowsBackend` first asks `IsWindow` and
fails with `Error::WindowGone` for a dead window (hide, restore, minimize, maximize and the other `()`
returning operations are then skipped: before, `Window::hide` of a dead window still ran
`ShowWindow` on it, a no-op, and recorded its handle in `HIDDEN_HWNDS`), and a failure of a window
that died meanwhile is reported as `WindowGone` (the error text changes from the Win32 one). Call sites
use `?` or ignore the result; none inspects the error. `frame` is the exception: it does not ask
`IsWindow` first, so the Win32 error is what callers get.

## Switched

| Cluster | Operation | Sites | Notes | e2e needed |
|---|---|---|---|---|
| focus | `focus(id, mouse_follows)` | 40 (process_event, process_command, workspace, window_manager, focus_manager, stackbar) | `animation/transition.rs` calls `Window::focus` from the slide-in code `Window::focus` defers to: left | commands, focus, focus_follows_mouse, stacking, workspaces, lifecycle |
| frame write | `set_rect(id, rect)` | 2 (workspace layout update, stackbar tab click) | `top = false` only | commands, resize, layouts, stacking, scrolling, workspaces |
| frame read | `frame(id)` | about 55, one revision per file: stackbar_manager (2 files), animation (snapshot, ghost_dispatcher, ghost test), live_resize, pan, scrolling, process_command, process_event, window_manager, border_manager (2 files) | `frame` is a provided method added to the trait for this (WindowsBackend: `Window::window_rect`, one DWM call; error kept). `Window::window_rect`/`WindowsApi::window_rect` remain only inside `window.rs` (the implementation, and `Window::set_position`/`move_to_area`/`center`/`focus` internals) | resize, layouts, stacking, scrolling, borders, animation, commands, lifecycle |
| hide / show | `hide`, `restore` (plain forms) | 23 | | commands, workspaces, stacking, lifecycle_hidden |
| hide / show | `minimize`, `close`, `raise` | 1 + 1 + 4 (process_command), 4 minimize (monitor_reconciliator) | | commands, monitor reconciliation, lifecycle |
| maximize | `maximize(id, bool)` | 5 (workspace.rs) | capability query already in the trait (`supports(Capability::Maximize)`, true here) | maximizing, commands, workspaces |

## Left on purpose (platform-owned subsystems, seam.md rule 5)

- `set_position(.., true)` (window.rs `move_to_area`, `center`, workspace maximize area): the raise-to-front
  flag is a Windows detail of the layout/animation code, the shared `set_rect` has none.
- `hide_with_border`, `restore_with_border`: borders are a platform subsystem (own windows and threads).
- `animation/transition.rs`, `animation/ghost*.rs` (except the frame reads above): the DWM ghost and
  slide-in machinery.
- `set_opacity`: the adapter writes the plain alpha, `Window::transparent`/`opaque` take the alpha from the
  transparency manager and animate.

## Not switched, and what the trait would need (for step 9)

| Operation | Why it stays on the platform API | What the trait would need |
|---|---|---|
| `foreground_window` | `WindowsApi::foreground_window() -> Result<isize>`: a null handle is an `Err` whose message callers pass on; the trait has `Option<WindowId>` | a `Result` variant (`Error::Platform` for "no foreground window"), or accept losing the message |
| `cursor_pos` | `Result<POINT>` vs `Option<(i32, i32)>`; callers `?` it | same: a `Result` return |
| `monitors` / `window` | `monitors()` rebuilds `MonitorInfo` and drops the `Monitor` fields the daemon needs (device id, serial, device, work-area offsets); `window()` asks title, class, exe, path | the daemon-side `Monitor` fields in `MonitorInfo` (or a platform extension), and for step 9 the monitor model moving to the core |
| `monitor_from_point` | never `None` on Windows (`MONITOR_DEFAULTTONEAREST`), `None` outside every monitor on KDE and macOS | an explicit "nearest" flag or two methods |
