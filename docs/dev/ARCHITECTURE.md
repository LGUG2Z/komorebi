# Architecture

komorebi is a tiling window manager. It decides where every window goes and asks a
compositor to put it there. Everything it needs from the compositor is behind one trait,
[`Backend`](../../komorebi/src/backend/mod.rs), so the window manager is written, and
tested, without one.

This page describes that trait, who implements and calls it, where each of its rules is
enforced, the model of the desktop the window manager tiles, the backend that reaches KWin,
how an event about a window becomes a tile, how a message on the daemon's socket is
answered, how subscribers are notified of both, what the orders a message gives do, and how the
NixOS module runs the daemon. It grows with the code: each part of the daemon that is added has
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
- **Message**: what a client sends the daemon on its socket, a [`SocketMessage`]: an order
  (focus a window, move it, retile, stop) or a question (the state, the monitors, one [`StateQuery`]).
- **Direction**: where an order goes from the focused container: a side of its tile
  ([`OperationDirection`]), or the next or the previous container of the workspace
  ([`CycleDirection`]).
- **State**: the window manager's monitors, workspaces, containers and windows, and its
  settings, as the daemon answers a `State` message with them ([`State`]).
- **Subscriber**: a program that listens on a socket of its own in [`DATA_DIR`] and has
  asked the daemon, by the name of that socket, to send it notifications.
- **Notification**: what a subscriber is sent after each event and each message the window
  manager handles: the event or the message, and the state after it ([`Notification`]).
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
- **Module**: `services.komorebi`, the NixOS module ([`nixos-module.nix`]), which installs
  komorebi and runs the daemon in every Plasma session as the systemd user unit
  `komorebi.service`.

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

KWin can restart under a running daemon, on the same session bus. The thread that serves
the connection watches the owner of `org.kde.KWin` on the bus. When KWin leaves it, the
bridge has gone with it and its windows too, and nothing will report them closing:
[`Table::compositor_lost`] marks every window closed and returns a `Destroy` for each, so the
window manager takes them out of its layout. When KWin is back, the daemon loads the bridge
into it again, the bridge sends a snapshot of the new KWin's desktop, and its events are
handled as before. The `Poll` call the old bridge held is still open in the daemon, since
zbus does not end a call when its caller leaves the bus: it is answered with nothing as soon
as it wakes, and the commands go to the new bridge's call.

[`connect`] makes the backend: it connects to the session bus, loads the bridge into KWin,
and returns once the snapshot has arrived, so that the window manager reads a full table when
it starts. [PLATFORM.md](PLATFORM.md) has the D-Bus names, the files and the environment
variables; [DESIGN.md](DESIGN.md#why-does-komorebi-run-a-script-inside-kwin) has the reasons.

## The command socket

The daemon takes messages on a Unix socket, `komorebi.sock` in [`DATA_DIR`]
(`$XDG_RUNTIME_DIR/komorebi`, or the local data directory without a runtime directory).
[`WindowManager::new`] binds it, after it removes a socket file that a daemon that did not
stop cleanly left behind, and the window manager keeps the listener. The daemon connects to
KWin before it makes the window manager, so a komorebi that cannot connect, such as a second
one in the same session, whose bus refuses it `org.komorebi.Bridge`, exits before it touches
the socket of the one that is running.

[`listen_for_commands`] starts a thread that accepts connections, and reads each connection
on a separate thread ([`read_commands_uds`]). A connection carries one [`SocketMessage`]
per line, as JSON with the variant's name in `type` and its arguments in `content`:
`{"type":"Query","content":"FocusedWorkspaceIndex"}`. Each message is handled by
[`WindowManager::process_command`] with the window manager locked, so messages are handled
one at a time. A message that waited a second for the lock is logged and dropped.

A question is answered on the same connection: the [`State`] or the monitors as JSON, or
the answer to a [`StateQuery`] as one value in plain text. The answer has no framing; it
ends when the daemon closes the connection, which it does once it has read the last line.
A client therefore shuts down its side for writing after its message, and reads until the
end ([`send_query`]). An order is answered with nothing, and a client that sends only
orders can close the connection as soon as it has written them ([`send_message`],
[`send_batch`]).

A message that fails (a query about the focused container when the workspace has none) is
logged, and the lines after it on that connection are not read. The client sees the
connection close with no answer.

`Stop` ends the daemon: [`WindowManager::stop`] removes the socket, lets the backend give up
what it set up in the compositor (the KWin backend unloads the bridge), and exits with
status 0. `SIGINT` and `SIGTERM` do the same from `main.rs`, with the exit status 130.

## Notifications

A subscriber binds a socket in [`DATA_DIR`] and sends `AddSubscriberSocket` with its name,
or `AddSubscriberSocketWithOptions` with its name and [`SubscribeOptions`]
([`subscribe`], [`subscribe_with_options`], `komorebic subscribe-socket`).
[`WindowManager::process_command`] records it in [`SUBSCRIPTION_SOCKETS`], and its options in
[`SUBSCRIPTION_SOCKET_OPTIONS`]. `RemoveSubscriberSocket` takes it out again. The
subscribers are kept in memory only, and a daemon that restarts has none.

At the end of [`WindowManager::process_event`] and of [`WindowManager::process_command`],
the window manager calls [`notify_subscribers`] with a [`Notification`]: the event or the
message ([`NotificationEvent`]), and the [`State`] after it. A [`State`] made before the event
or the message is compared with the window manager after it ([`State::has_been_modified`]);
the monitors are compared down to the windows, and a window by its id only. An event about a
window komorebi does not manage, and a message that fails, return before this and are not
notified.

[`notify_subscribers`] serializes the notification once, and for each subscriber connects to
its socket, writes the JSON and closes the connection, all on the thread that handled the
event or the message, with the window manager still locked. A subscriber with
`filter_state_changes` is skipped when the state did not change, unless the notification is
about a window being shown or a subscriber being added. A subscriber that cannot be
connected to is removed, with its socket file, after the loop. Writing to a subscriber that
was connected to has no timeout, and a failed write returns an error from the call that
handled the event or the message. A subscriber that stops reading therefore holds the window
manager, and with it the thread that handles events: the backend's events wait in the
channel of [`window_manager_event_listener`], and past 20 the backend drops them.

When the daemon stops ([`WindowManager::stop`], or `SIGINT` and `SIGTERM` in `main.rs`), it
connects to each subscriber and shuts the connection down without writing anything.

## Commands

A message that gives an order is a call to a method of the [`WindowManager`] from
[`WindowManager::process_command`]. An order that goes somewhere takes one of two kinds of
direction, both from `komorebi-layouts`:

- An [`OperationDirection`] (left, right, up, down) is a side of the focused container's tile.
  The workspace asks its layout which container is on that side
  ([`Workspace::new_idx_for_direction`]). A layout answers from the index of the focused
  container and the number of containers, not from where the windows are.
- A [`CycleDirection`] (next, previous) follows the order of the workspace's containers, and
  wraps around at both ends ([`Workspace::new_idx_for_cycle_direction`]).

`FocusWindow` and `CycleFocusWindow` focus another container of the focused workspace and give
its window the focus with the backend's `focus` ([`WindowManager::focus_container_in_direction`],
[`WindowManager::focus_container_in_cycle_direction`]). They retile nothing. When there is no
container in that direction, the message fails and changes nothing. Whether the pointer follows
the focus is [`WindowManager::mouse_follows_focus`], which `MouseFollowsFocus` and
`ToggleMouseFollowsFocus` set.

`MoveWindow` and `CycleMoveWindow` swap the focused container with the container in that
direction ([`WindowManager::move_container_in_direction`],
[`WindowManager::move_container_in_cycle_direction`]), and tile the workspace again: the window
that moved takes the other's tile, keeps the focus, and the pointer follows it. The swap goes
through `LockableSequence`, so a lock stays with its index: the container swapped onto a locked
index is locked in its place. When there is no container in that direction, the message fails
and nothing moves.

## The NixOS module

The flake exports one module under two names, `nixosModules.default` and
`nixosModules.komorebi`: [`nixos-module.nix`] and the flake's overlay, which builds
`komorebi-full` (both programs) with the flake's nixpkgs and Rust toolchain. Its options are
`enable`, `package` (by default `pkgs.komorebi-full`, from the overlay), `logLevel`, and
`execStart`, which is read-only: the module sets it to `bin/komorebi` of the package with
`--log-level`. With `services.komorebi.enable`, the module:

- installs the package in the system profile;
- defines the user unit `komorebi.service`, which every user of the machine has. Plasma's
  `plasma-workspace.target` wants it, so a Plasma session starts it at login, and it is ordered
  after `plasma-core.target`, which Plasma orders after KWin's service, `plasma-kwin_wayland`;
- runs `execStart` in it, with `Restart=on-failure`, the exit status of a `SIGTERM` (130)
  counted as success, and a `PATH` of `/run/wrappers/bin`, `/etc/profiles/per-user/%u/bin` and
  `/run/current-system/sw/bin` instead of the store paths NixOS gives a unit;
- asserts that Plasma 6 is enabled.

The VM tests run on machines built with it ([vm-tests.md](vm-tests.md)). Their headless node
runs the daemon in test sessions, with a template unit, `komorebi@NAME`, that runs the module's
`execStart` on the session's private bus. The session node logs in to Plasma, which starts the
module's unit.

| Rule | Enforced by | Proof | Escape hatch |
|---|---|---|---|
| komorebi is enabled only on a machine with Plasma 6 | evaluation error: an assertion of the module | `nixos-module-eval` ("without Plasma 6, an assertion fails") | none |
| The log level is one the daemon accepts | evaluation error: `logLevel` is an enum of the daemon's `--log-level` values | `nixos-module-eval` ("a log level the daemon does not have does not evaluate") | none |
| The unit runs `execStart`, and nothing but the module sets it | evaluation error: the option is read-only | `nixos-module-eval` ("execStart cannot be set", "the unit runs execStart") | a unit of your own |
| `komorebi` and `komorebic` are on every user's `PATH` | the module installs the package in the system profile | `nixos-module-eval`; every VM suite runs `komorebic` | none |
| Every Plasma session starts komorebi, once KWin's service has started | systemd: the user unit is wanted by `plasma-workspace.target`, after `plasma-core.target` | `nixos-module-eval`, `vm-module` | `systemctl --user mask komorebi` |
| A daemon that fails or is killed is started again | systemd: `Restart=on-failure` | `vm-module` (`SIGKILL`) | none |
| A daemon that is told to stop stays stopped, and the unit is not failed | systemd: the exit statuses of `Stop` (0) and of `SIGTERM` (130) are successes | `vm-module` | none |
| The daemon has the `PATH` of a terminal in the session | the unit's environment, with NixOS's default `PATH` turned off | `nixos-module-eval`, `vm-module` | none |
| The VM suites run komorebi as the module installs it | framework path: the test nodes import the module, and `komorebi@NAME` runs its `execStart` | every VM suite | a suite's `node` can set `services.komorebi.enable = false` (`vm-render-smoke` does) |

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
  window out, move the focus, move the focused container).
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
- [`process_command`](../../komorebi/src/process_command.rs): the thread that listens on the
  socket, and what the window manager does with each message.
- [`state`](../../komorebi/src/state.rs): the [`State`] a `State` message is answered with,
  and whether an event or a message changed it.
- [`lib.rs`](../../komorebi/src/lib.rs): the subscribers, the [`Notification`], and
  [`notify_subscribers`].
- [`komorebi-client`](../../komorebi-client/src/lib.rs): a library that writes messages on
  the socket and reads the answers, and subscribes to notifications, for programs that talk
  to the daemon.
- [`komorebic`](../../komorebic/src/main.rs): the command line. A subcommand that asks
  something of the daemon is one message, sent with `komorebi-client`; `start` runs the
  daemon, `log` follows its log file in [`LOG_DIR`], and the hidden `docgen` writes
  `docs/cli`.
- [`main.rs`](../../komorebi/src/main.rs): the daemon. It connects to KWin, or installs the
  fake with `--fake-backend`, makes the window manager, which binds the socket, loads and
  tiles the desktop, starts the threads that listen on the socket and handle events, and
  waits to be stopped.
- [`nixos-module.nix`]: the NixOS module, which installs komorebi and runs the daemon as a
  systemd user unit of every Plasma session.

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
| A komorebi that cannot connect to KWin leaves the socket of the running one alone | startup order: `main.rs` connects to KWin before [`WindowManager::new`] binds the socket | `without_kwin_komorebi_leaves_the_socket_it_finds_alone` | `--fake-backend` asks no bus, and takes the socket |
| Events reach the window manager in the order KWin raised them | framework path: the `Event` interface is `spawn = false`, so the calls are handled inline, and one channel and one thread carry the events to the window manager | `vm-kwin-events` ("the Show came first") | none |
| No event names a window before the event that announces it | convention in the bridge, which announces a window it has not seen when it is activated; the table shows a window once | `a_new_window_raises_show_and_then_focus_in_that_order`, `announcing_the_same_window_twice_shows_it_once`, `vm-kwin-events` | none |
| A window's id names one window while the daemon runs | framework path: the table numbers each KWin window the first time it sees it, and never gives a number out twice | `ids_are_assigned_once_per_window_and_survive_a_new_snapshot`, `tombstones_are_dropped_after_a_while_and_their_ids_are_not_reused` | none |
| A backend never waits for the window manager | framework path: an event that does not fit in the channel (20 waiting) is dropped and logged | none | none |
| Every window is where komorebi asked for it, within the size its client keeps to | VM test | `vm-core-e2e`, `vm-client-geometry` | a client may refuse a size: the frame is what `window_rect` says |
| A retile reaches KWin as one batch | framework path: the answer to `Poll` waits a millisecond after the first command for the others | `vm-latency` | none |
| The window manager sees a frame it asked for at once | framework path: the KWin backend's `set_rect` records it in the table before KWin answers | `a_requested_rect_is_visible_at_once_and_replaced_by_the_real_one` | none |
| A KWin that restarts gets the bridge again, and the windows of the old one leave the layout | framework path: the connection's thread watches `NameOwnerChanged` for `org.kde.KWin`, and the table destroys every window when the name is lost | `when_kwin_goes_away_every_window_it_had_is_destroyed`, `vm-kwin-restart` | none |
| After KWin restarts, the commands go to the new bridge | framework path: a `Poll` held since before KWin last left the bus is answered with nothing, and passes on the wake-up it got | `a_poll_held_by_the_bridge_of_a_kwin_that_has_gone_gets_no_commands`, `vm-kwin-restart` ("a window opened in the third KWin is tiled") | none |
| Messages are handled one at a time, and in order on one connection | framework path: [`read_commands_uds`] reads a connection line by line and handles each message with the window manager locked | `the_messages_on_one_connection_are_handled_in_order` | none |
| A socket left behind by a daemon that did not stop cleanly does not keep the next one from starting | framework path: [`WindowManager::new`] removes the file before it binds | `a_socket_file_left_behind_is_replaced` | none |
| A daemon that is told to stop leaves no socket behind | framework path: [`WindowManager::stop`] removes it before it exits | `the_daemon_answers_on_its_socket_and_stops_when_it_is_told_to` | `SIGKILL` |
| A container moved onto a locked index is locked in its place, and the lock leaves the container that moved away | framework path: [`Workspace::swap_containers`] swaps through `LockableSequence`, which keeps the lock with the index | `test_swap_respecting_locks_various_cases`, `a_lock_stays_with_its_index_when_a_window_is_moved_onto_it` | `containers_mut()` is the `VecDeque` itself |
| Moving the focus moves no window | test | `focus_gives_the_focus_to_the_window_on_that_side_and_moves_nothing`, `vm-core-e2e` ("moving the focus moved no window") | none |
| A subscriber that is gone is dropped, not retried | test | `a_subscriber_that_is_gone_is_dropped_and_not_tried_again` | none |
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
9. [`notify_subscribers`] sends every subscriber a [`Notification`] of the `Show`, with the
   state that now has the window. A `Show` is sent to a subscriber with
   `filter_state_changes` too, whether or not it changed the state.

## One message, end to end

`komorebic state` asks for the state of the window manager:

1. `komorebic` calls [`send_query`], which connects to `komorebi.sock` in [`DATA_DIR`],
   writes `{"type":"State"}`, and shuts down its side of the connection for writing.
2. The thread that [`listen_for_commands`] started accepts the connection and starts a
   thread for it, which calls [`read_commands_uds`].
3. `read_commands_uds` reads the line, parses it into [`SocketMessage`]`::State`, and locks
   the window manager, waiting up to a second.
4. [`WindowManager::process_command`] makes a [`State`] from the window manager, a copy of
   every monitor with its workspaces, containers and windows. Each window is serialized
   with what the backend reports about it now: its frame, title, executable, class and path.
5. The state is written to the connection as pretty-printed JSON, and the index of managed
   windows is rebuilt, as after every message. The subscribers are sent a notification of
   the `State` message, except those with `filter_state_changes`: it changed nothing.
6. There are no more lines, so `read_commands_uds` returns and the connection is closed.
   `send_query` has read to the end, and returns the JSON, which `komorebic` prints.

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
- [`komorebi/src/process_command.rs`](../../komorebi/src/process_command.rs): the socket, what
  each message does, and the tests that send messages through a real socket.
- [`komorebi-client/src/lib.rs`](../../komorebi-client/src/lib.rs): the client side of the
  socket.
- [`komorebi-client/tests/subscribe.rs`](../../komorebi-client/tests/subscribe.rs):
  subscribers and the notifications they are sent, through real sockets.
- [`komorebic/src/main.rs`](../../komorebic/src/main.rs): the subcommands, and
  [`komorebic/tests/cli.rs`](../../komorebic/tests/cli.rs), what each writes on the socket.
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
  tiling a real KWin, checked against what KWin has, and read and stopped with `komorebic`.
- [`nix/tests/vm/suites/kwin-restart`](../../nix/tests/vm/suites/kwin-restart/test.py): KWin
  restarting under the daemon.
- [`komorebi/tests/wm_events.rs`](../../komorebi/tests/wm_events.rs): the window manager
  driven end to end against the fake.
- [`komorebi/tests/socket.rs`](../../komorebi/tests/socket.rs): the daemon over its socket,
  from its start to a `Stop`.
- [`komorebi/tests/focus.rs`](../../komorebi/tests/focus.rs) and
  [`komorebi/tests/moving.rs`](../../komorebi/tests/moving.rs): the focus and move commands
  against the fake.
- [`nix/nixos-module.nix`](../../nix/nixos-module.nix): the NixOS module;
  [`nix/tests/nixos-module.nix`](../../nix/tests/nixos-module.nix), its scenarios, evaluated in
  real NixOS systems; and [`nix/tests/vm/suites/module`](../../nix/tests/vm/suites/module/test.py),
  a Plasma session in which the module's unit runs the daemon.

[`nixos-module.nix`]: ../../nix/nixos-module.nix
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
[`WindowManager::focus_container_in_direction`]: ../../komorebi/src/window_manager.rs
[`WindowManager::focus_container_in_cycle_direction`]: ../../komorebi/src/window_manager.rs
[`WindowManager::mouse_follows_focus`]: ../../komorebi/src/window_manager.rs
[`WindowManager::move_container_in_direction`]: ../../komorebi/src/window_manager.rs
[`WindowManager::move_container_in_cycle_direction`]: ../../komorebi/src/window_manager.rs
[`Workspace::swap_containers`]: ../../komorebi/src/workspace.rs
[`Workspace::new_idx_for_direction`]: ../../komorebi/src/workspace.rs
[`Workspace::new_idx_for_cycle_direction`]: ../../komorebi/src/workspace.rs
[`OperationDirection`]: ../../komorebi/src/core/mod.rs
[`CycleDirection`]: ../../komorebi/src/core/mod.rs
[`WindowManagerEvent`]: ../../komorebi/src/window_manager_event.rs
[`window_manager_event_listener`]: ../../komorebi/src/window_manager_event_listener.rs
[`listen_for_events`]: ../../komorebi/src/process_event.rs
[`KWinBackend`]: ../../komorebi/src/kwin/mod.rs
[`connect`]: ../../komorebi/src/kwin/mod.rs
[`kwin`]: ../../komorebi/src/kwin/mod.rs
[`kwin::state`]: ../../komorebi/src/kwin/state.rs
[`Table`]: ../../komorebi/src/kwin/state.rs
[`Table::handle`]: ../../komorebi/src/kwin/state.rs
[`Table::compositor_lost`]: ../../komorebi/src/kwin/state.rs
[`bridge.js`]: ../../komorebi/src/kwin/bridge.js
[`kwin-api.d.ts`]: ../../komorebi/src/kwin/kwin-api.d.ts
[`bridge.test.js`]: ../../komorebi/src/kwin/bridge.test.js
[`fixtures`]: ../../komorebi/src/kwin/fixtures
[`DATA_DIR`]: ../../komorebi/src/lib.rs
[`LOG_DIR`]: ../../komorebi/src/lib.rs
[`SocketMessage`]: ../../komorebi/src/core/mod.rs
[`StateQuery`]: ../../komorebi/src/core/mod.rs
[`State`]: ../../komorebi/src/state.rs
[`State::has_been_modified`]: ../../komorebi/src/state.rs
[`SubscribeOptions`]: ../../komorebi/src/core/mod.rs
[`Notification`]: ../../komorebi/src/lib.rs
[`NotificationEvent`]: ../../komorebi/src/lib.rs
[`notify_subscribers`]: ../../komorebi/src/lib.rs
[`SUBSCRIPTION_SOCKETS`]: ../../komorebi/src/lib.rs
[`SUBSCRIPTION_SOCKET_OPTIONS`]: ../../komorebi/src/lib.rs
[`WindowManager::new`]: ../../komorebi/src/window_manager.rs
[`WindowManager::stop`]: ../../komorebi/src/window_manager.rs
[`WindowManager::process_command`]: ../../komorebi/src/process_command.rs
[`listen_for_commands`]: ../../komorebi/src/process_command.rs
[`read_commands_uds`]: ../../komorebi/src/process_command.rs
[`send_query`]: ../../komorebi-client/src/lib.rs
[`send_message`]: ../../komorebi-client/src/lib.rs
[`send_batch`]: ../../komorebi-client/src/lib.rs
[`subscribe`]: ../../komorebi-client/src/lib.rs
[`subscribe_with_options`]: ../../komorebi-client/src/lib.rs
