"""Helpers for the NixOS VM tests of komorebi for KDE.

A suite imports what it needs from here:

* [`Checks`](komorebi_vm.checks.Checks) counts checks and fails at the end.
* [`Kde`](komorebi_vm.kde.Kde) is a KWin session inside a test machine, with
  the tools to look at it (`kdotool`, `busctl`, `kscreen-doctor`, screenshots)
  and the komorebi daemon that runs in it.
* [`Outputs`](komorebi_vm.kde.Outputs) are the session's outputs, which a suite
  disables, enables and scales to change the monitors under the daemon.
* [`daemon`](komorebi_vm.daemon) reads what the daemon logs about the events it
  handles and the frames it asks for.
* [`pixels`](komorebi_vm.pixels) reads a screenshot: the color of a pixel, and
  the ring around a window.
* [`latency`](komorebi_vm.latency) times the daemon's commands from a recording
  of the session bus.
* [`Pointer`](komorebi_vm.pointer.Pointer) moves and clicks the pointer of a
  session node, through [`uinput`](komorebi_vm.uinput), a program the node runs.

The library depends on the small [`Machine`](komorebi_vm.machine.Machine)
protocol, not on the test driver, so `nix/tests/vm/lib/tests` can run it
against a fake machine (the `vm-lib` check does).
"""

from komorebi_vm import daemon, latency, pixels, uinput
from komorebi_vm.checks import Checks
from komorebi_vm.kde import Kde, Outputs, Rect
from komorebi_vm.machine import Machine
from komorebi_vm.pointer import Pointer

__all__ = [
    "Checks",
    "Kde",
    "Machine",
    "Outputs",
    "Pointer",
    "Rect",
    "daemon",
    "latency",
    "pixels",
    "uinput",
]
