"""Helpers for the NixOS VM tests of komorebi for KDE.

A suite imports what it needs from here:

* [`Checks`](komorebi_vm.checks.Checks) counts checks and fails at the end.
* [`Kde`](komorebi_vm.kde.Kde) is a KWin session inside a test machine, with
  the tools to look at it (`kdotool`, `busctl`, `kscreen-doctor`, screenshots)
  and the komorebi daemon that runs in it.
* [`daemon`](komorebi_vm.daemon) reads what the daemon logs about the events it
  handles.

The library depends on the small [`Machine`](komorebi_vm.machine.Machine)
protocol, not on the test driver, so `nix/tests/vm/lib/tests` can run it
against a fake machine (the `vm-lib` check does).
"""

from komorebi_vm import daemon
from komorebi_vm.checks import Checks
from komorebi_vm.kde import Kde, Rect
from komorebi_vm.machine import Machine

__all__ = ["Checks", "Kde", "Machine", "Rect", "daemon"]
