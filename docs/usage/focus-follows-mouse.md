# Focus follows mouse

With focus follows mouse, the window under the pointer is the window that has the focus: moving the
pointer into another window gives that window the focus, with no click. komorebi does not do this
itself. It asks KWin to, by changing KWin's own focus policy, and follows where the focus goes like
it follows any other change of focus.

## Turning it on and off

`komorebic focus-follows-mouse` turns it on or off, and `komorebic toggle-focus-follows-mouse`
switches it. It is off by default: KWin's focus policy is the user's own until komorebi changes
it.

```sh
komorebic focus-follows-mouse enable
komorebic focus-follows-mouse disable
komorebic toggle-focus-follows-mouse
```

Both take `--implementation`, `windows` (the default) or `komorebi`, the two that komorebi for
Windows has. On KDE they are the same: both ask KWin for its own focus policy, and which one was
asked for is only remembered, in the `focus_follows_mouse` of the daemon's state.

## In komorebi.json

The `focus_follows_mouse` key of [`komorebi.json`](configuration.md#settings-of-the-daemon) is
`"Windows"` or `"Komorebi"` to turn it on, which is what `--implementation` names, and has no value
or is left out to leave it off:

```json
{
  "focus_follows_mouse": "Windows"
}
```

The file sets it when the daemon starts and when the file changes, and a file that leaves it out
turns it off and gives the user's focus policy back, also when it was turned on with a command.

## What it does to KWin

Turning it on sets two keys in the group `[Windows]` of the user's `kwinrc`, `FocusPolicy=1`
("focus follows mouse") and `DelayFocusInterval=0`, and asks KWin to read its configuration
again. Turning it off, and exiting komorebi, put back what the user had: the value each key
had before komorebi changed it, or no value if the key was not there. A daemon that was killed
leaves the keys changed until the next run, whose exit puts them back.

## The pointer follows the focus too

[`komorebic mouse-follows-focus`](../cli/mouse-follows-focus.md) is the other direction: the pointer
goes to the middle of the window that is given the focus. With both on, the pointer and the focus
stay in the same window.
