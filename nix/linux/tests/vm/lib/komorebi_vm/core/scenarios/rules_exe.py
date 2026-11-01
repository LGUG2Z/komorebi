"""Rules that match a window by the program that opened it decide where it goes, or that it is
left alone. The program is `e2e-exe-sent.exe` on Windows and `e2e-exe-sent` on Linux, and the
rule's id is `platform.exe_id(program)`.

The names differ from those of `rules_class`: `ignore-rule` drops a rule whose id an earlier
rule has, whatever its kind."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Platform
from komorebi_vm.core.scenarios.support import QUIET


def run(platform: Platform, checks: Checks) -> None:
    """Gives the daemon a workspace rule and an ignore rule by exe, opens a window of each
    program and one of neither, and checks where each went.

    The daemon must be running, on the first workspace, which is the one in view, with no window,
    and with workspaces 0 and 1. The rules are cleared afterwards.
    """
    platform.komorebic("ensure-workspaces", "0", "2")
    platform.komorebic("workspace-rule", "exe", platform.exe_id("e2e-exe-sent"), "0", "1")
    platform.komorebic("ignore-rule", "exe", platform.exe_id("e2e-exe-left"))

    sent = platform.spawn("by-exe-sent", exe="e2e-exe-sent")
    checks.check(
        "a window an exe rule matches goes to the workspace the rule names",
        platform.wait_for(lambda: platform.state().workspace_of(sent.title) == 1, timeout=20),
    )
    checks.check(
        "... where it is put away while another workspace is in view",
        platform.wait_for(lambda: platform.hidden(sent)),
    )

    left = platform.spawn("by-exe-left", exe="e2e-exe-left")
    checks.check(
        "a window an ignore rule by exe matches is not managed",
        not platform.wait_for(
            lambda: platform.state().workspace_of(left.title) is not None, timeout=QUIET
        ),
    )

    plain = platform.spawn("by-exe-plain", exe="e2e-exe-other")
    checks.check(
        "a window of another program stays on the first workspace",
        platform.wait_for(lambda: platform.state().workspace_of(plain.title) == 0, timeout=20),
    )

    platform.komorebic("clear-all-workspace-rules")
    for client in (sent, left, plain):
        platform.close(client)
    checks.check(
        "the three windows are closed",
        platform.wait_for(lambda: platform.state().managed() == 0, timeout=20),
    )
