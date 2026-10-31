# Windows: which call sites go through `backend()`

Step 8 of the unified-binaries plan. `komorebi::windows_backend::backend()` returns the shared
`komorebi_backend::Backend` (a stateless `WindowsBackend` over `WindowsApi` and `Window`). The daemon
is switched one cluster at a time; each switch is its own revision marked BEHAVIOUR.

What every switched call changes in the Windows daemon: `WindowsBackend` first asks `IsWindow` and
fails with `Error::WindowGone` for a dead window, and a failure of a window that died meanwhile is
reported as `WindowGone` (the error text changes from the Win32 one). Call sites use `?` or ignore the
result; none inspects the error. The operations return `Result` where `Window::hide`, `restore` and
`minimize` returned `()`: those sites ignore it (`let _ =`), as the old code could not fail.

| Cluster | Operation | Status | Sites | Left on `Window` / `WindowsApi`, and why | e2e needed |
|---|---|---|---|---|---|
| focus | `focus(id, mouse_follows)` | switched | process_event 4, process_command 3, workspace 7, window_manager 24, focus_manager 1, stackbar 1 | `animation/transition.rs:534` calls `Window::focus` from inside the slide-in code that `Window::focus` itself defers to | commands, focus, focus_follows_mouse, stacking, workspaces, lifecycle |
| frame | `set_rect(id, rect)` | switched for `top = false` | workspace.rs (layout update), stackbar tab click | `set_position(.., true)` callers (window.rs `move_to_area`/`center`, workspace maximize area): the shared `set_rect` has no `top` flag | commands, resize, layouts, stacking, scrolling, workspaces |
| frame | `window_rect(id)` | NOT switched | 0 of about 60 | the shared `window_rect` is built on `window(id)`, which asks title, class, exe and path of the window, gives `None` for a failure and the call sites rely on `Err` or `unwrap_or_default`. Needs a cheap frame query on the trait (a `frame(id) -> Result<Rect>`) or an override of the provided method with an error | |
| hide / show | `hide`, `restore` | switched for the plain forms | container, workspace, monitor, process_command, process_event, window_manager (23) | `hide_with_border` and `restore_with_border` (no border flag in the trait); `animation/transition.rs` and `ghost_dispatcher.rs` (inside the slide-in machinery); the `Container`/`Workspace` methods stay, they call these | commands, workspaces, stacking, lifecycle_hidden |
| hide / show | `minimize`, `close`, `raise` | switched | process_command (close 1, minimize 1, raise 4), monitor_reconciliator (minimize 4) | | commands, monitor reconciliation, lifecycle |
| hide / show | `unminimize`, `maximize` | not switched | | `unminimize` has no daemon caller (the daemon uses `restore`); `maximize`/`unmaximize` need the `Capability::Maximize` check and keep their own bookkeeping in workspace.rs | |
| queries | `foreground_window`, `cursor_pos`, `monitors`, `monitor_from_point`, `window` | NOT switched | 0 | `WindowsApi::foreground_window` is `Result<isize>` (null handle is an `Err` with a message the callers pass on), the trait gives `Option<WindowId>`; `cursor_pos` is `Result<POINT>` vs `Option<(i32, i32)>`; `monitors` rebuilds `MonitorInfo` (drops the `Monitor` fields the daemon needs); `monitor_from_point` is never `None` on Windows. Each switch would drop an error the caller reports or fabricate a default | |
