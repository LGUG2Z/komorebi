# Design

This document explains why komorebi for KDE is built the way it is, one decision at a time.
It is opinionated: these are the reasons behind the code, and reasonable people may disagree
with them. Where a reason was not written down, the entry says that it needs one.

It grows with the code: a decision is added in the commit that makes it.

## What is komorebi for KDE the port of?

komorebi has a Windows implementation, the original, and a macOS one. The Windows one is
the reference. I used it as the reference for what to adapt, and I prefer what KDE and
Wayland do natively over a workaround that the other two platforms need. The events the
window manager reacts to follow the Windows ones (a window is shown, destroyed, focused,
minimized, moved, resized), and a window is a small `Copy` handle, as there. The model the
window manager tiles is the one both platforms have: monitors, each with workspaces, each
with containers, each with windows.

## Why is everything the window manager needs behind one trait?

So that the window manager can be tested without a compositor. A test that needs a real
KWin has to boot a virtual machine, which takes a minute or more and needs QEMU with a
hypervisor ([vm-tests.md](vm-tests.md)); a test against an in-memory desktop takes
milliseconds and runs anywhere. The logic (which window goes where, what happens when one
closes, how a stack cycles) is tested with the second kind, and the VM tests are kept for
what only KWin can tell.

The cost is that a second implementation has to be kept honest. A fake that does what it
is told is not KWin: a real client may refuse a size, a compositor may report an event the
fake never raises, and a fake that is too permissive hides a bug that a compositor would
show. The fake is therefore written to be strict where KWin is (a command about a window
that no longer exists is an error) and to clamp where a real client does (a window with a
minimum size keeps to it).

## Why is the backend a process-wide singleton?

So that a window can be a small `Copy` value that carries only its id. A window that
had to hold a reference to its backend would not be `Copy`, and every function that
moves, focuses or hides one would take a context argument as well. That is a lot of
plumbing for a thing that never changes while the daemon runs.

The cost is a global. A test that wants a private desktop overrides it for its own thread
with `with_backend`, which restores the previous one afterwards, also on a panic.

## Why does a backend answer from a table instead of asking the compositor?

A query that waits for the compositor can wait for a long time, and the window manager
asks a lot of questions: every move of a container is followed by a look at where each
window ended up. A backend keeps a table that the compositor's events update, so a query
is a lookup. The trade-off is that the table can be a moment behind the compositor, which
is why a command returns when it has been accepted and not when the client has acted, and
why the frame a window really has is read back afterwards.
