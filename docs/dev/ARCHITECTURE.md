# Architecture

komorebi is a tiling window manager. It decides where every window goes and asks a
compositor to put it there. Everything it needs from the compositor is behind one trait,
[`Backend`](../../komorebi/src/backend/mod.rs), so the window manager is written, and
tested, without one.

This page describes that trait, who implements and calls it, where each of its rules is
enforced, the model of the desktop the window manager tiles, the backend that reaches KWin,
how an event about a window becomes a tile, how workspaces are the compositor's desktops, how
a stack hides its windows, how a message on the daemon's socket is answered, how subscribers
are notified of both, what the orders a message gives do, how `komorebi.json` configures the
daemon, and how the NixOS module runs the daemon. It grows with the code: each part of the
daemon that is added has its place here.

## Vocabulary

- **Window**: a top-level surface a client has asked the compositor for, named by the
  compositor's `u32` id.
- **Monitor**: a connected output, with its bounds and its work area (its bounds minus
  the panels).
- **Workspace**: the containers a monitor shows, in order, and the layout that tiles
  them. A monitor has a ring of workspaces and shows one of them. Workspace `i` is the
  compositor's desktop `i` on that monitor. A workspace can have a name, which the named
  commands find it by.
- **Desktop**: one of KWin's virtual desktops. KWin shows the windows of one desktop on each
  output and hides the others; with its per-output virtual desktops on, each output shows a
  desktop of its own.
- **Container**: what a layout places. It gets one tile, and every window in it is put on
  that tile.
- **Stack**: a container with more than one window. It has one tile, and the window in front
  (the container's focused window) is in sight; the others are hidden.
- **Hidden window**: a window of a stack that is not in front, or of a container a monocle
  leaves out, which komorebi has taken out of sight ([`Backend::hide`]) and recorded in
  [`HIDDEN_WINDOWS`]. It is alive, not minimized, and on its workspace.
- **Parking desktop**: the virtual desktop, named `komorebi-park`, that the KWin backend hides
  windows on. It is not a workspace.
- **Layout**: the arrangement (BSP, columns, rows, ...) that turns a work area and a
  number of containers into one rectangle, a **tile**, for each container.
- **Monocle**: a workspace's one container that has the whole work area. It is held out of the
  workspace's containers, so the layout gives no container a tile, and the windows of the
  containers are hidden, until it is put back where it was ([`Workspace::monocle_container`]).
- **Layer**: which of a workspace's two kinds of windows the focus, move and resize commands
  act on: its tiles (`Tiling`) or its floating windows (`Floating`) ([`WorkspaceLayer`]).
- **Maximized window**: a workspace's one window that the compositor has maximized. It is in no
  container, so it has left the layout, until it is put back in a container of its own where it
  was ([`Workspace::maximized_window`]).
- **Event**: what the compositor reports about a window: it was shown, destroyed, given
  the focus, minimized or restored, or put on another workspace's desktop; and that a monitor
  shows another workspace ([`WindowManagerEvent`]). Two events come from komorebi itself: a
  command asks it to manage the focused window, or to stop managing it.
- **Message**: what a client sends the daemon on its socket, a [`SocketMessage`]: an order
  (focus a window, move it, change the layout, resize it, show a workspace, retile, stop) or a
  question (the state, the monitors, one [`StateQuery`]).
- **Configuration**: `komorebi.json`, which the daemon reads when it starts and again when it
  changes ([`StaticConfig`]). It lives in the **config home**, [`HOME_DIR`], unless `--config`
  names another.
- **Rule**: a condition on a window's title, executable, class or path ([`MatchingRule`]), in
  one of the process-wide lists that decide whether a window is managed (the ignore and the
  manage rules) and whether it floats when it opens (the float rules).
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

A command is one of: move and resize a window, focus it, raise it, hide it or restore it,
minimize it or bring it back, maximize it or unmaximize it, close it, put the pointer in the
middle of a rectangle, make desktops for the workspaces, show a workspace on a monitor, and put
a window on a workspace. The workspace a window is on, and the one each monitor shows, are
queries.

The window manager tiles a tree of four levels, each level a [`Ring`]: a list with one
element focused, so that "the focused workspace" or "the focused container" means
the same thing at every level.

- A **monitor** ([`Monitor`]) is one of the backend's monitors: its bounds, its work area,
  its padding, an optional work area offset, an optional window-based offset with the number of
  windows up to which it applies, and its workspaces. It shows the focused one.
- A **workspace** ([`Workspace`]) has a layout, its padding, an optional work area offset of its
  own, whether it takes the monitor's window-based offset, its containers, in order, its
  monocle container and its maximized window, if it has them, and its floating windows, which
  are in no container and which the layout does not tile, and which layer it is on.
- A **container** ([`Container`]) has its windows. A new window gets a container of its
  own, unless the window container behavior says to put it in the focused one. With more than
  one window it is a stack: the focused window is in front and the others are hidden. A
  container can be locked, which keeps it at its index while others are inserted, removed or
  swapped around it.
- A **window** ([`Window`]) is a `Copy` handle that carries only the compositor's id.
  Everything else about the window, and every change to it, goes through the backend.

Tiling a workspace ([`Workspace::update`]) is a function of this tree and a work area. The
monitor gives its focused workspace the work area, the padding to fall back to, the offset
(its own, or else the window manager's) and its window-based offset ([`WorkspaceGlobals`]).
The workspace takes the offset (its own, or else the monitor's) off the work area, then the
window-based offset while it has no more containers than the monitor's limit, then its
padding; the layout gives one tile per container, in order, each shrunk by the container
padding, and every window of a container is asked to move onto its tile with `set_rect`. A
tile is clamped to the work area, so it cannot reach onto a neighboring monitor.

The [`WindowManager`] keeps the monitors, the windows the user minimized (they are in no
workspace until they are restored), and an index from each window it manages to the
monitor and workspace that have it. It is loaded from the backend at startup: every
monitor, focused on the workspace the compositor shows on it, and every open window that is
normal, alive, not minimized and not ignored by a rule, on the workspace whose desktop it is
on, of the monitor its middle is on. After that it changes only when an event comes in, and
each event ends with the workspace in front tiled again.

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

## Workspaces

komorebi's workspaces are the compositor's virtual desktops: workspace `i` is desktop `i` on
every monitor, and each monitor shows one at a time. The compositor shows the windows of the
desktop in front, hides the others, and keeps which window of each desktop had the focus; the
window manager hides no window to leave a workspace. The reasons are in
[DESIGN.md](DESIGN.md#why-are-komorebis-workspaces-kwins-virtual-desktops).

Showing a workspace ([`Monitor::load_focused_workspace`]) is three steps. Every window of every
workspace of the monitor is put on the desktop of its workspace, unless it is already there.
The compositor is asked to show the workspace on the monitor ([`Backend::switch_workspace`]),
which leaves the other monitors as they are. The workspace's focused window is given the focus.
The order matters: a window that is focused while it is on another desktop takes the monitor
back to that desktop.

Desktops are global in KWin: every monitor has as many as the monitor with the most
workspaces, and each has one name. At startup, and when a workspace is asked for that has no
desktop, the backend creates the missing ones over KWin's `org.kde.KWin.VirtualDesktopManager`
interface, named "Workspace 1", "Workspace 2" and so on ([`WindowManager::sync_workspace_desktops`]).
Without KWin's per-output virtual desktops every output would show the same desktop, so
[`connect`] turns them on in the user's `kwinrc`, and [`Kwinrc`] keeps the user's value and
puts it back when komorebi stops, also after a crash.

Something else can switch a monitor to another desktop: a pager, a KWin shortcut, an
application that raises a window of another desktop, which KWin answers by showing that
desktop. KWin has one signal for it, which does not say which output, so the bridge sends the
desktop of every output after it, and once a second. The table compares it with what it had,
and a monitor that shows another desktop is a `WorkspaceChange` event: the window manager
focuses that workspace and tiles it ([`WindowManager::adopt_workspace_change`]), and asks the
compositor for nothing, since KWin has already shown the windows and chosen which to focus. A
window put on another desktop by something else (the window menu's "Move to Desktop") is a
`WorkspaceMove`: the window manager moves the window to that workspace. komorebi's own switch
and move come back from KWin as the same reports; the table records them when the command is
sent ([`Table::switch_commands`], [`Table::window_workspace_commands`]), so they raise nothing.

## Stacks

A container has one tile, so when it has more than one window the others have to be out of
sight, and out of the way of the pointer and the focus: the window manager hides them
([`Window::hide`]). Minimizing them would not do. KWin reports a minimize as the user's, and
the windows would be taken for ones the user does not want tiled. What komorebi uses is the one
thing KWin has that hides a window and leaves it alone: a virtual desktop nobody looks at, the
parking desktop. The reasons are in
[DESIGN.md](DESIGN.md#why-are-the-windows-behind-the-front-one-of-a-stack-moved-to-a-parking-desktop).

A container keeps its front window in sync with what is in sight ([`Container::add_window`],
[`Container::load_focused_window`]): the window that is added or focused is shown, and every
other one is hidden. Operations that change which window is in front, or which container a
window is in, end with that call: stacking and unstacking ([`Workspace::move_window_to_container`],
[`Workspace::new_container_for_focused_window`]), a window that closes
([`Workspace::remove_window`], which brings the next one forward and focuses it), a window that
is floated, a focus change to a window that is behind ([`Workspace::focus_container_by_window`]),
and cycling the stack or focusing a window of it by its index.

A monocle hides the other containers in the same way. Turning it on hides every window of each
of them ([`Container::hide`]), turning it off restores the window in front of each
([`Container::restore`]), and cycling it hides the container it gave up. While a monocle is on,
[`Workspace::restore`] restores its window and no other, and [`Workspace::hide`] hides it with
the rest.

[`Window::hide`] puts the window's id in [`HIDDEN_WINDOWS`] before it asks the backend, and
[`Window::restore`] takes it out, so that when the compositor reports something about the window
because of the hide the window manager knows who did it. A `Minimize` for a window in the set is
not the user's and changes nothing. On the way out, [`WindowManager::stop`] and the daemon's
`SIGINT` and `SIGTERM` handler restore every window in the set before the backend shuts down
([`WindowManager::restore_all_windows`]).

The KWin backend hides a window by putting it on the parking desktop. The table keeps for each
window whether it is parked and the desktop it came from ([`Table::hide_commands`],
[`Table::restore_commands`]): a hidden window is still on its workspace for every query
([`Table::window_workspace`]), a window put on another workspace while it is hidden is hidden
still and comes back there ([`Table::window_workspace_commands`]), and KWin's report that the
window moved to the parking desktop is not a `WorkspaceMove`. The parking desktop is made the
first time it is needed ([`NeedsPark`], `create_park_desktop` in [`kwin`]), by position, so
that it stays after the workspaces' desktops. The table checks that KWin still lists it before it
sends a window there, and asks for a new one when somebody removed it.

An application that raises a hidden window, or a click on it in the task manager, makes KWin
switch that monitor to the parking desktop. The table notices that a monitor shows it, sends the
monitor back to the desktop it showed before, and raises no `WorkspaceChange`; the window's own
`FocusChange` then brings it to the front of its stack.

KWin keeps its virtual desktops in `kwinrc` as soon as one is made, so a parking desktop that
komorebi does not remove stays for every later session. The backend removes it in `shutdown`,
after the restoring commands have reached the bridge. A daemon that was killed leaves it, with
windows on it: the first snapshot the next daemon gets lists a desktop with that name, which the
table treats as stale ([`Table::stale_park`]). It is never the parking desktop of the new run.
The snapshot sends the windows on it to the first workspace's desktop, and half a second later
the backend removes it (`sweep_stale_park`).

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
container in that direction, the focus goes to the monitor in that direction
([`WindowManager::monitor_idx_in_direction`], from the bounds of the monitors), to its container
nearest the monitor the focus came from (the leftmost one going right); with no monitor there
either, the message fails and changes nothing. Whether the pointer follows
the focus is [`WindowManager::mouse_follows_focus`], which `MouseFollowsFocus` and
`ToggleMouseFollowsFocus` set.

`MoveWindow` and `CycleMoveWindow` swap the focused container with the container in that
direction ([`WindowManager::move_container_in_direction`],
[`WindowManager::move_container_in_cycle_direction`]), and tile the workspace again: the window
that moved takes the other's tile, keeps the focus, and the pointer follows it. The swap goes
through `LockableSequence`, so a lock stays with its index: the container swapped onto a locked
index is locked in its place. When there is no container in that direction, the container goes
to the monitor in that direction ([`WindowManager::move_container_to_monitor`]), and
[`WindowManager::cross_monitor_move_behaviour`] says what it does there: by default (`Swap`) the
container at that edge of the other monitor comes back in its place, `Insert` puts it in among
the containers there, and `NoOp` keeps it where it is. With no monitor in that direction, the
message fails and nothing moves. [`WindowManager::cross_boundary_behaviour`] can make both cross
to the next workspace of the same monitor instead.

`ChangeLayout` and `CycleLayout` set the focused workspace's [`Layout`], and `FlipLayout`
mirrors it along an axis, on top of the flip it has ([`Workspace::layout_flip`]); each tiles
the workspace again ([`WindowManager::change_workspace_layout_default`],
[`WindowManager::cycle_layout`], [`WindowManager::flip_layout`]). A flip mirrors the tiles,
not the order of the containers, and a direction is flipped with them, so `focus left` on a
flipped layout goes to the tile on the left. `ToggleTiling` turns the workspace's tiling off
and on ([`Workspace::tile`]): with it off, [`Workspace::update`] moves no window, the workspace
still takes the windows that open into containers, and they are tiled when it is turned on.

`LayoutRatios` and `ScrollingLayoutColumns` set the focused workspace's [`LayoutOptions`]
([`Workspace::layout_options`]): the share of the work area its first columns and rows take, and
the number of columns of the scrolling layout in view. The layout calculates the tiles from
them, and from the tiles it calculated last ([`Workspace::latest_layout`]), which the scrolling
layout scrolls from.

The scrolling layout is a strip of columns that runs past the edges of the work area. Its tiles
are the only ones [`Workspace::update`] does not clamp to the work area, so the windows of the
columns out of view are off the screen; a window whose tile is out of view while part of it is
in view is centered in the work area first. A `FocusChange` tiles a scrolling workspace after the
focused container has changed, which scrolls the strip to it. KWin keeps part of a window it
resizes on an output, so the bridge asks again for the position of a window KWin pulled back
within a second of the command, three times at most ([`bridge.js`]'s `settle`), unless the window
is maximized meanwhile.

`Promote` moves the focused container to index 0, the largest tile of every layout but the grid,
through `LockableSequence`, so the containers before it move one index on
([`WindowManager::promote_container_to_front`]). `PromoteSwap` swaps it with the container at
index 0 and records the index it came from in [`Workspace::promotion_swap_container_idx`], so
that from index 0 it swaps back ([`WindowManager::promote_container_swap`]). `PromoteFocus`
focuses the container at index 0 ([`WindowManager::promote_focus_to_front`]), and
`PromoteWindow` focuses the container in a direction and promotes it. On the grid layout they
change nothing.

`ToggleFloat` takes the window the compositor has focused out of its container into the
workspace's floating windows, and back into a container of its own at the focused index
([`WindowManager::toggle_float`]). A floating window is in the index of managed windows like
any other, so its events find it, and [`Workspace::update`] leaves it where it is. Where a
floated window goes is [`WindowManagementBehaviour::toggle_float_placement`]: by default it
stays where it is.

`StackWindow` puts the focused window and the window of the container in a direction in one
container ([`WindowManager::add_window_to_container`]): the focused window goes to the front of
that container, or, when the focused container is a stack and the other is not, the other
container's window goes into the stack. `UnstackWindow` takes the front window out of a stack
into a container of its own after it ([`WindowManager::remove_window_from_container`]), `StackAll`
puts every window of the workspace in one container ([`WindowManager::stack_all`]), and
`UnstackAll` takes them out again ([`WindowManager::unstack_all`]). Each ends with the workspace
tiled again, and the window that was in front has the focus.

`CycleStack` brings the next or the previous window of the focused container to the front, which
wraps around ([`WindowManager::cycle_container_window_in_direction`]), and `FocusStackWindow` the
window at an index ([`WindowManager::focus_container_window`]). Both show that window, hide the
one that was in front ([`Container::load_focused_window`]) and give the window the focus.
`FocusStackWindow` first focuses the monitor the pointer is on, so that a program such as a bar
that runs it for a click on its own monitor acts there. `CycleStackIndex` swaps the front window
with the next or the previous one in the container's order, and keeps it in front
([`WindowManager::cycle_container_window_index_in_direction`]): only the order of the stack
changes. A container of one window cannot be cycled, and an index it does not have cannot be
focused.

A window that opens gets the behavior [`WindowManager::window_management_behaviour`] says for
its workspace: a container of its own (`Create`, the default), or a place in the focused
container (`Append`), and a floating window with the float override on. A workspace's own
[`Workspace::window_container_behaviour`] and [`Workspace::float_override`] win over the window
manager's, and `Append` on an empty workspace is `Create`. `ToggleWindowContainerBehaviour` and
`ToggleFloatOverride` flip the window manager's, and `ToggleWorkspaceWindowContainerBehaviour` and
`ToggleWorkspaceFloatOverride` the focused workspace's, which starts from the opposite of the
window manager's value the first time.

`ToggleMonocle` takes the focused container out of the workspace's containers and makes it the
monocle container, which [`Workspace::update`] puts on the whole work area, less the padding, and
hides the windows of the other containers; turned off, their windows are restored and the
container is put back at the index it came from ([`WindowManager::toggle_monocle`],
[`Workspace::new_monocle_container`], [`Workspace::reintegrate_monocle_container`]). The container's resize adjustment is kept
meanwhile, and so is the whole of [`Workspace::resize_dimensions`], so that the tiles come back
as they were resized. While there is a monocle container, the window in front of it is the one
that has the focus: a `FocusChange` to another window gives the focus back to it, and
[`WindowManager::update_focused_workspace`] focuses it, not the focused container's window, which
is one of the tiles behind. `MoveWindow` and `CycleMoveWindow` are refused, and `ToggleFloat` is
ignored, for the focused container is not the window in front. What `FocusWindow` does is
[`WindowManager::monocle_focus_behaviour`]: with `Cycle`, it makes the next or the previous
container the monocle ([`WindowManager::cycle_monocle`]); with `NoOp`, the default, there is no
container on any side, so the focus goes to the monitor on that side, and it fails where there
is none. `CycleFocusWindow` always carries the monocle to the next or the previous container.
`MonocleFocusBehaviour` sets the behavior, and `ToggleMonocleFocusBehaviour` switches it. A focus
that crosses to a monitor whose workspace has a monocle container gives the focus to its window,
and a container moved to a workspace with one turns that monocle off first.

`ToggleMaximize` takes the focused window out of its container, and the container with it when
it was the last window in it, remembers the index, and asks the backend to maximize it: KWin's
own maximize, to the work area with the panels left alone ([`WindowManager::toggle_maximize`],
[`Workspace::new_maximized_window`]). From then on the workspace has a maximized window, in no
container, and [`Workspace::update`] asks the backend to maximize it again and moves nothing
else, so the containers stay where they were behind it. Toggled again, the window is restored
(the backend gives it back the frame it had), put in a container of its own at the index it came
from, and the workspace is tiled again ([`Workspace::reintegrate_maximized_window`]). A window in
monocle that is maximized leaves the monocle. While a window is maximized, the messages that are
about the tiles are dropped before they are handled: `MoveWindow`, `CycleMoveWindow`, the stack
messages, the resize, promote and preselect messages, `ToggleLock`, `ToggleFloat`,
`ToggleMonocle`, and the messages that send a container to another monitor or workspace
(`is_layout_command` in [`process_command`](../../komorebi/src/process_command.rs)). A window that
closes while it is maximized leaves the workspace like any other. `FocusWindow` finds no
container on any side of the maximized window, so the focus goes to the monitor on that side and
fails where there is none, and a maximized window, or a monocle, on the workspace it reaches is
given the focus whole. `CycleFocusWindow` is not dropped: it gives the focus to the next
container's window (the code has a TODO for what it should do). A maximized window is hidden
with the rest of its workspace, and restored and focused ahead of the containers' windows when
the workspace is shown again ([`Workspace::hide`], [`Workspace::restore`]).

A workspace is on one of two layers ([`Workspace::layer`]), and the focus, move and resize
messages go to the windows of that layer. `ToggleFloat` puts the workspace on the floating layer,
and putting the window back, or a `FocusChange` to a window that is not floating, a new tiled
window or a focus message that reaches a container, puts it on the tiling layer, which is where
a workspace starts; a `FocusChange` to a floating window, and a window that a float rule floats
when it opens, put it on the floating layer. `ToggleWorkspaceLayer` switches it by hand and raises
the windows of the layer it goes to, the focused one last, and gives the focus to one of them.
On the floating layer `FocusWindow` gives the focus to the nearest floating window on that side
of the focused one, by where the compositor has them, and where there is none the focus crosses
as it does for tiles, to the monitor on that side, and fails where there is none
([`WindowManager::focus_floating_window_in_direction`]); `CycleFocusWindow` goes round the
floating windows in the order they were floated
([`WindowManager::focus_floating_window_in_cycle_direction`]); `MoveWindow` moves the focused
floating window by [`WindowManager::resize_delta`], not out of the work area of the monitor
([`WindowManager::move_floating_window_in_direction`]); the resize messages change its frame,
and nothing is tiled ([`WindowManager::resize_window`]); and `PreselectDirection` does nothing.
`ToggleMaximize` maximizes the focused floating window there, which then leaves the floating
windows. `ToggleWorkspaceLayer` hides the floating windows when it goes to the tiling layer, and
shows them when it goes to the floating layer. A window that opens while the workspace is on its floating layer floats
(`floating_layer_override`) when the floating layer behaviour says to float: the workspace's own
([`Workspace::floating_layer_behaviour`]), else its monitor's, else the window manager's
([`WindowManager::window_management_behaviour`]). All three tile by default, and nothing sets
the first two until the configuration does.

`ToggleLock` locks the focused container, or unlocks it ([`WindowManager::toggle_lock`]), and
`LockMonitorWorkspaceContainer` and `UnlockMonitorWorkspaceContainer` do the same for a
container given by its indices. A lock changes nothing until containers are inserted, removed or
swapped around it, which the workspace does through `LockableSequence`.

`Minimize` and `Close` ask the backend to minimize or close the window the compositor has
focused, and change nothing else: the window leaves the layout when the compositor reports it,
through the `Minimize` and `Destroy` events. `UnmanageFocusedWindow` and `ManageFocusedWindow`
do not change the model either: they raise an `Unmanage` or a `Manage` event for the focused
window on the channel the backend's events go through
([`WindowManager::unmanage_focused_window`], [`WindowManager::manage_focused_window`]), and
the window manager handles them in order with the compositor's. An `Unmanage` is handled like a
`Destroy` of a window that stays open: the window leaves its workspace, is centered in the work
area, and its id goes into a list of unmanaged windows that `handle_event` checks before it acts
on any other event about a window; a `Manage` takes the id out of the list and is handled like a
`Show`. A `Destroy` takes the id out of the list too.

Whether a window is managed is [`Window::should_manage`]: the window has to be alive and normal,
and then the rules decide ([`should_act`]). An ignore rule that matches keeps the window out
unless a manage rule matches it too. The float rules do not decide whether a window is managed,
but a `Show` puts a window one matches in the workspace's floating windows instead of a container
(a `Manage` does not ask them). The rules live in process-wide lists in `lib.rs`:
`IgnoreRule` and `ManageRule` add a rule that names an application by one of the four
([`ApplicationIdentifier`]), and take the windows that it names exactly out of the containers of
the workspaces in front; `SessionFloatRule` adds a float rule made of the focused window's
executable, title and class, all three to be equal, and floats it, and `ClearSessionFloatRules`
takes those rules out again. `GlobalState` answers with the ignore and the manage rules
([`GlobalState`]), and `DebugWindow` with what [`Window::should_manage`] read of a window and
which rules matched ([`RuleDebug`]). A rule decides when a window is shown; the bridge reports
a title change (`caption`), which is a `TitleUpdate` event, and a `TitleUpdate` about a window
the index does not have, and that is not minimized, is sent again as a `Show`, so that a rule
about the title it has now decides.

`TogglePause` sets [`WindowManager::is_paused`]: while it is set, `handle_event` returns at once
for every event, and [`read_commands_uds`] hands only `TogglePause`, `State`, `GlobalState` and
`Stop` to the window manager. `UnmanagedWindowOperationBehaviour` sets
[`WindowManager::unmanaged_window_operation_behaviour`]: with `NoOp`,
[`WindowManager::process_command`] refuses every message while the window the compositor has
focused is in no container of the focused workspace
([`WindowManager::handle_unmanaged_window_behaviour`]).

`WorkspacePadding`, `ContainerPadding` and their `FocusedWorkspace` forms set a workspace's own
padding, and `NamedWorkspacePadding` and `NamedWorkspaceContainerPadding` do the same for the
workspace with a name, which they look up like `NamedWorkspaceLayout` does
([`WindowManager::monitor_workspace_index_by_name`]). `AdjustWorkspacePadding` and
`AdjustContainerPadding` change the focused workspace's by a number of pixels, which needs a padding
of its own to change ([`WindowManager::adjust_workspace_padding`]). `WorkAreaOffset`,
`MonitorWorkAreaOffset` and `WorkspaceWorkAreaOffset` set the offset of the window manager, of a
monitor or of a workspace, and `ToggleWindowBasedWorkAreaOffset` sets whether the focused workspace
takes its monitor's window-based offset ([`Workspace::apply_window_based_work_area_offset`]). Each
tiles the workspace in front again, or every monitor.

`ResizeWindowEdge` and `ResizeWindowAxis` resize the focused container
([`WindowManager::resize_window`]). A resize is an adjustment of the container's tile, kept in
[`Workspace::resize_dimensions`], one per container, which the layout applies when it calculates
the tiles; before every tiling, the workspace clears the parts of each adjustment that its layout
cannot apply, such as the top and the bottom of a column. The adjustment is the one
`komorebi-layouts` gives for the edge and [`WindowManager::resize_delta`], cut down until no tile
would be smaller than a tenth of the work area, or than it is already. The adjustments are kept
until a `Retile`, which drops them; `RetileWithResizeDimensions` tiles with them, and `Save`,
`Load`, `QuickSave` and `QuickLoad` write them to a file as JSON and read them back.

`FocusWorkspaceNumber` shows another workspace of the focused monitor
([`WindowManager::focus_workspace`]). `MoveContainerToWorkspaceNumber` takes the focused
container (or the focused window, when it is a floating one) to the back of another workspace
of the monitor and shows that workspace, and `SendContainerToWorkspaceNumber` does the same
and leaves the workspace in front showing ([`WindowManager::move_container_to_workspace`]). A
workspace that does not exist yet is created, with the ones before it, and gets a desktop.

The other workspace commands are made of the same two operations. `CycleFocusWorkspace`,
`CycleMoveContainerToWorkspace` and `CycleSendContainerToWorkspace` take the next or the previous
workspace, which wraps around, and `CycleFocusEmptyWorkspace` the next or the previous with no
window. A monitor remembers the workspace that was in front before ([`Monitor::focus_workspace`]
records it), which `FocusLastWorkspace`, `MoveContainerToLastWorkspace` and
`SendContainerToLastWorkspace` take. `FocusWorkspaceNumbers` shows a workspace on every monitor.
`WorkspaceName` names a workspace, `EnsureNamedWorkspaces` gives a monitor named workspaces, and
`FocusNamedWorkspace`, `NamedWorkspaceLayout` and `NamedWorkspaceTiling` look a workspace up by
its name on every monitor ([`WindowManager::monitor_workspace_index_by_name`]); a name no
workspace has changes nothing. `FocusNamedWorkspace` focuses the monitor that has the workspace
([`WindowManager::focus_monitor`]). `WorkspaceLayout` and `WorkspaceTiling` set the layout and the
tiling of a workspace by index. `NewWorkspace` adds a workspace and shows it, `EnsureWorkspaces`
gives a monitor workspaces up to a number, and `CloseWorkspace` removes the workspace in front
when it has no window and no name and is not the monitor's last. A name is komorebi's: the
compositor's desktops keep theirs.

A window the compositor reports shown or focused on a monitor that is not the focused one
focuses that monitor ([`WindowManager::monitor_idx_from_window`]). The monitor commands work on
the ring of monitors: `FocusMonitorNumber`, `CycleFocusMonitor` and `FocusMonitorAtCursor` focus
one ([`WindowManager::focus_monitor`]), and `FocusMonitorWorkspaceNumber` a workspace of it;
`MoveContainerToMonitorNumber`, `SendContainerToMonitorNumber`, their cycle and monitor-workspace
forms, and `MoveContainerToNamedWorkspace` and `SendContainerToNamedWorkspace` take the focused
container to another monitor, put its windows on the desktop of the workspace they go to, and tile
both monitors ([`WindowManager::move_container_to_monitor`]); `MoveWorkspaceToMonitorNumber` and
`CycleMoveWorkspaceToMonitor` take the workspace in front to another monitor, and
`SwapWorkspacesToMonitorNumber` exchanges the workspaces of two monitors.
`DisplayIndexPreference` gives a monitor, by its serial number id, the index the window manager
puts it at when it loads the monitors, with placeholders before it while the monitors in between
are not connected. `EagerFocus` focuses the first window of an executable, searching every
workspace of every monitor, and shows its workspace.

`PreselectDirection` reserves the index on one side of the focused container for the next
window ([`WindowManager::preselect_container_in_direction`]). The workspace inserts an empty
container there ([`Container::preselect`]), which [`Workspace::update`] keeps and the layout
gives a tile, and [`Workspace::new_container_for_window`] puts the next window in it.
`CancelPreselect` removes it. A workspace reserves one place at a time, and the grid layout
none.

## Configuration

The daemon is configured by `komorebi.json`, which [`StaticConfig`] reads. `main.rs` takes the
file `--config` names, or else `komorebi.json` in [`HOME_DIR`] (`$KOMOREBI_CONFIG_HOME`, or
`~/.config/komorebi`) if there is one; without a file, the daemon runs on its defaults. Loading
has three steps, because the monitors are known only after the backend is asked:

1. [`StaticConfig::preload`] reads the file, applies the settings that are process-wide (the
   default paddings, [`LAYOUT_DEFAULTS`], the indices of the monitors, and the rules in
   [`IGNORE_IDENTIFIERS`], [`MANAGE_IDENTIFIERS`] and [`FLOATING_APPLICATIONS`]) and makes the
   [`WindowManager`], with the settings that are its own. It starts a watcher on the file.
2. [`WindowManager::init`] loads the monitors and the windows from the backend, in the order the
   indices of the monitors give.
3. [`StaticConfig::postload`] gives each monitor the entry of the file that is its own, and each
   workspace of the monitor the entry of its place ([`Workspace::load_static_config`]). A monitor
   takes the entry at the index `display_index_preferences` gives its serial number id, or else
   the first entry that no preference names and no monitor has taken. `main.rs` then asks the
   backend for a desktop for each workspace ([`WindowManager::sync_workspace_desktops`]), and
   tiles.

When the file changes, the watcher (a [`hotwatch`] watcher, which waits two seconds for a file
to settle) writes `ReloadStaticConfiguration` to the daemon's own socket, and the message is
handled like any other, with the window manager locked: [`StaticConfig::reload`] reads the file
first and applies it only when it parsed, so a file that is half written changes nothing. It does
for the window manager that is running what the three steps do for one that starts, and tiles
every monitor again.

`app_specific_configuration_path` names files of rules for particular applications, which
[`StaticConfig`] reads after the rules of its own (`apply_globals`): the `ignore`, `manage` and
`floating` rules of each application in each file are appended to the same lists, once each (a
rule that is in the list already is not added again). A path with no file is skipped with a
warning, since one `komorebi.json` may name a file that only another platform has; a file that is
there and is not an applications.json is an error.

Three kinds of key reach the model. A **setting** of the window manager is a field of it
([`WindowManager::resize_delta`] and the others). A **default** is a process-wide static that a
monitor or a workspace falls back to when it has no value of its own: the paddings, the layout
defaults. A **rule** is an entry of one of the process-wide lists. A workspace's own
`layout_rules`, `work_area_offset_rules` and `window_container_behaviour_rules` are fields of the
[`Workspace`], which [`Workspace::update`] reads each time it tiles.

One `komorebi.json` is meant for every platform komorebi runs on, so [`StaticConfig`] has no
`deny_unknown_fields`: a key it does not read is dropped, and the keys of other platforms that
mean nothing on KDE Plasma are fields that nothing reads. `StaticConfig::from(&WindowManager)`
goes the other way, for `GenerateStaticConfig`.

| Rule | Enforced by | Proof | Escape hatch |
|---|---|---|---|
| A key komorebi does not read does not stop a configuration from loading | serde: [`StaticConfig`] ignores unknown keys, and a key of another platform is a field nothing reads | `the_keys_that_do_nothing_on_kde_are_accepted_and_ignored`, `fixtures_load_round_trip_and_only_lose_windows_only_keys` | none |
| The keys the parity test ignores are keys `StaticConfig` does not read | test: the ignore lists are compared with the schema of [`StaticConfig`] | `the_lists_name_no_key_that_is_read` | none |
| The example in the docs is the example file, which loads | test | `the_example_in_the_docs_is_the_example_file`, `fixtures_load_round_trip_and_only_lose_windows_only_keys` | none |
| A path to an applications file that is not there is skipped, and one that is there and is broken is an error | framework path: `handle_asc_file` returns early when the path does not exist, and parses the file otherwise | `a_missing_application_specific_configuration_is_skipped_not_fatal`, `an_application_specific_configuration_that_exists_but_is_broken_is_still_an_error`, `a_file_that_is_not_there_is_skipped_and_one_that_is_broken_is_an_error` | none |
| The rules of an applications file and of the configuration add up | framework path: both are appended to the process-wide lists | `the_rules_of_the_configuration_and_of_the_file_add_up`, `every_file_of_a_list_is_read` | none |
| A file that cannot be read changes nothing | framework path: [`StaticConfig::reload`] reads and parses the file before it applies anything | `a_configuration_that_cannot_be_read_changes_nothing`, `the_daemon_follows_the_file_it_is_given_when_the_file_changes` | none |
| The daemon applies a change to its file, written in place or replaced by a rename | framework path: the watcher sends `ReloadStaticConfiguration` to the daemon's own socket | `the_daemon_follows_the_file_it_is_given_when_the_file_changes`, `vm-module` | none: the file has to be the one the daemon was started with |
| A monitor takes the entry its display index preference gives it, else the first nobody has | framework path: [`StaticConfig::postload`] and [`StaticConfig::reload`] choose the entry by serial number id first | `a_monitor_takes_the_configuration_its_display_index_preference_gives_it` | none |
| A workspace that names no layout and does not say it tiles moves no window | framework path: [`Workspace::load_static_config`] sets [`Workspace::tile`] | `a_workspace_with_no_layout_does_not_tile`, `a_workspace_tiles_when_it_has_a_layout_or_is_told_to` | `"tile": true` |
| A workspace's own layout options replace the defaults of its layout, whole | framework path: [`Workspace::update`] takes the workspace's options and rules when it has either, and else those of `layout_defaults` | `layout_options_of_a_workspace_replace_the_defaults_of_its_layout` | none |
| The highest of the rules a count of containers reaches decides the layout, the offset and the container behavior | framework path: [`Workspace::update`] reads the rules, kept in the order of their thresholds | `layout_rules_change_the_layout_with_the_number_of_windows`, `work_area_offset_rules_change_the_offset_with_the_number_of_windows`, `window_container_behaviour_rules_change_the_behaviour_with_the_number_of_containers` | none |
| A workspace's own container behavior and float override come before the one for every workspace, and its floating layer behavior before its monitor's, which comes before the one for every workspace | framework path: [`WindowManager::window_management_behaviour`] reads the workspace, then (for the floating layer) the monitor, then the window manager | `the_behaviour_of_a_workspace_comes_before_the_one_for_every_workspace`, `the_float_override_of_a_workspace_comes_before_the_one_for_every_workspace`, `the_floating_layer_behaviour_of_a_workspace_comes_before_a_monitors_and_that_before_the_default` | none |
| The configuration the daemon generates loads and says the same | test | `a_configuration_comes_out_again_as_the_settings_of_the_window_manager` | none |
| The daemon reads the file it is given or the one in the config home, not both | startup order: `main.rs` takes `--config` before the config home | `the_daemon_follows_the_file_it_is_given_when_the_file_changes`, `the_daemon_reads_komorebi_json_from_the_config_home` | none |

## The NixOS module

The flake exports one module under two names, `nixosModules.default` and
`nixosModules.komorebi`: [`nixos-module.nix`] and the flake's overlay, which builds
`komorebi-full` (both programs) with the flake's nixpkgs and Rust toolchain. Its options are
`enable`, `package` (by default `pkgs.komorebi-full`, from the overlay), `logLevel`,
`configFile`, and `execStart`, which is read-only: the module sets it to `bin/komorebi` of the
package with `--log-level`, and `--config` with `configFile` when that is set. With
`services.komorebi.enable`, the module:

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
| The daemon is configured by `configFile` when it is set | framework path: `execStart` passes it as `--config`, which comes before the config home | `nixos-module-eval` ("configFile reaches execStart and the unit as --config"), `vm-module` | none: with no `configFile` the daemon reads the file of the config home |
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
  the four levels of the tree; `window` also has the rule engine ([`should_act`]).
- [`lockable_sequence`](../../komorebi/src/lockable_sequence.rs): inserting, removing and
  swapping in a list where some elements are locked in place, which is how a workspace
  changes its containers.
- [`window_manager`](../../komorebi/src/window_manager.rs): the [`WindowManager`] and the
  operations on it (tile the workspace in front, tile every monitor, take a minimized
  window out, move the focus, move the focused container, change the layout, resize or
  promote the focused container, float a window, switch the layer, put a container in
  monocle, maximize a window, lock a container, set the padding of a workspace, manage and
  unmanage a window, stack and unstack windows and choose the one in front, show a
  workspace, move a container to another workspace, follow a workspace switch made by
  something else, name a workspace and find it by its name, set the layout of a workspace
  that is not in front, focus another monitor, and move containers and workspaces between
  monitors).
- [`desktop`](../../komorebi/src/desktop.rs): loads the monitors and the open windows from
  the backend.
- [`window_manager_event`](../../komorebi/src/window_manager_event.rs) and
  [`process_event`](../../komorebi/src/process_event.rs): the events, and what the window
  manager does with each.
- [`window_manager_event_listener`](../../komorebi/src/window_manager_event_listener.rs): the
  channel that carries events from the backend to the window manager, and
  [`listen_for_events`], the thread that takes them off it.
- [`kwin`](../../komorebi/src/kwin/mod.rs): the KWin backend, its D-Bus service and the held
  call; [`kwin::state`](../../komorebi/src/kwin/state.rs), the table;
  [`kwin::kwinrc`](../../komorebi/src/kwin/kwinrc.rs), the editor of the user's `kwinrc`, which
  keeps the values it changes and puts them back; the parking desktop is made, swept and removed
  in `kwin` and tracked in `kwin::state`; and
  [`bridge.js`](../../komorebi/src/kwin/bridge.js), the bridge.
- [`static_config`](../../komorebi/src/static_config.rs): `komorebi.json`, [`StaticConfig`], and
  how it is loaded, reloaded and generated.
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
  fake with `--fake-backend`, makes the window manager from `komorebi.json` when there is one,
  which binds the socket, loads the desktop, gives the monitors their configuration and tiles, starts the threads that listen on the socket and handle events, and
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
- Workspace `i` is the compositor's desktop `i`, and each monitor shows one
  (`switch_workspace`, `set_window_workspace`, `window_workspace`, `current_workspaces`). A
  backend asked for a workspace that has no desktop makes the desktops up to it
  (`ensure_workspaces`). The compositor shows and hides the windows of the desktops, and a
  backend reports a switch or a move it did not make as an event.
- A hidden window (`hide`) is out of sight, alive, not minimized, and on the workspace it was
  on until `restore`: `window_workspace` still names that workspace, and
  `set_window_workspace` changes where it comes back to, not where it is hidden. Only the back of
  a stack and the containers behind a monocle are hidden: nothing is hidden to leave a
  workspace.
- `shutdown`, `is_window`, `window_rect` and `monitor_from_point` have defaults, written in
  terms of the queries. An implementor should not need to override them, except `shutdown`
  when it created something in the compositor that has to be given up.

## What is guaranteed, and by what

| Rule | Enforced by | Proof | Escape hatch |
|---|---|---|---|
| A backend implements every query and command but `maximize` | type system: the methods have no defaults, except `maximize`, which does nothing until a backend implements it | `compile_fail,E0046` on [`Backend`] | none |
| A backend can be shared between threads | type system: `Backend: Send + Sync + 'static` | `compile_fail,E0277` on [`Backend`] | none |
| The window manager asks for a backend that was never installed | runtime error: [`backend`] panics, naming [`set_backend`] and [`with_backend`] | `# Panics` on [`backend`] | none |
| A test's backend does not outlive the test | framework path: [`with_backend`] restores the previous one on drop | `with_backend_puts_the_previous_backend_back_when_the_closure_panics` | none |
| A command about a missing window is an error, not a panic | test | `a_command_about_a_window_nobody_opened_is_an_error`, `a_command_about_a_closed_window_is_an_error` and `hiding_or_restoring_a_window_nobody_opened_is_an_error` against the fake, `a_command_about_a_window_the_table_does_not_know_is_an_error_and_sends_nothing` against the KWin backend | none |
| A locked container keeps its index | framework path: a [`Workspace`] inserts, removes and swaps its containers through `LockableSequence` | `test_locked_containers_with_new_window`, `test_locked_containers_remove_window`, `a_window_that_opens_does_not_push_a_locked_container_from_its_index` | `containers_mut()` is the `VecDeque` itself |
| A container in monocle goes back to the index it came from, and the tiles come back as they were resized | framework path: [`Workspace::new_monocle_container`] keeps the index and [`Workspace::reintegrate_monocle_container`] inserts at it, and [`Workspace::update`] does not shorten the resize adjustments while there is a monocle container | `test_toggle_monocle`, `a_monocle_goes_back_to_its_index_even_when_a_window_opens_meanwhile`, `a_tile_that_was_resized_is_the_size_it_was_after_a_monocle` | none |
| The window in monocle keeps the focus | framework path: `handle_event` gives the focus back to the monocle window on a `FocusChange`, and [`WindowManager::update_focused_workspace`] focuses it | `a_window_that_takes_the_focus_from_the_monocle_is_made_to_give_it_back` | none |
| What is about the tiles does nothing while a window is in monocle | framework path: [`WindowManager::move_container_in_direction`] and [`WindowManager::move_container_in_cycle_direction`] bail, and [`WindowManager::toggle_float`] returns, when the window in front is in monocle | `what_is_about_the_tiles_is_refused_or_ignored_while_a_monocle_is_on` | none |
| A focus that crosses to a monitor in monocle gives its monocle window the focus, and a container moved or sent to a workspace in monocle turns the monocle off | framework path: the `None` arm of [`WindowManager::focus_container_in_direction`], [`WindowManager::move_container_in_direction`], [`WindowManager::move_container_to_monitor`] and [`Monitor::move_container_to_workspace`] | `focusing_towards_a_monitor_in_monocle_gives_the_focus_to_its_monocle_window`, `a_container_moved_to_a_monitor_in_monocle_turns_the_monocle_off_there`, `a_container_sent_to_a_workspace_in_monocle_turns_the_monocle_off_there` | none |
| A window in monocle that closes takes the monocle with it | framework path: [`Workspace::remove_window`] looks in the monocle container, which the index of managed windows lists | `closing_the_window_in_monocle_tiles_the_others_again` | none |
| In a real KWin, the window in monocle fills the work area and goes back to its tile | VM test | `vm-core-e2e` ("in monocle the focused window fills the work area", "toggling back puts it on its tile") | none |
| A clean stop, and the next start after a crash, bring back the windows a monocle hid | VM test | `vm-core-e2e` ("and every window is on a workspace's desktop", "after a crash and a restart a monocle still hides the other window") | none |
| A maximized window goes back in a container of its own at the index it came from, and the tiles are where they were | framework path: [`Workspace::new_maximized_window`] keeps the index and [`Workspace::reintegrate_maximized_window`] inserts at it, after asking the backend to restore the window, and nothing else moves in between | `toggle_maximize_takes_the_window_out_of_the_layout_and_back`, `maximizing_works_in_every_layout`, `a_window_goes_back_in_a_container_of_its_own_at_the_index_it_came_from` | none |
| What is about the tiles does nothing while a window is maximized | framework path: `is_layout_command` in [`WindowManager::process_command`] drops the message before it is handled | `while_a_window_is_maximized_what_is_about_the_tiles_does_nothing` | none |
| A window stays maximized through a retile | framework path: [`Workspace::update`] asks the backend to maximize it again, and moves no container | `a_retile_while_a_window_is_maximized_asks_the_compositor_to_maximize_it_again` | none |
| A maximized window that closes takes the maximize with it | framework path: [`Workspace::remove_window`] looks at the maximized window first, which the index of managed windows lists | `a_maximized_window_that_closes_leaves_the_layout_whole` | none |
| In a real KWin, the maximized window fills the work area, is on top, and goes back to its tile | VM test | `vm-features` ("the focused window fills the work area", "and is the one on top", "toggled back, it is in its tile again") | none |
| The focus, move and resize messages act on the windows of the layer a workspace is on | framework path: [`WindowManager::process_command`] matches on [`Workspace::layer`] for `FocusWindow`, `CycleFocusWindow` and `MoveWindow`, [`WindowManager::resize_window`] for the resize messages, and `PreselectDirection` checks it | `focusing_in_a_direction_on_the_floating_layer_goes_to_the_nearest_floating_window`, `moving_on_the_floating_layer_moves_the_floating_window_by_the_resize_delta`, `resizing_on_the_floating_layer_changes_the_frame_of_the_floating_window`, `a_direction_to_preselect_is_ignored_on_the_floating_layer`, `focusing_a_floating_window_towards_a_monitor_crosses_to_it_and_to_its_monocle_window` | none |
| The layer follows the window that has the focus | framework path: `handle_event` sets [`Workspace::layer`] on a `FocusChange`, a `Show` and [`WindowManager::toggle_float`], and [`Monitor::move_container_to_workspace`] on the workspace a window is moved to | `floating_a_window_puts_the_workspace_on_the_floating_layer_and_focusing_a_tile_takes_it_back`, `a_window_sent_to_another_workspace_puts_that_workspace_on_the_layer_it_arrived_on` | none |
| A window that opens on the floating layer floats only when its floating layer behaviour says so | framework path: [`WindowManager::window_management_behaviour`] takes the workspace's behaviour, else its monitor's, else its own, and `handle_event` floats the window on `floating_layer_override` | `the_floating_layer_behaviour_of_the_workspace_wins_over_the_monitors_and_the_window_managers`, `a_window_that_opens_on_the_floating_layer_floats_when_the_behaviour_says_so` | none |
| The windows of the layer a workspace is not on are out of sight | framework path: `ToggleWorkspaceLayer` hides the floating windows when it goes to the tiling layer and restores them when it goes to the floating one | `going_back_to_the_tiles_hides_the_floating_windows_and_going_to_them_shows_them_again` | none |
| Cycling the focus on the floating layer visits every floating window | test | `cycling_the_focus_on_the_floating_layer_goes_round_the_floating_windows_in_order` | none |
| A floating window is never moved out of the work area by `move` | framework path: [`WindowManager::move_floating_window_in_direction`] keeps the frame inside the focused monitor's work area | `moving_on_the_floating_layer_moves_the_floating_window_by_the_resize_delta` | none |
| A window komorebi does not manage is never moved | framework path: `handle_event` asks [`Window::should_manage`] before it acts on any event but a destroy | `dialogs_and_other_non_normal_windows_are_left_alone`, `an_ignore_rule_keeps_a_window_unmanaged` | `ManageRule` |
| A window the user unmanaged stays out of the layout until it is managed again | framework path: `handle_event` checks the list of unmanaged windows before it acts on any event but a removal | `unmanage_takes_the_focused_window_out_until_manage_takes_it_back` | none |
| A paused window manager moves no window | framework path: `handle_event` returns before anything else, and [`read_commands_uds`] drops every message but `TogglePause`, `State`, `GlobalState` and `Stop` | `while_paused_no_event_changes_the_layout_and_manage_takes_in_a_window_it_missed`, `while_paused_only_toggle_pause_state_and_stop_are_handled` | none |
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
| A window komorebi puts off the screen stays there | framework path: the bridge asks again for the position of a window KWin pulled back after a `geo` command, for a second, three times at most | `a window KWin pulls back from where it was asked to be is asked for the position again`, `a window that cannot go where it was asked is asked again three times at most`, `vm-scrolling` | a window that cannot go where it was asked is left where KWin put it |
| A resize leaves no tile smaller than a tenth of the work area, or than it was | framework path: [`WindowManager::resize_window`] calculates the tiles for the adjustment, and cuts the adjustment down until they all fit | `a_resize_leaves_every_tile_about_a_tenth_of_the_work_area_wide_at_least` | the paddings are taken off afterwards, so a tile can end up a little smaller |
| The offset of a workspace wins over its monitor's, which wins over the window manager's | framework path: [`Monitor::update_workspace_globals`] gives the workspace the monitor's offset or else the window manager's, and [`Workspace::update`] takes its own before that | `the_workspaces_offset_wins_over_the_monitors_which_wins_over_the_global_one` | none |
| A workspace whose tiling is off moves no window | framework path: [`Workspace::update`] returns before the layout when [`Workspace::tile`] is false | `with_tiling_off_no_window_is_moved_until_it_is_back_on` | none |
| komorebi hides no window to leave a workspace | framework path: showing a workspace is one request to the compositor ([`Backend::switch_workspace`]), which shows and hides the windows of its desktops | `switching_workspace_asks_the_compositor_to_show_the_other_desktop`, `vm-workspace-switch` ("KWin shows the desktop, komorebi hides nothing") | none |
| Showing a workspace on one monitor leaves the other monitors as they are | KWin's per-output virtual desktops, which [`connect`] turns on | `showing_a_workspace_switches_that_output_only`, `vm-kwin-contract` ("the other keeps its own") | a `kwinrc` komorebi cannot write: every monitor switches |
| A window is on its new workspace's desktop before the desktop is shown or the window focused | framework path: [`Monitor::load_focused_workspace`] puts every window on its desktop first | `moving_a_window_to_another_workspace_puts_it_on_the_desktop_before_showing_it` | none |
| A workspace switch or a window move made by something else is followed, and answered with no command | framework path: the table reports what it did not send as a `WorkspaceChange` or a `WorkspaceMove`, and [`WindowManager::adopt_workspace_change`] asks the compositor for nothing | `a_workspace_switch_by_someone_else_is_followed_not_undone`, `a_window_moved_to_another_desktop_by_someone_else_changes_workspace`, `another_party_switching_a_monitor_is_a_workspace_change`, `vm-workspace-switch` ("nothing is flip-flopping") | none |
| komorebi's own switch or move, reported back by KWin, changes nothing | framework path: the table records a switch and a move when it makes their commands | `the_compositor_reporting_komorebis_own_switch_changes_nothing`, `komorebis_own_move_coming_back_as_an_event_moves_nothing`, `komorebi_switching_a_monitor_comes_back_as_a_change_to_what_it_already_knows` | none |
| The user's `kwinrc` gets back the value komorebi changed, also after a crash | framework path: [`Kwinrc`] records the first value of a key in a backup file, and the backend's `shutdown` puts it back | `what_komorebi_changed_is_put_back_exactly_and_what_it_added_is_removed`, `a_backup_left_by_a_crash_keeps_the_values_from_before_the_first_run`, `vm-workspace-switch` ("back as they were") | `SIGKILL`: the next run puts it back when it stops |
| A workspace is closed only when it has no window and no name and is not the monitor's last | framework path: the `CloseWorkspace` arm of [`WindowManager::process_command`] checks all three | `new_workspace_adds_one_and_close_workspace_removes_it_when_it_is_empty_and_unnamed` | none |
| The focus and a move at the edge of a monitor go to the monitor on that side, and fail only when there is none | framework path: the `None` arms of [`WindowManager::focus_container_in_direction`] and [`WindowManager::move_container_in_direction`] | `the_focus_crosses_to_the_monitor_on_that_side`, `a_move_at_the_edge_of_a_monitor_swaps_with_the_window_on_the_next_one_by_default`, `focus_towards_a_side_with_no_window_is_an_error_and_changes_nothing` | `NoOp` keeps a move on its monitor |
| A window shown or focused on another monitor focuses that monitor, and nothing else does | framework path: `handle_event` focuses the monitor of a `Show` or `FocusChange` before it acts on it | `a_window_that_opens_on_the_monitor_that_is_not_focused_focuses_it`, `a_scrolling_strip_on_the_next_monitor_over_does_not_take_focus_with_it` | none |
| Only the window in front of a stack is in sight | framework path: [`Container::add_window`] and [`Container::load_focused_window`] hide every window of the container but the focused one, and every operation that changes the front ends with the call | `a_window_added_to_a_container_is_in_front_and_the_others_are_hidden`, `loading_the_focused_window_shows_it_and_hides_the_rest`, `stacking_puts_two_windows_in_one_container_and_hides_the_one_behind`, `vm-stacks` ("exactly one window is on it") | none |
| A hidden window is still on its workspace, and is not reported as moved | framework path: the table keeps the desktop a window came from, and a move to the parking desktop is its own | `parking_a_window_hides_it_without_unmanaging_it`, `komorebis_own_move_and_its_park_and_restore_raise_nothing_new`, `a_window_is_put_on_a_workspace_by_putting_it_on_its_desktop` | none |
| What the compositor reports about a window komorebi hid is not taken for the user's doing | framework path: [`Window::hide`] records the window in [`HIDDEN_WINDOWS`] before it asks the backend, and the `Minimize` arm of `handle_event` checks it | `a_minimize_komorebi_asked_for_is_not_mistaken_for_the_user`, `a_minimize_the_user_asked_for_takes_the_window_out_of_the_layout` | none |
| While a window is in monocle the windows of the other containers are hidden, and they are shown again when it is turned off | framework path: [`WindowManager::monocle_on`], [`WindowManager::cycle_monocle`] and [`WindowManager::monocle_off`] hide and restore the containers, and [`Workspace::remove_window`] restores them when the window in monocle closes | `a_monocle_hides_the_other_windows_until_it_is_turned_off`, `a_window_in_monocle_that_leaves_shows_the_containers_behind_it`, `the_back_of_a_stack_stays_hidden_in_monocle_and_after_it`, `vm-stacks` ("the window in monocle fills the work area and the other one is out of sight") | none |
| The window the compositor focuses in a stack comes to the front | framework path: the `FocusChange` arm of `handle_event` focuses the container by the window, which loads it ([`Workspace::focus_container_by_window`]) | `focusing_a_hidden_window_of_a_stack_brings_it_to_the_front`, `the_window_the_user_focuses_in_the_back_of_a_stack_comes_to_the_front` | none |
| Cycling a stack changes which window is in front and nothing else | framework path: [`Container::load_focused_window`] shows the focused window and hides the rest; a cycle by index only swaps two places | `cycling_the_stack_brings_the_next_window_to_the_front_and_hides_the_one_that_was`, `cycling_the_index_moves_the_front_window_along_the_stack_and_keeps_it_in_front`, `focus_stack_window_brings_the_window_at_an_index_to_the_front`, `vm-stacks` ("cycle the stack") | none |
| A stack with one window cannot be cycled, and an index it lacks is refused | runtime error: the window manager's functions return it before they change anything | `cycling_a_window_that_is_alone_is_an_error`, `focus_stack_window_refuses_an_index_the_stack_does_not_have` | none |
| Closing the window in front of a stack brings the next one forward | framework path: [`Workspace::remove_window`] shows the container's new front window and focuses it | `closing_the_window_in_front_of_a_stack_brings_the_next_one_forward`, `vm-stacks` ("closing the window in view") | none |
| The back of a stack stays hidden when its workspace is shown again | framework path: [`Workspace::restore`] restores the front window of each container and no other | `the_back_of_a_stack_stays_hidden_across_a_workspace_round_trip`, `vm-stacks` ("a workspace round trip") | none |
| A window is never sent to a parking desktop that is not there | framework path: [`Table::hide_commands`] asks for a new one unless KWin lists the one it made | `a_parking_desktop_that_has_gone_is_made_again_not_sent_windows` | none |
| A monitor raised onto the parking desktop is sent back | framework path: the table sends the monitor back to the desktop it showed before | `following_a_hidden_window_to_the_parking_desktop_sends_that_output_back` | none |
| A clean stop leaves no window hidden and no parking desktop | framework path: [`WindowManager::stop`] and the signal handler restore every hidden window, and the backend's `shutdown` removes the desktop after the restoring commands have gone out | `restoring_every_window_brings_back_the_hidden_ones_unless_it_is_told_to_ignore_that`, `vm-stacks` ("a clean stop puts every window back") | `SIGKILL`, and `StopIgnoreRestore`, which restores nothing |
| A parking desktop a dead daemon left is swept by the next one, and its windows go home | startup check: the first snapshot marks it stale, sends its windows to the first workspace's desktop, and the backend removes it | `a_parking_desktop_left_by_a_crashed_run_is_swept`, `a_stale_parking_desktop_is_not_the_one_windows_are_hidden_on`, `vm-stacks` ("a crash leaves a parking desktop") | none |
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
   whether the window is alive and normal, and then the rules whether it is to be managed. If
   it is not, the event changes nothing. If a float rule matches it, it goes into the
   workspace's floating windows and the steps below do not apply.
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
- [`komorebi/src/static_config.rs`](../../komorebi/src/static_config.rs): `komorebi.json`.
  [`komorebi/tests/config.rs`](../../komorebi/tests/config.rs) loads files over the fake
  desktop, [`komorebi/tests/config_displays.rs`](../../komorebi/tests/config_displays.rs) one
  with several monitors, [`komorebi/tests/hot_reload.rs`](../../komorebi/tests/hot_reload.rs) the
  daemon following a file, and
  [`komorebi/tests/config_parity.rs`](../../komorebi/tests/config_parity.rs) the files of other
  platforms and the example, which load; [`komorebi/tests/asc.rs`](../../komorebi/tests/asc.rs)
  the applications files; [`komorebi/src/core/asc.rs`](../../komorebi/src/core/asc.rs) their
  format.
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
  tiling a real KWin, checked against what KWin has, with a window in monocle, and read and
  stopped with `komorebic`.
- [`nix/tests/vm/suites/kwin-restart`](../../nix/tests/vm/suites/kwin-restart/test.py): KWin
  restarting under the daemon.
- [`komorebi/tests/wm_events.rs`](../../komorebi/tests/wm_events.rs): the window manager
  driven end to end against the fake.
- [`komorebi/tests/socket.rs`](../../komorebi/tests/socket.rs): the daemon over its socket,
  from its start to a `Stop`.
- [`komorebi/tests/focus.rs`](../../komorebi/tests/focus.rs),
  [`komorebi/tests/moving.rs`](../../komorebi/tests/moving.rs) and
  [`komorebi/tests/layouts.rs`](../../komorebi/tests/layouts.rs): the focus, move and layout
  commands against the fake;
  [`komorebi/tests/layout_options.rs`](../../komorebi/tests/layout_options.rs) and
  [`komorebi/tests/scrolling.rs`](../../komorebi/tests/scrolling.rs), the layout options and the
  scrolling layout; [`komorebi/tests/resize.rs`](../../komorebi/tests/resize.rs), resizing;
  [`komorebi/tests/promote.rs`](../../komorebi/tests/promote.rs), promoting;
  [`komorebi/tests/float.rs`](../../komorebi/tests/float.rs), floating;
  [`komorebi/tests/lock.rs`](../../komorebi/tests/lock.rs), locking;
  [`komorebi/tests/floating_layer.rs`](../../komorebi/tests/floating_layer.rs), the layers;
  [`komorebi/tests/monocle.rs`](../../komorebi/tests/monocle.rs), the monocle;
  [`komorebi/tests/maximize.rs`](../../komorebi/tests/maximize.rs) and
  [`komorebi/tests/maximize_commands.rs`](../../komorebi/tests/maximize_commands.rs), maximizing;
  [`komorebi/tests/manage.rs`](../../komorebi/tests/manage.rs), minimizing, closing,
  unmanaging and managing the focused window, and pausing;
  [`komorebi/tests/padding.rs`](../../komorebi/tests/padding.rs), the paddings and the work area
  offsets; [`komorebi/tests/rules.rs`](../../komorebi/tests/rules.rs) and
  [`komorebi/tests/rule_commands.rs`](../../komorebi/tests/rule_commands.rs), the rules and the
  messages that add and show them.
- [`nix/tests/vm/suites/scrolling`](../../nix/tests/vm/suites/scrolling/test.py): the scrolling
  strip in a real KWin, with its columns out of view off the screen.
- [`nix/tests/vm/suites/features`](../../nix/tests/vm/suites/features/test.py): what is on the
  screen of a real compositing session, read from screenshots.
- [`nix/tests/vm/suites/commands`](../../nix/tests/vm/suites/commands/test.py): every layout,
  and every `komorebic` command, against a real KWin with two outputs.
- [`komorebi/tests/workspaces.rs`](../../komorebi/tests/workspaces.rs): workspaces as the
  fake's desktops;
  [`komorebi/tests/workspace_commands.rs`](../../komorebi/tests/workspace_commands.rs), naming,
  cycling and the other workspace commands;
  [`komorebi/tests/monitors.rs`](../../komorebi/tests/monitors.rs), two monitors: crossing from
  one to the other, the monitor commands and `EagerFocus`; [`komorebi/src/kwin/kwinrc.rs`](../../komorebi/src/kwin/kwinrc.rs), the
  `kwinrc` editor and its tests.
- [`komorebi/tests/stacks.rs`](../../komorebi/tests/stacks.rs): stacking, unstacking and the windows
  a stack hides, the behavior of new windows and the float override, against the fake;
  [`nix/tests/vm/suites/stacks`](../../nix/tests/vm/suites/stacks/test.py), the same in a real
  KWin, with the parking desktop, a crash and a stop.
- [`nix/tests/vm/suites/workspace-switch`](../../nix/tests/vm/suites/workspace-switch/test.py):
  workspaces as a real KWin's desktops, switched by `komorebic` and by KWin;
  [`nix/tests/vm/suites/kwin-contract`](../../nix/tests/vm/suites/kwin-contract/test.py), the
  behavior of KWin's per-output virtual desktops komorebi relies on, with two outputs and no
  komorebi.
- [`nix/nixos-module.nix`](../../nix/nixos-module.nix): the NixOS module;
  [`nix/tests/nixos-module.nix`](../../nix/tests/nixos-module.nix), its scenarios, evaluated in
  real NixOS systems; and [`nix/tests/vm/suites/module`](../../nix/tests/vm/suites/module/test.py),
  a Plasma session in which the module's unit runs the daemon.

[`nixos-module.nix`]: ../../nix/nixos-module.nix
[`StaticConfig`]: ../../komorebi/src/static_config.rs
[`StaticConfig::preload`]: ../../komorebi/src/static_config.rs
[`StaticConfig::postload`]: ../../komorebi/src/static_config.rs
[`StaticConfig::reload`]: ../../komorebi/src/static_config.rs
[`Workspace::load_static_config`]: ../../komorebi/src/workspace.rs
[`HOME_DIR`]: ../../komorebi/src/lib.rs
[`LAYOUT_DEFAULTS`]: ../../komorebi/src/lib.rs
[`IGNORE_IDENTIFIERS`]: ../../komorebi/src/lib.rs
[`MANAGE_IDENTIFIERS`]: ../../komorebi/src/lib.rs
[`FLOATING_APPLICATIONS`]: ../../komorebi/src/lib.rs
[`hotwatch`]: https://crates.io/crates/hotwatch
[`WindowManager::init`]: ../../komorebi/src/window_manager.rs
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
[`should_act`]: ../../komorebi/src/window.rs
[`RuleDebug`]: ../../komorebi/src/window.rs
[`GlobalState`]: ../../komorebi/src/state.rs
[`MatchingRule`]: ../../komorebi/src/core/config_generation.rs
[`ApplicationIdentifier`]: ../../komorebi/src/core/mod.rs
[`Window::set_position`]: ../../komorebi/src/window.rs
[`Window::hide`]: ../../komorebi/src/window.rs
[`Window::restore`]: ../../komorebi/src/window.rs
[`HIDDEN_WINDOWS`]: ../../komorebi/src/lib.rs
[`Container::add_window`]: ../../komorebi/src/container.rs
[`Container::hide`]: ../../komorebi/src/container.rs
[`Container::load_focused_window`]: ../../komorebi/src/container.rs
[`Container::restore`]: ../../komorebi/src/container.rs
[`Workspace::move_window_to_container`]: ../../komorebi/src/workspace.rs
[`Workspace::new_container_for_focused_window`]: ../../komorebi/src/workspace.rs
[`Workspace::remove_window`]: ../../komorebi/src/workspace.rs
[`Workspace::focus_container_by_window`]: ../../komorebi/src/workspace.rs
[`Workspace::restore`]: ../../komorebi/src/workspace.rs
[`Workspace::hide`]: ../../komorebi/src/workspace.rs
[`Workspace::window_container_behaviour`]: ../../komorebi/src/workspace.rs
[`Workspace::float_override`]: ../../komorebi/src/workspace.rs
[`WindowManager::add_window_to_container`]: ../../komorebi/src/window_manager.rs
[`WindowManager::remove_window_from_container`]: ../../komorebi/src/window_manager.rs
[`WindowManager::stack_all`]: ../../komorebi/src/window_manager.rs
[`WindowManager::unstack_all`]: ../../komorebi/src/window_manager.rs
[`WindowManager::restore_all_windows`]: ../../komorebi/src/window_manager.rs
[`WindowManager::cycle_container_window_in_direction`]: ../../komorebi/src/window_manager.rs
[`WindowManager::cycle_container_window_index_in_direction`]: ../../komorebi/src/window_manager.rs
[`WindowManager::focus_container_window`]: ../../komorebi/src/window_manager.rs
[`WindowManager::window_management_behaviour`]: ../../komorebi/src/window_manager.rs
[`Backend::hide`]: ../../komorebi/src/backend/mod.rs
[`Table::hide_commands`]: ../../komorebi/src/kwin/state.rs
[`Table::restore_commands`]: ../../komorebi/src/kwin/state.rs
[`Table::window_workspace`]: ../../komorebi/src/kwin/state.rs
[`Table::stale_park`]: ../../komorebi/src/kwin/state.rs
[`NeedsPark`]: ../../komorebi/src/kwin/state.rs
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
[`Workspace::layout_flip`]: ../../komorebi/src/workspace.rs
[`Workspace::layout_options`]: ../../komorebi/src/workspace.rs
[`Workspace::resize_dimensions`]: ../../komorebi/src/workspace.rs
[`WindowManager::resize_window`]: ../../komorebi/src/window_manager.rs
[`WindowManager::toggle_float`]: ../../komorebi/src/window_manager.rs
[`WindowManager::toggle_lock`]: ../../komorebi/src/window_manager.rs
[`WindowManager::toggle_monocle`]: ../../komorebi/src/window_manager.rs
[`WorkspaceLayer`]: ../../komorebi/src/workspace.rs
[`Workspace::layer`]: ../../komorebi/src/workspace.rs
[`Workspace::floating_layer_behaviour`]: ../../komorebi/src/workspace.rs
[`WindowManager::focus_floating_window_in_direction`]: ../../komorebi/src/window_manager.rs
[`WindowManager::focus_floating_window_in_cycle_direction`]: ../../komorebi/src/window_manager.rs
[`WindowManager::move_floating_window_in_direction`]: ../../komorebi/src/window_manager.rs
[`WindowManager::toggle_maximize`]: ../../komorebi/src/window_manager.rs
[`WindowManager::monocle_on`]: ../../komorebi/src/window_manager.rs
[`WindowManager::monocle_off`]: ../../komorebi/src/window_manager.rs
[`WindowManager::cycle_monocle`]: ../../komorebi/src/window_manager.rs
[`WindowManager::monocle_focus_behaviour`]: ../../komorebi/src/window_manager.rs
[`Workspace::monocle_container`]: ../../komorebi/src/workspace.rs
[`Workspace::new_monocle_container`]: ../../komorebi/src/workspace.rs
[`Workspace::reintegrate_monocle_container`]: ../../komorebi/src/workspace.rs
[`Workspace::maximized_window`]: ../../komorebi/src/workspace.rs
[`Workspace::new_maximized_window`]: ../../komorebi/src/workspace.rs
[`Workspace::reintegrate_maximized_window`]: ../../komorebi/src/workspace.rs
[`WindowManager::adjust_workspace_padding`]: ../../komorebi/src/window_manager.rs
[`Workspace::apply_window_based_work_area_offset`]: ../../komorebi/src/workspace.rs
[`Monitor::update_workspace_globals`]: ../../komorebi/src/monitor.rs
[`WindowManager::manage_focused_window`]: ../../komorebi/src/window_manager.rs
[`WindowManager::unmanage_focused_window`]: ../../komorebi/src/window_manager.rs
[`WindowManager::is_paused`]: ../../komorebi/src/window_manager.rs
[`WindowManager::unmanaged_window_operation_behaviour`]: ../../komorebi/src/window_manager.rs
[`WindowManager::handle_unmanaged_window_behaviour`]: ../../komorebi/src/window_manager.rs
[`WindowManagementBehaviour::toggle_float_placement`]: ../../komorebi/src/core/mod.rs
[`WindowManager::promote_container_to_front`]: ../../komorebi/src/window_manager.rs
[`WindowManager::promote_container_swap`]: ../../komorebi/src/window_manager.rs
[`WindowManager::promote_focus_to_front`]: ../../komorebi/src/window_manager.rs
[`Workspace::promotion_swap_container_idx`]: ../../komorebi/src/workspace.rs
[`WindowManager::resize_delta`]: ../../komorebi/src/window_manager.rs
[`Workspace::latest_layout`]: ../../komorebi/src/workspace.rs
[`LayoutOptions`]: ../../komorebi/src/core/mod.rs
[`Workspace::tile`]: ../../komorebi/src/workspace.rs
[`Layout`]: ../../komorebi/src/core/mod.rs
[`Container::preselect`]: ../../komorebi/src/container.rs
[`WindowManager::change_workspace_layout_default`]: ../../komorebi/src/window_manager.rs
[`WindowManager::cycle_layout`]: ../../komorebi/src/window_manager.rs
[`WindowManager::flip_layout`]: ../../komorebi/src/window_manager.rs
[`WindowManager::preselect_container_in_direction`]: ../../komorebi/src/window_manager.rs
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
[`Table::switch_commands`]: ../../komorebi/src/kwin/state.rs
[`Table::window_workspace_commands`]: ../../komorebi/src/kwin/state.rs
[`Kwinrc`]: ../../komorebi/src/kwin/kwinrc.rs
[`Backend::switch_workspace`]: ../../komorebi/src/backend/mod.rs
[`Monitor::load_focused_workspace`]: ../../komorebi/src/monitor.rs
[`WindowManager::sync_workspace_desktops`]: ../../komorebi/src/window_manager.rs
[`WindowManager::adopt_workspace_change`]: ../../komorebi/src/window_manager.rs
[`WindowManager::focus_workspace`]: ../../komorebi/src/window_manager.rs
[`WindowManager::move_container_to_workspace`]: ../../komorebi/src/window_manager.rs
[`WindowManager::monitor_workspace_index_by_name`]: ../../komorebi/src/window_manager.rs
[`WindowManager::focus_monitor`]: ../../komorebi/src/window_manager.rs
[`WindowManager::monitor_idx_in_direction`]: ../../komorebi/src/window_manager.rs
[`WindowManager::monitor_idx_from_window`]: ../../komorebi/src/window_manager.rs
[`WindowManager::move_container_to_monitor`]: ../../komorebi/src/window_manager.rs
[`WindowManager::cross_monitor_move_behaviour`]: ../../komorebi/src/window_manager.rs
[`WindowManager::cross_boundary_behaviour`]: ../../komorebi/src/window_manager.rs
[`Monitor::focus_workspace`]: ../../komorebi/src/monitor.rs
[`Monitor::move_container_to_workspace`]: ../../komorebi/src/monitor.rs
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
