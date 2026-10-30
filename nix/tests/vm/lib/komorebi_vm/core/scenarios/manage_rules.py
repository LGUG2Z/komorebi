"""A manage rule has a window managed that an ignore rule leaves alone, by title, class and exe."""

from typing import Any

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import QUIET


def _kinds(platform: Platform) -> list[tuple[str, str, dict[str, Any]]]:
    """For each kind of rule: its name, the id of the rule, and what the window is opened with."""
    return [
        ("title", "e2e-manage-title", {}),
        ("class", "e2e-manage-class", {"app_id": "e2e-manage-class"}),
        ("exe", platform.exe_id("e2e-manage-exe"), {"exe": "e2e-manage-exe"}),
    ]


def run(platform: Platform, checks: Checks) -> None:
    """For each kind of rule, an ignore rule leaves a window alone and a manage rule for the same
    application then has the next one managed and tiled.

    A window the daemon leaves alone does not make room for itself: the window that is tiled keeps
    its frame. The daemon must be running, with no window, and the rules are for names no other
    scenario uses, because `ignore-rule` drops a rule whose id an earlier rule has.
    """
    base = platform.spawn("manage-base")
    platform.wait_for(lambda: platform.state().workspace_of(base.title) == 0, timeout=20)
    for kind, rule_id, how in _kinds(platform):
        title = rule_id if kind == "title" else f"manage-{kind}"
        platform.komorebic("ignore-rule", kind, rule_id)
        before = platform.os_window(base)
        ignored = platform.spawn(f"{title} ignored", **how)
        checks.check(
            f"a window an ignore rule by {kind} matches is not managed",
            not platform.wait_for(
                lambda c=ignored: platform.state().workspace_of(c.title) is not None,
                timeout=QUIET,
            ),
        )
        after = platform.os_window(base)
        checks.check(
            f"... and the window that is tiled keeps its place ({kind})",
            before is not None and after is not None and before.frame == after.frame,
        )
        platform.close(ignored)
        platform.wait_for(lambda c=ignored: platform.os_window(c) is None, timeout=20)

        platform.komorebic("manage-rule", kind, rule_id)
        managed: Client = platform.spawn(f"{title} managed", **how)
        checks.check(
            f"with a manage rule by {kind} too, a window it matches is managed",
            platform.wait_for(
                lambda c=managed: platform.state().workspace_of(c.title) == 0, timeout=20
            ),
        )
        checks.check(
            f"... and tiled: {base.title} gives it room ({kind})",
            platform.wait_for(
                lambda b=before: (
                    (a := platform.os_window(base)) is not None
                    and b is not None
                    and a.frame != b.frame
                ),
                timeout=20,
            ),
        )
        platform.close(managed)
        platform.wait_for(lambda c=managed: platform.state().workspace_of(c.title) is None)
    platform.close(base)
    platform.wait_for(lambda: platform.state().managed() == 0, timeout=20)
