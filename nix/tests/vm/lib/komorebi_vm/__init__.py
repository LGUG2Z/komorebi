"""Helpers for the NixOS VM tests of komorebi for KDE.

A suite imports what it needs from here:

* [`Checks`](komorebi_vm.core.checks.Checks) counts checks and fails at the end.
* [`Kde`](komorebi_vm.kde.Kde) is a KWin session inside a test machine, with
  the tools to look at it (`kdotool`, `busctl`, `kscreen-doctor`, screenshots)
  and the komorebi daemon that runs in it.
* [`Outputs`](komorebi_vm.kde.Outputs) are the session's outputs, which a suite
  disables, enables and scales to change the monitors under the daemon.
* [`Keys`](komorebi_vm.keys.Keys) presses keys on the machine's keyboard.
* [`daemon`](komorebi_vm.daemon) reads what the daemon logs about the events it
  handles and the frames it asks for.
* [`pixels`](komorebi_vm.core.pixels) reads a screenshot: the color of a pixel, and
  the ring around a window.
* [`latency`](komorebi_vm.latency) times the daemon's commands from a recording
  of the session bus.
* [`RdpClient`](komorebi_vm.rdp.RdpClient) logs in to KRDP from FreeRDP on an X server of its own,
  and reads its picture back.
* [`Pointer`](komorebi_vm.pointer.Pointer) moves and clicks the pointer of a
  session node, through [`uinput`](komorebi_vm.uinput), a program the node runs.

The library depends on the small [`Machine`](komorebi_vm.machine.Machine)
protocol, not on the test driver, so `nix/tests/vm/lib/tests` can run it
against a fake machine (the `vm-lib` check does).
"""

from komorebi_vm import daemon, latency, uinput
from komorebi_vm.core import pixels
from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.kde import Kde, Outputs
from komorebi_vm.keys import Keys
from komorebi_vm.machine import Machine
from komorebi_vm.pointer import Pointer
from komorebi_vm.rdp import RdpClient

__all__ = [
    "Checks",
    "Kde",
    "Keys",
    "Machine",
    "Outputs",
    "Pointer",
    "RdpClient",
    "Rect",
    "daemon",
    "latency",
    "pixels",
    "uinput",
]
