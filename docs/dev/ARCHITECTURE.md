# Architecture

komorebi is a tiling window manager. It decides where every window goes and asks a
compositor to put it there. Everything it needs from the compositor is behind one trait,
[`Backend`](../../komorebi/src/backend/mod.rs), so the window manager is written, and
tested, without one.

This page describes that trait, who implements and calls it, where each of its rules is
enforced, the model of the desktop the window manager tiles, the backend that reaches KWin,
and how an event about a window becomes a tile. It grows with the code: each part of the
daemon that is added has its place here.

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
- **Bridge**: the script komorebi loads into KWin ([`bridge.js`]). It reports what KWin
  tells it and carries out what the daemon asks.
- **Table**: the KWin backend's model of the desktop ([`Table`]), which the bridge's events
  keep up to date and the queries are answered from.
- **Held call**: the `Poll` call the bridge always has open, which the daemon answers when it
  has commands for the bridge, or after 10 seconds with none.

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

## The KWin backend

[`KWinBackend`] is the backend of a real Plasma session. KWin has no protocol that lets
another process read and set window geometry, so the backend is in two processes:

- **The bridge**, [`bridge.js`], a script KWin runs inside itself. It subscribes to KWin's
  signals and sends each as a JSON `Event` call over the session bus, and it carries out the
  commands it gets as the answer to its held `Poll` call. When it starts it sends a snapshot
  of the desktop. It holds no window manager logic.
- **The daemon's side**, [`kwin`], owns `org.komorebi.Bridge` on the session bus, with one
  object for `Event` and one for `Poll`. `Event` calls are handled one at a time, in the order
  they arrive, and never wait. A `Poll` call is held until there are commands, so that the
  bridge gets them as soon as they are queued, without a timer and without a call from the
  daemon into KWin.

Every decision about what an event means is made in the daemon, by the table
([`kwin::state`]): an event goes in, the table is updated, and what follows comes out, a
window event for the window manager or the answer to a query. The table is pure (no D-Bus,
no threads), so the unit tests drive it with the JSON the bridge sends.

The bridge is JavaScript, which no Rust build compiles, and KWin loads the file as it is, so
it is checked without a build step. `tsc` checks it as strict TypeScript through its JSDoc
comments, which type every message, against [`kwin-api.d.ts`], the part of KWin's scripting
API the bridge uses. [`bridge.test.js`] runs it with `node --test` against a fake of that API.
The two sides share the messages of a session in [`fixtures`]: the bridge's tests check that
it sends exactly those events, and answer it with those commands, and the Rust tests feed
the events to the table and check that the backend sends those commands. A change to a
message on one side fails a test on the other.

The path of an event: a KWin signal, the bridge's handler, `Event(json)` on the session bus,
[`Table::handle`], a [`WindowManagerEvent`] on the channel of
[`window_manager_event_listener`], the thread that [`listen_for_events`] starts, and
[`WindowManager::process_event`]. The queries (`windows`, `window`, `foreground_window`,
`monitors`) are answered from the table. `cursor_pos` is the exception: only the bridge can
read the pointer, so it is asked through the held call, and given 250 ms to answer before
the table's last known position is returned.

A command goes the other way. The backend looks up the UUID KWin knows the window by,
queues a JSON command for the bridge, and returns. The held `Poll` call is answered with the
queue, as one batch: the daemon waits a millisecond after the first command for the rest, so
that a retile of every window of a workspace reaches KWin at once. The bridge carries the
commands out in order and reports the ones it could not carry out (a window that closed in
the meantime) as a `failed` event, which is logged. `set_rect` also records the frame it
asked for in the table, so that the window manager sees it straight away; the `geometry`
event that follows says what the client made of it. The pointer cannot be moved from a
script: when it follows the focus, the backend invokes KWin's "Move Mouse to Focus" shortcut,
which centers it on the window KWin has focused. When komorebi stops, the backend asks KWin to
unload the bridge.

[`connect`] makes the backend: it connects to the session bus, loads the bridge into KWin,
and returns once the snapshot has arrived, so that the window manager reads a full table when
it starts. [PLATFORM.md](PLATFORM.md) has the D-Bus names, the files and the environment
variables; [DESIGN.md](DESIGN.md#why-does-komorebi-run-a-script-inside-kwin) has the reasons.

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
- [`window_manager_event_listener`](../../komorebi/src/window_manager_event_listener.rs): the
  channel that carries events from the backend to the window manager, and
  [`listen_for_events`], the thread that takes them off it.
- [`kwin`](../../komorebi/src/kwin/mod.rs): the KWin backend, its D-Bus service and the held
  call; [`kwin::state`](../../komorebi/src/kwin/state.rs), the table; and
  [`bridge.js`](../../komorebi/src/kwin/bridge.js), the bridge.
- [`main.rs`](../../komorebi/src/main.rs): the daemon. It connects to KWin, or installs the
  fake with `--fake-backend`, loads and tiles the desktop, starts the thread that handles
  events, and waits to be stopped.

The handle is a process-wide singleton so that a window can be a small `Copy` value
that carries only its id, and ask the backend for what it needs. A test replaces it for
its own thread with [`with_backend`], which puts the previous one back when the test is
done, also if it panics.

## Traits and roles

**[`Backend`] is the compositor.** It is implemented once for each compositor and once for
the fake. The window manager is its only caller, through [`backend`]. The contract:

- Queries (`windows`, `window`, `foreground_window`, `cursor_pos`) answer from the
  backend's own table and never wait for the compositor, except the KWin backend's
  `cursor_pos`, which waits up to 250 ms for the bridge. `monitors` may fail.
- A command returns once the compositor has accepted it, not once the client has acted on
  it. The frame a window ends up with is what `window_rect` says afterwards, because a
  client may refuse a size.
- A command about a window that does not exist, or is no longer alive, is an error. It
  never panics. The KWin backend knows a closed window for a minute after it closed, and
  sends its commands to the bridge, which reports that it could not carry them out.
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
| A command about a missing window is an error, not a panic | test | `a_command_about_a_window_nobody_opened_is_an_error` and `a_command_about_a_closed_window_is_an_error` against the fake, `a_command_about_a_window_the_table_does_not_know_is_an_error_and_sends_nothing` against the KWin backend | none |
| A locked container keeps its index | framework path: a [`Workspace`] inserts, removes and swaps its containers through `LockableSequence` | `test_locked_containers_with_new_window` and `test_locked_containers_remove_window` | `containers_mut()` is the `VecDeque` itself |
| A window komorebi does not manage is never moved | framework path: `handle_event` asks [`Window::should_manage`] before it acts on any event but a destroy | `dialogs_and_other_non_normal_windows_are_left_alone` | none |
| A destroyed window leaves whichever workspace has it | framework path: the destroy is looked up in the index of managed windows, which is rebuilt at startup and before and after every event | `a_window_that_was_open_before_komorebi_started_is_removed_when_it_closes` | none |
| The daemon does not start without KWin, unless it is told to run against the fake | startup check: [`connect`] fails, and the daemon exits with the error, code 1 | `without_a_session_bus_komorebi_exits_with_an_error_naming_it` | `--fake-backend` |
| One komorebi per session | startup check: the session bus gives `org.komorebi.Bridge` to one connection | none | none |
| Events reach the window manager in the order KWin raised them | framework path: the `Event` interface is `spawn = false`, so the calls are handled inline, and one channel and one thread carry the events to the window manager | `vm-kwin-events` ("the Show came first") | none |
| No event names a window before the event that announces it | convention in the bridge, which announces a window it has not seen when it is activated; the table shows a window once | `a_new_window_raises_show_and_then_focus_in_that_order`, `announcing_the_same_window_twice_shows_it_once`, `vm-kwin-events` | none |
| A window's id names one window while the daemon runs | framework path: the table numbers each KWin window the first time it sees it, and never gives a number out twice | `ids_are_assigned_once_per_window_and_survive_a_new_snapshot`, `tombstones_are_dropped_after_a_while_and_their_ids_are_not_reused` | none |
| A backend never waits for the window manager | framework path: an event that does not fit in the channel (20 waiting) is dropped and logged | none | none |
| Every window is where komorebi asked for it, within the size its client keeps to | VM test | `vm-core-e2e`, `vm-client-geometry` | a client may refuse a size: the frame is what `window_rect` says |
| A retile reaches KWin as one batch | framework path: the answer to `Poll` waits a millisecond after the first command for the others | `vm-latency` | none |
| The window manager sees a frame it asked for at once | framework path: the KWin backend's `set_rect` records it in the table before KWin answers | `a_requested_rect_is_visible_at_once_and_replaced_by_the_real_one` | none |
| The bridge holds no window manager logic | convention, with every decision about an event in the table, which the `kwin::state` unit tests pin | the unit tests in `komorebi/src/kwin/state.rs` | none |
| What the bridge sends is what the table reads | type checker: the JSDoc types of the messages in [`bridge.js`]; tests: the events of [`fixtures`] are what the bridge's tests check it sends and what the table's tests feed it | `what the bridge sends for a session is what the daemon's tests read`, `the_bridges_events_for_a_session_are_the_window_events_komorebi_expects` | none |
| What the backend sends is what the bridge carries out | tests: the commands of [`fixtures`] are what the backend's tests check it sends and what the bridge's tests answer it with, checking what KWin is asked to do for each | `the_commands_of_a_session_go_to_the_bridge_as_the_bridges_tests_answer_it`, `what the bridge sends for a session is what the daemon's tests read`, `a command for a window KWin no longer has is given back, and the rest are carried out` | none |
| Everything the bridge sends over D-Bus is one string | type checker: [`kwin-api.d.ts`] gives `callDBus` one string argument | `tsc`, `every call goes to the daemon's names, with one JSON string` | none |
| The bridge uses nothing a KWin script does not have | type checker: the library is ECMAScript 2016 alone, and KWin's API is what [`kwin-api.d.ts`] declares | `tsc` | a declaration KWin does not match: the VM tests run the bridge in KWin |
| Queries never block on the compositor | convention only | none | the KWin backend's `cursor_pos` waits up to 250 ms |
| Only the backend module reaches the compositor | convention only | none | none |

The rows enforced by convention are rules nothing checks. A real backend that waits inside a
query breaks one of them, a module that talks to D-Bus directly breaks another, and a bridge
that decides something in JavaScript breaks a third: the bridge's tests check what it sends,
not that it decides nothing.

## One event, end to end

A window is shown, and ends up on a tile:

1. KWin raises `windowAdded`. The bridge sends an `added` event with what KWin says about
   the window, and [`Table::handle`] gives the window an id, records it, and returns a
   [`WindowManagerEvent`]`::Show` carrying the window and the compositor signal it came
   from. The event goes on the channel, and the event thread calls
   [`WindowManager::process_event`], which hands it to `handle_event` and logs how long that
   took. The end-to-end tests build the event themselves and call `process_event`.
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
   what `window_rect` says afterwards. The KWin backend queues a `geo` command for the
   bridge, the bridge sets the window's frame, the client draws at the size it accepts, and
   KWin reports the frame it ended up with as a `geometry` event, which updates the table.
7. If the new container is the only one, the window is given the focus with the backend's
   `focus`, and the pointer follows it (on KWin, through its "Move Mouse to Focus"
   shortcut).
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
  does, and the thread that handles them.
- [`komorebi/src/kwin/mod.rs`](../../komorebi/src/kwin/mod.rs): the KWin backend, its D-Bus
  service and the held call.
- [`komorebi/src/kwin/state.rs`](../../komorebi/src/kwin/state.rs): the table, with the
  shapes of the bridge's events, and its tests.
- [`komorebi/src/kwin/bridge.js`](../../komorebi/src/kwin/bridge.js): the bridge, with the
  types of its messages; [`kwin-api.d.ts`] and
  [`tsconfig.json`](../../komorebi/src/kwin/tsconfig.json), what `tsc` checks it against;
  [`bridge.test.js`], its tests; [`fixtures`], the messages both sides' tests read.
- [`nix/tests/vm/suites/kwin-events`](../../nix/tests/vm/suites/kwin-events/test.py): the
  events of a real KWin, through the daemon.
- [`nix/tests/vm/suites/core-e2e`](../../nix/tests/vm/suites/core-e2e/test.py): the daemon
  tiling a real KWin, checked against what KWin has.
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
[`window_manager_event_listener`]: ../../komorebi/src/window_manager_event_listener.rs
[`listen_for_events`]: ../../komorebi/src/process_event.rs
[`KWinBackend`]: ../../komorebi/src/kwin/mod.rs
[`connect`]: ../../komorebi/src/kwin/mod.rs
[`kwin`]: ../../komorebi/src/kwin/mod.rs
[`kwin::state`]: ../../komorebi/src/kwin/state.rs
[`Table`]: ../../komorebi/src/kwin/state.rs
[`Table::handle`]: ../../komorebi/src/kwin/state.rs
[`bridge.js`]: ../../komorebi/src/kwin/bridge.js
[`kwin-api.d.ts`]: ../../komorebi/src/kwin/kwin-api.d.ts
[`bridge.test.js`]: ../../komorebi/src/kwin/bridge.test.js
[`fixtures`]: ../../komorebi/src/kwin/fixtures
