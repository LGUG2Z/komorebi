"""Rules that match a window by its title decide where it goes, or that it is left alone."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Platform


def _on(platform: Platform, title: str) -> int | None:
    return platform.state().workspace_of(title)


def run(platform: Platform, checks: Checks) -> None:
    """Gives the daemon three rules, opens a window that each matches, and checks where it went.

    An initial rule sends a window when it opens, so that moving it back leaves it there. A rule
    that is not initial sends it again whenever the rules are enforced, and an ignore rule leaves
    it alone. The daemon must be running, on the first workspace, which is the one in view, with
    no window.
    """

    platform.komorebic("ensure-workspaces", "0", "3")
    platform.komorebic("initial-workspace-rule", "title", "sent-once", "0", "1")
    platform.komorebic("workspace-rule", "title", "sent-always", "0", "2")
    platform.komorebic("ignore-rule", "title", "left-alone")

    once = platform.spawn("sent-once")
    checks.check(
        "a window an initial rule matches goes to the workspace the rule names",
        platform.wait_for(lambda: _on(platform, "sent-once") == 1, timeout=20),
    )
    checks.check(
        "... where it is put away while another workspace is in view",
        platform.wait_for(lambda: platform.hidden(once)),
    )
    platform.komorebic("focus-workspace", "1")
    checks.check(
        "... and in view when its workspace is",
        platform.wait_for(lambda: not platform.hidden(once)),
    )
    platform.komorebic("move-to-workspace", "0")
    checks.check(
        "moved back to the first workspace, it is there",
        platform.wait_for(lambda: _on(platform, "sent-once") == 0, timeout=20),
    )
    # an initial rule is for the window that opens; `enforce-workspace-rules` applies it again
    # on purpose, so what is checked is that the daemon does not send the window back on its own
    checks.check(
        "... and an initial rule does not send it there again on its own",
        not platform.wait_for(lambda: _on(platform, "sent-once") != 0, timeout=3),
    )

    always = platform.spawn("sent-always")
    checks.check(
        "a window a rule that is not initial matches goes to its workspace too",
        platform.wait_for(lambda: _on(platform, "sent-always") == 2, timeout=20),
    )
    platform.komorebic("focus-workspace", "2")
    platform.komorebic("move-to-workspace", "0")
    platform.wait_for(lambda: _on(platform, "sent-always") == 0, timeout=20)
    platform.komorebic("enforce-workspace-rules")
    checks.check(
        "... and when the rules are enforced it is sent back",
        platform.wait_for(lambda: _on(platform, "sent-always") == 2, timeout=20),
    )

    managed = platform.state().managed()
    ignored = platform.spawn("left-alone")
    checks.check(
        "a window an ignore rule matches is not managed",
        platform.wait_for(lambda: platform.os_window(ignored) is not None)
        and platform.wait_for(lambda: platform.state().managed() == managed)
        and _on(platform, "left-alone") is None,
    )

    platform.komorebic("clear-all-workspace-rules")
    platform.komorebic("focus-workspace", "0")
    # the title is the window's name here, so the first window of it goes before the next opens
    platform.close(once)
    platform.wait_for(lambda: platform.os_window(once) is None, timeout=20)
    later = platform.spawn("sent-once")
    checks.check(
        "once the rules are cleared, a window they matched stays on the workspace in view",
        platform.wait_for(lambda: _on(platform, "sent-once") == 0, timeout=20)
        and not platform.wait_for(lambda: _on(platform, "sent-once") != 0, timeout=3),
    )
    for client in (always, ignored, later):
        platform.close(client)
