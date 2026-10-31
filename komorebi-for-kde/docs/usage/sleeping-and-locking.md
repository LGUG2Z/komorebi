# Sleeping and locking

komorebi looks at the desktop again when the machine wakes up and when the screen is unlocked.
There is nothing to configure and no command to run.

## Why

While the screen is locked or the machine is asleep, KWin sends its clients no frame callbacks,
so the windows do not draw and nothing komorebi asked for in that time is done: a window moved
by something else stays where it was moved, and a window that was asked to move does not. The
monitors can come back in another order, or at another size, or not all of them. A window that
is where komorebi did not put it is not in its tile.

## What komorebi does

- **The screen is unlocked.** The lock screen is a window of KWin's, on every desktop and in no
  task list, and komorebi does not manage windows like that: it has no tile, and nothing is
  moved about while the screen is locked. Locking the screen does nothing. A moment after it is
  unlocked (0.6 seconds: the lock screen goes before the windows can be seen), komorebi looks
  at the desktop.
- **The machine wakes up.** komorebi hears it from logind (`PrepareForSleep`, which is false when
  the machine is awake). Going to sleep does nothing. A moment after it wakes (2.5 seconds: the
  outputs are found again one after another, and the compositor moves windows about as they
  are), komorebi looks at the desktop.

Looking at the desktop is the same for both. komorebi reads the monitors again, as when one is
unplugged or plugged in ([Changing monitors](changing-monitors.md)), reads the size and work
area of every monitor, finds every window that is on another monitor than the one it has it on,
and tiles every monitor. Nothing is done while komorebi is paused
([`komorebic toggle-pause`](../cli/toggle-pause.md)).

The bridge compares the window KWin has active with the one it last told komorebi about once a
second, because the window that is active again after an unlock is not always announced, and
komorebi's idea of the focus follows KWin's.

## What is not tested

A real suspend and a real resume are not tested: the virtual machines the tests run in have no
way to wake from one. The signals komorebi waits for are tested on private buses, and the lock
screen against a real KWin.

## Following it from a program

A [subscriber](subscribing-to-notifications.md) is sent a notification, whose `event` is `"Wake"`,
when komorebi has looked at the desktop after a wake or an unlock.
