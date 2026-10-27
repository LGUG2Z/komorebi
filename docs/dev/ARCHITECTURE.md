# Architecture

komorebi is a tiling window manager. It decides where every window goes and asks a
compositor to put it there. Everything it needs from the compositor is behind one trait,
[`Backend`](../../komorebi/src/backend/mod.rs), so the window manager is written, and
tested, without one.

This page describes that trait, who implements and calls it, where each of its rules is
enforced, the model of the desktop the window manager tiles, and how an event about a
window becomes a tile. It grows with the code: each part of the daemon that is added has
its place here.

## Vocabulary

- **Window**: a top-level surface a client has asked the compositor for, named by the
  compositor's `u32` id.
- **Monitor**: a connected output, with its bounds and its work area (its bounds minus
  the panels).
- **Workspace**: the containers a monitor shows, in order, and the layout that tiles
  them. A monitor has a ring of workspaces and shows one of them.
- **Container**: what a layout places. It gets one tile, and every window in it is put on
  that tile.
- **Layout**: the arrangement (BSP, columns, rows, ...) that turns a work area and a
  number of containers into one rectangle, a **tile**, for each container.
- **Event**: what the compositor reports about a window: it was shown, destroyed, given
  the focus, minimized or restored ([`WindowManagerEvent`]).
- **Compositor**: the program that owns the screen and the windows. On KDE Plasma it is
  KWin.
- **Backend**: the implementation of the [`Backend`](../../komorebi/src/backend/mod.rs)
  trait that talks to a compositor.
- **Fake**: [`FakeBackend`](../../komorebi/src/backend/fake.rs), the in-memory backend
  the tests use.

## Design

The window manager reads the desktop and changes it through [`Backend`] and through
nothing else. A real backend answers from a table that compositor events keep up to date,
and sends commands to the compositor. The fake answers from a table the test fills in, and
records the commands. The window manager cannot tell them apart, which is the point:
the same logic runs against both. The reasons are in
[DESIGN.md](DESIGN.md#why-is-everything-the-window-manager-needs-behind-one-trait).

## The model

The desktop, as the window manager sees it, is two lists and a pointer:

- **Windows**, each a [`WindowInfo`]: its id, process, title, `app_id`, executable, frame,
  whether it is minimized, whether it is a normal application window (as opposed to a
  dialog, a splash screen or a dock), and whether it is alive. A window that has been
  destroyed stays in the table for a short time with `alive` false, so that the event that
  mentions it can still be logged.
- **Monitors**, each a [`MonitorInfo`]: its id, name, an identity that survives a
  hot-plug, its bounds and its work area.
- **The pointer**, a position if the compositor says where it is, and the **focus**, the id
  of the window that has it.

A command is one of: move and resize a window, focus it, raise it, minimize it or bring it
back, close it, and put the pointer in the middle of a rectangle.

The window manager tiles a tree of four levels, each level a [`Ring`]: a list with one
element focused, so that "the focused workspace" or "the focused container" means
the same thing at every level.

- A **monitor** ([`Monitor`]) is one of the backend's monitors: its bounds, its work area,
  its padding, an optional work area offset, and its workspaces. It shows the focused one.
- A **workspace** ([`Workspace`]) has a layout, its padding, and its containers, in order.
- A **container** ([`Container`]) has its windows. A new window gets a container of its
  own. A container can be locked, which keeps it at its index while others are inserted,
  removed or swapped around it.
- A **window** ([`Window`]) is a `Copy` handle that carries only the compositor's id.
  Everything else about the window, and every change to it, goes through the backend.

Tiling a workspace ([`Workspace::update`]) is a function of this tree and a work area. The
monitor gives its focused workspace the work area, the padding to fall back to and the
offset ([`WorkspaceGlobals`]). The workspace takes the offset and its padding off the work
area, the layout gives one tile per container, in order, each shrunk by the container
padding, and every window of a container is asked to move onto its tile with `set_rect`. A
tile is clamped to the work area, so it cannot reach onto a neighboring monitor.

The [`WindowManager`] keeps the monitors, the windows the user minimized (they are in no
workspace until they are restored), and an index from each window it manages to the
monitor and workspace that have it. It is loaded from the backend at startup: every
monitor, and every open window that is normal, alive and not minimized, on the focused
workspace of the monitor its middle is on. After that it changes only when an event comes
in, and each event ends with the workspace in front tiled again.

## The parts

- [`backend`](../../komorebi/src/backend/mod.rs): the trait, the two data types, and the
  process-wide handle ([`set_backend`], [`backend`], [`with_backend`]).
- [`backend::fake`](../../komorebi/src/backend/fake.rs): the fake.
- [`ring`](../../komorebi/src/ring.rs): the [`Ring`], and the macro that gives every type
  holding one the same accessors (`containers()`, `focused_container_mut()`, ...).
- [`monitor`](../../komorebi/src/monitor.rs), [`workspace`](../../komorebi/src/workspace.rs),
  [`container`](../../komorebi/src/container.rs), [`window`](../../komorebi/src/window.rs):
  the four levels of the tree.
- [`lockable_sequence`](../../komorebi/src/lockable_sequence.rs): inserting, removing and
  swapping in a list where some elements are locked in place, which is how a workspace
  changes its containers.
- [`window_manager`](../../komorebi/src/window_manager.rs): the [`WindowManager`] and the
  operations on it (tile the workspace in front, tile every monitor, take a minimized
  window out).
- [`desktop`](../../komorebi/src/desktop.rs): loads the monitors and the open windows from
  the backend.
- [`window_manager_event`](../../komorebi/src/window_manager_event.rs) and
  [`process_event`](../../komorebi/src/process_event.rs): the events, and what the window
  manager does with each.
- [`main.rs`](../../komorebi/src/main.rs): the daemon. It installs the backend chosen on the
  command line (`--fake-backend` is the only one), loads and tiles the desktop, and waits to
  be stopped.

The handle is a process-wide singleton so that a window can be a small `Copy` value
that carries only its id, and ask the backend for what it needs. A test replaces it for
its own thread with [`with_backend`], which puts the previous one back when the test is
done, also if it panics.

## Traits and roles

**[`Backend`] is the compositor.** It is implemented once for each compositor and once for
the fake. The window manager is its only caller, through [`backend`]. The contract:

- Queries (`windows`, `window`, `foreground_window`, `cursor_pos`) answer from the
  backend's own table and never wait for the compositor. `monitors` may fail.
- A command returns once the compositor has accepted it, not once the client has acted on
  it. The frame a window ends up with is what `window_rect` says afterwards, because a
  client may refuse a size.
- A command about a window that does not exist, or is no longer alive, is an error. It
  never panics.
- `shutdown`, `is_window`, `window_rect` and `monitor_from_point` have defaults, written in
  terms of the queries. An implementor should not need to override them, except `shutdown`
  when it created something in the compositor that has to be given up.

## What is guaranteed, and by what

| Rule | Enforced by | Proof | Escape hatch |
|---|---|---|---|
| A backend implements every query and command | type system: the methods have no defaults | `compile_fail,E0046` on [`Backend`] | none |
| A backend can be shared between threads | type system: `Backend: Send + Sync + 'static` | `compile_fail,E0277` on [`Backend`] | none |
| The window manager asks for a backend that was never installed | runtime error: [`backend`] panics, naming [`set_backend`] and [`with_backend`] | `# Panics` on [`backend`] | none |
| A test's backend does not outlive the test | framework path: [`with_backend`] restores the previous one on drop | `with_backend_puts_the_previous_backend_back_when_the_closure_panics` | none |
| A command about a missing window is an error, not a panic | test | `a_command_about_a_window_nobody_opened_is_an_error` and `a_command_about_a_closed_window_is_an_error`, against the fake only | none |
| A locked container keeps its index | framework path: a [`Workspace`] inserts, removes and swaps its containers through `LockableSequence` | `test_locked_containers_with_new_window` and `test_locked_containers_remove_window` | `containers_mut()` is the `VecDeque` itself |
| A window komorebi does not manage is never moved | framework path: `handle_event` asks [`Window::should_manage`] before it acts on any event but a destroy | `dialogs_and_other_non_normal_windows_are_left_alone` | none |
| A destroyed window leaves whichever workspace has it | framework path: the destroy is looked up in the index of managed windows, which is rebuilt at startup and before and after every event | `a_window_that_was_open_before_komorebi_started_is_removed_when_it_closes` | none |
| The daemon does not start without a backend | startup check: a usage error, exit code 2 | `without_a_backend_komorebi_exits_with_an_error_naming_the_flag` | none |
| Queries never block on the compositor | convention only | none | none |
| Only the backend module reaches the compositor | convention only | none | none |

The last two rows are rules nothing checks. A real backend that waits inside a query
breaks the first, and a module that talks to D-Bus directly breaks the second.

## One event, end to end

A window is shown, and ends up on a tile:

1. The event is a [`WindowManagerEvent`]`::Show` carrying the window and the compositor
   signal it came from. The end-to-end tests build it and call
   [`WindowManager::process_event`], which hands it to `handle_event` and logs how long
   that took.
2. `handle_event` asks [`Window::should_manage`], which asks the backend's `window(id)`
   whether the window is alive and normal. If it is not, the event changes nothing.
3. The index of managed windows is rebuilt from the containers. If the workspace in front
   already has the window, or the index has it on another monitor or workspace, the event
   creates nothing.
4. [`Workspace::new_container_for_window`] puts the window in a new container after the
   focused one, inserted through `LockableSequence` so that a locked container keeps its
   index, and focuses the new container.
5. [`WindowManager::update_focused_workspace`] asks the focused [`Monitor`] to give its
   focused workspace the work area, the padding and the offset ([`WorkspaceGlobals`]),
   and calls [`Workspace::update`].
6. `Workspace::update` asks the layout for one tile per container, clamps each to the work
   area, and calls [`Window::set_position`] for each window, which is the backend's
   `set_rect`. The fake puts the window there at once; the frame a window ends up with is
   what `window_rect` says afterwards.
7. If the new container is the only one, the window is given the focus with the backend's
   `focus`, and the pointer follows it.
8. The index is rebuilt again, so the next event finds the window.

## Where to look in the code

- [`komorebi/src/backend/mod.rs`](../../komorebi/src/backend/mod.rs): the trait.
- [`komorebi/src/backend/fake.rs`](../../komorebi/src/backend/fake.rs): the fake and its
  tests.
- [`komorebi/src/core`](../../komorebi/src/core/mod.rs): the vocabulary the rest is written
  in.
- [`komorebi/src/workspace.rs`](../../komorebi/src/workspace.rs): tiling, in
  `Workspace::update`.
- [`komorebi/src/monitor.rs`](../../komorebi/src/monitor.rs): the work area, padding and
  offset a monitor gives the workspace it shows.
- [`komorebi/src/process_event.rs`](../../komorebi/src/process_event.rs): what each event
  does.
- [`komorebi/tests/wm_events.rs`](../../komorebi/tests/wm_events.rs): the window manager
  driven end to end against the fake.

[`Backend`]: ../../komorebi/src/backend/mod.rs
[`WindowInfo`]: ../../komorebi/src/backend/mod.rs
[`MonitorInfo`]: ../../komorebi/src/backend/mod.rs
[`set_backend`]: ../../komorebi/src/backend/mod.rs
[`backend`]: ../../komorebi/src/backend/mod.rs
[`with_backend`]: ../../komorebi/src/backend/mod.rs
[`Ring`]: ../../komorebi/src/ring.rs
[`Monitor`]: ../../komorebi/src/monitor.rs
[`Workspace`]: ../../komorebi/src/workspace.rs
[`Workspace::update`]: ../../komorebi/src/workspace.rs
[`WorkspaceGlobals`]: ../../komorebi/src/workspace.rs
[`Container`]: ../../komorebi/src/container.rs
[`Window`]: ../../komorebi/src/window.rs
[`Window::should_manage`]: ../../komorebi/src/window.rs
[`Window::set_position`]: ../../komorebi/src/window.rs
[`Workspace::new_container_for_window`]: ../../komorebi/src/workspace.rs
[`WindowManager`]: ../../komorebi/src/window_manager.rs
[`WindowManager::process_event`]: ../../komorebi/src/process_event.rs
[`WindowManager::update_focused_workspace`]: ../../komorebi/src/window_manager.rs
[`WindowManagerEvent`]: ../../komorebi/src/window_manager_event.rs
