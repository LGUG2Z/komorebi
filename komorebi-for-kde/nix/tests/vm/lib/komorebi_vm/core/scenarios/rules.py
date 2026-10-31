"""Rules that match a window by its title decide where it goes, or that it is left alone."""

import json

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Platform
from komorebi_vm.core.scenarios.support import QUIET


def _on(platform: Platform, title: str) -> int | None:
    return platform.state().workspace_of(title)


def run(platform: Platform, checks: Checks) -> None:
    """Gives the daemon four rules, opens a window that each matches, and checks where it went.

    An initial rule sends a window when it opens, so that moving it back leaves it there. A rule
    that is not initial sends it again whenever the rules are enforced, and at the next event (a
    window that opens); the same by the name of a workspace. An ignore rule leaves it alone. Once
    the rules are cleared the daemon's global state has none. The daemon must be running, on the
    first workspace, which is the one in view, with no window.
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
        not platform.wait_for(lambda: _on(platform, "sent-once") != 0, timeout=QUIET),
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

    platform.komorebic("workspace-name", "0", "2", "chat")
    platform.komorebic("named-workspace-rule", "title", "sent-named", "chat")
    named = platform.spawn("sent-named")
    checks.check(
        "a window a rule by the name of a workspace matches goes to that workspace",
        platform.wait_for(lambda: _on(platform, "sent-named") == 2, timeout=20),
    )
    platform.komorebic("focus-workspace", "2")
    # the always rule's window is on that workspace too: the focus is on the one to move
    platform.activate(named)
    platform.komorebic("move-to-workspace", "0")
    platform.wait_for(lambda: _on(platform, "sent-named") == 0, timeout=20)
    # a window that opens is an event, and a rule that is not initial is applied at every event
    poke = platform.spawn("sent-poke")
    checks.check(
        "a rule that is not initial sends the window back at the next event, a window that opens",
        platform.wait_for(lambda: _on(platform, "sent-named") == 2, timeout=30),
    )

    platform.komorebic("clear-all-workspace-rules")
    state = json.loads(platform.komorebic("global-state"))
    if "workspace_rules" in state:
        checks.check(
            "once the rules are cleared the daemon's global state has none",
            not state["workspace_rules"],
        )
    else:
        checks.skip("the global state has no rule", "the port's global state has no such field")
    platform.komorebic("focus-workspace", "0")
    # the title is the window's name here, so the first window of it goes before the next opens
    platform.close(once)
    platform.wait_for(lambda: platform.os_window(once) is None, timeout=20)
    later = platform.spawn("sent-once")
    checks.check(
        "once the rules are cleared, a window they matched stays on the workspace in view",
        platform.wait_for(lambda: _on(platform, "sent-once") == 0, timeout=20)
        and not platform.wait_for(lambda: _on(platform, "sent-once") != 0, timeout=QUIET),
    )
    for client in (always, ignored, later, named, poke):
        platform.close(client)
