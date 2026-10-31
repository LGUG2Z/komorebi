# Windows: which call sites go through `backend()`

Step 8 of the unified-binaries plan. `komorebi::windows_backend::backend()` returns the shared
`komorebi_backend::Backend` (a stateless `WindowsBackend` over `WindowsApi` and `Window`). The daemon
switches one cluster at a time; each switch is its own revision marked BEHAVIOUR.

What every switch changes, for the Windows daemon: `WindowsBackend` first asks `IsWindow` and fails
with `Error::WindowGone` for a dead window, and a failure of a window that died meanwhile is reported
as `WindowGone` (the message text changes: it was the Win32 error). Call sites use `?` or ignore the
result, none inspects the error.

| Cluster | Operation | Status | Sites | Left on `Window`/`WindowsApi`, and why | e2e needed |
|---|---|---|---|---|---|
| focus | `focus(id, mouse_follows)` | switched | process_event (4), process_command (3), workspace (7), window_manager (24), focus_manager, stackbar | `animation/transition.rs:534` calls `Window::focus` from inside the slide-in code that `Window::focus` itself defers to (it would recurse through the backend for no gain) | commands, focus, focus_follows_mouse, stacking, workspaces, lifecycle |
| frame / position | `set_rect`, `window_rect` | not switched | | `Window::set_position` has the `top` flag, animation/slide-in/live-resize guards that the backend `set_rect` (always `top = false`) cannot carry | |
| hide / show | `hide`, `restore`, `minimize`, `unminimize`, `raise`, `close` | not switched | | | |
| queries | `window`, `foreground_window`, `cursor_pos`, `monitors`, `monitor_from_point` | not switched | | | |
