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

## Why does komorebi not pick the next window to focus when one closes?

Because the compositor already has. When a window is destroyed, KWin activates the window
it considers next and reports that as a focus change, which komorebi follows. If komorebi
also focused a window of its own choosing, the two would fight over the focus. komorebi
focuses the window of its focused container only when the compositor has left nothing
focused.

## Why does komorebi run a script inside KWin?

KWin has no protocol that lets another process read and set window geometry. Its scripting
API has both, and a signal for each change komorebi follows: a window added, removed,
activated, minimized, moved or resized. So the part of the backend that sees and touches
windows is a script KWin runs, the bridge, and the daemon talks to it over the session bus.

## Why does the bridge hold no logic?

So that everything the backend decides can be tested without KWin. A script runs only inside
KWin: its own tests run it against a fake of KWin's API, which is only as right as the fake,
and a test of it against KWin needs a virtual machine. The bridge reports what KWin says, as
it says it, and carries out what it is told; what an event means (is this a window komorebi
manages, has it been announced already, which id does it have) is decided by the table in
`kwin::state`, which is plain Rust that takes the bridge's JSON and is unit-tested with it.

## Why is the bridge type-checked and tested without a JavaScript build chain?

I wanted safety, tests and typing around the bridge that do not need a JavaScript build
chain. KWin loads `bridge.js` as it is, so the checks read that file and produce nothing:

- `tsc` checks it as strict TypeScript through its JSDoc comments (`checkJs`, `noEmit`),
  against `kwin-api.d.ts`, a hand-written declaration of the part of KWin's scripting API the
  bridge uses. KWin publishes no declarations, and the declarations are narrower than the API
  where that catches a mistake: a property the bridge has no reason to set is read-only, and
  `callDBus` takes one string.
- `bridge.test.js` runs under `node --test`, node's own test runner, and loads the bridge into
  a `node:vm` context whose globals are a fake KWin.
- The messages of a session are JSON files that the bridge's tests and the Rust tests both
  read, so neither side can change a message alone.

`tsc` and `node` come from nixpkgs, through the dev shell and the flake's
`komorebi-bridge-js` check. There is no package.json, no npm dependency and no bundler.

## Why do commands go to the bridge as the answer to a held call?

Because it was the fastest channel measured, and it costs nothing while nothing happens. Four
ways for the daemon to get commands to a script in KWin were measured in October 2026, on
KWin 6.8.1 in a VM under HVF: twenty windows retiled in one batch, twenty times, timed from
the send until every window reported its new frame.

| Channel | p50 / p95 / max |
|---|---|
| a held `Poll` call, answered when there are commands | 16 / 19 / 20 ms |
| the script polls on a 16 ms timer | 16 / 20 / 32 ms |
| the script polls on a 50 ms timer | 49 / 57 / 65 ms |
| a new script per batch (`loadScript`, `run`, `unloadScript`) | 17 / 18 / 20 ms |

A timer is as fast only at about 60 D-Bus calls a second, forever. A new script per batch is
as fast, but 7 of its 17 ms (p50) are spent in the daemon's calls to KWin. A held call is
answered as soon as there is a batch, and an idle bridge makes one call every 10 seconds.
Rounds after idle gaps of 15 seconds to 10 minutes all completed, the slowest in 49 ms.

The hold is 10 seconds because QtDBus fails a call after 25 seconds. A call that fails is
never answered (the callback of `callDBus` runs on success only), so the bridge has a
one-second watchdog that calls `Poll` again when the last call is older than the hold plus 3
seconds, which is also how it finds a daemon that has restarted.

## Why are events and the held call on two D-Bus objects?

Because events must arrive in order and a held call waits. zbus handles each method call in a
task of its own by default, and two pairs of events were seen swapped. The `Event` interface
is therefore `spawn = false`: zbus handles its calls inline, one after the other, in the
order they arrive. The price is that a handler that waits holds up every call after it, so
`Event` never waits, and `Poll`, which waits by design, is a separate object with the default
spawning.

## Why does everything cross D-Bus as a JSON string?

Because a script cannot choose the D-Bus type of what it sends. `callDBus` sends a JavaScript
number as a 32-bit integer, and a call whose arguments do not match the method's signature
fails with no error the script can see. One string argument always matches, and the daemon
parses it. The declaration of `callDBus` the bridge is type-checked against takes one string,
so `tsc` refuses anything else.
