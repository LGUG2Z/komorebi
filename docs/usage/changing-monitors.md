# Monitors that change

komorebi follows the outputs KWin has. There is nothing to configure and no command to run: this
page says what komorebi does when a monitor changes.

## A monitor changes size

A monitor changes size when its resolution or its scale changes (in System Settings, under
Display & Monitor, or with `kscreen-doctor output.<name>.scale.1.5`), and when a panel changes
the room the windows have on it: a panel that starts, grows, moves or is hidden. komorebi reads
the size and the work area of every monitor again, and tiles the workspace in front on the
focused monitor in the area it now has.

The windows of the other monitors keep their tiles until something tiles them again.
[`komorebic retile`](../cli/retile.md) does it for every monitor.

KWin has no signal for the room a panel leaves, so komorebi looks at it when a panel opens,
closes or moves, and once a second. A panel that is laid out a moment after komorebi starts is
found within a second.

A monitor that is unplugged or plugged in is not followed: komorebi keeps the monitors it
started with until it is started again.

## Following it from a program

A [subscriber](subscribing-to-notifications.md) is sent a notification each time komorebi has
read the monitors again, whether or not anything changed in the state. Its `event` is
`{"Resize":0}`, where the number is the id of the monitor that changed, or 0 when KWin does not
say which.
