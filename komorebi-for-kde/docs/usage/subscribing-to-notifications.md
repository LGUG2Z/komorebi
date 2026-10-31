# Subscribing to notifications

A program such as a status bar, a widget or a script can follow what komorebi does by
subscribing to its notifications. After each event the window manager handles (a window
was shown, closed, focused, minimized, restored or moved to another output), each message it
takes on its socket and each change of the monitors, the daemon sends every subscriber a
notification: the event, the message or the change, and the state of the window manager after it.

## Subscribing

A subscriber listens on a Unix socket of its own in the daemon's data directory,
`$XDG_RUNTIME_DIR/komorebi` (on NixOS and most systemd distributions,
`/run/user/<uid>/komorebi`), and tells the daemon the name of the socket with
[`komorebic subscribe-socket`](../cli/subscribe-socket.md). The socket has to exist before
it subscribes: the daemon sends the notification of the subscription at once, and removes a
subscriber it cannot connect to.

With [socat](http://www.dest-unreach.org/socat/), a shell can subscribe and print every
notification:

```sh
socat -u UNIX-LISTEN:"$XDG_RUNTIME_DIR/komorebi/bar.sock",fork STDOUT &
komorebic subscribe-socket bar.sock
```

[`komorebic unsubscribe-socket bar.sock`](../cli/unsubscribe-socket.md) stops the
notifications.

## Notifications

The daemon sends each notification on a new connection to the subscriber's socket: it
connects, writes one JSON object, and closes the connection. There is no newline after the
object. The first notification a subscriber receives is the one of its subscription.

A notification has two fields:

- `event`: the event, the message or the change of the monitors. An event is a window event,
  with the signal it came from and the window, such as
  `{"type":"Show","content":[{"type":"KWin","content":"WindowAdded"},{"id":...}]}`. A
  message is written the way a client writes it on the daemon's socket, such as
  `{"type":"Retile"}`. A change of the monitors is the name of the change with its content:
  `{"Resize":0}` for a monitor that changed size, with the id of the monitor, or 0 when KWin
  does not say which, and `{"DisplayConnectionChange":0}` for a monitor that was unplugged or
  plugged in, and `"Wake"` for the machine waking up or the screen being unlocked.
  [Changing monitors](changing-monitors.md) and [Sleeping and locking](sleeping-and-locking.md)
  say when each is sent.
- `state`: the state of the window manager after it, with the same fields as what
  [`komorebic state`](../cli/state.md) prints, on one line.

An event about a window that komorebi does not manage, and a message that fails, are not
sent.

## Only when something changed

A subscriber can ask to be sent a notification only when the event or the message changed
the state, with the `filter_state_changes` option. It is still sent the notification of a
window that is shown, of every change of the monitors, of every new subscription, so it
receives the state as soon as it subscribes, of a theme, which changes the colors of the
borders and nothing in the state, and of every reload of `komorebi.json`.
`komorebic subscribe-socket` has no option for it; a program writes the message
on the daemon's socket itself:

```sh
printf '%s' '{"type":"AddSubscriberSocketWithOptions","content":["bar.sock",{"filter_state_changes":true}]}' |
  socat - UNIX-CONNECT:"$XDG_RUNTIME_DIR/komorebi/komorebi.sock"
```

## When a subscriber goes away

A subscriber that the daemon cannot connect to, because its program has exited or its
socket was removed, is removed at the next notification, and its socket file with it. The
daemon does not try it again: a program that wants notifications again subscribes again.
The daemon keeps its subscribers only while it runs, so after a restart every program has
to subscribe again.

When the daemon stops, it connects to each subscriber and closes the connection without
writing anything.

A subscriber should accept and read every connection as it comes. The daemon writes each
notification while the window manager is locked, and waits when the socket's buffer is
full, so a subscriber that stops reading can hold up the window manager. While it is held
up, the window events KWin reports wait for it, and once 20 are waiting the daemon drops
the ones that come after them.

## In Rust

The `komorebi-client` crate has `subscribe` and `subscribe_with_options`, which listen on
the socket, subscribe, and return the listener: each connection on it is one
`Notification`. `cargo doc --package komorebi-client --open` shows their documentation,
with an example.
