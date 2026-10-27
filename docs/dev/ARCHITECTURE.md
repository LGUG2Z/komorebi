# Architecture

komorebi is a tiling window manager. It decides where every window goes and asks a
compositor to put it there. Everything it needs from the compositor is behind one trait,
[`Backend`](../../komorebi/src/backend/mod.rs), so the window manager is written, and
tested, without one.

This page describes that trait, who implements and calls it, where each of its rules is
enforced, and the model of the desktop the window manager tiles. It grows with the code:
each part of the daemon that is added has its place here.

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
| Queries never block on the compositor | convention only | none | none |
| Only the backend module reaches the compositor | convention only | none | none |

The last two rows are rules nothing checks. A real backend that waits inside a query
breaks the first, and a module that talks to D-Bus directly breaks the second.

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
