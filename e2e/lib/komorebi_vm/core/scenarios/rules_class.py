"""Rules that match a window by its class (its app id on Wayland) decide where it goes, or that it
is left alone."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Platform
from komorebi_vm.core.scenarios.support import QUIET


def run(platform: Platform, checks: Checks) -> None:
    """Gives the daemon a workspace rule and an ignore rule by class, opens a window of each
    class and one of neither, and checks where each went.

    The daemon must be running, on the first workspace, which is the one in view, with no window,
    and with workspaces 0 and 1. The rules are cleared afterwards.
    """
    platform.komorebic("ensure-workspaces", "0", "2")
    platform.komorebic("workspace-rule", "class", "e2e-sent", "0", "1")
    platform.komorebic("ignore-rule", "class", "e2e-left")

    sent = platform.spawn("by-class-sent", app_id="e2e-sent")
    checks.check(
        "a window a class rule matches goes to the workspace the rule names",
        platform.wait_for(lambda: platform.state().workspace_of(sent.title) == 1, timeout=20),
    )
    checks.check(
        "... where it is put away while another workspace is in view",
        platform.wait_for(lambda: platform.hidden(sent)),
    )

    left = platform.spawn("by-class-left", app_id="e2e-left")
    checks.check(
        "a window an ignore rule by class matches is not managed",
        not platform.wait_for(
            lambda: platform.state().workspace_of(left.title) is not None, timeout=QUIET
        ),
    )

    plain = platform.spawn("by-class-plain", app_id="e2e-other")
    checks.check(
        "a window of another class stays on the first workspace",
        platform.wait_for(lambda: platform.state().workspace_of(plain.title) == 0, timeout=20),
    )

    platform.komorebic("clear-all-workspace-rules")
    for client in (sent, left, plain):
        platform.close(client)
    checks.check(
        "the three windows are closed",
        platform.wait_for(lambda: platform.state().managed() == 0, timeout=20),
    )
