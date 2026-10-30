"""The daemon's state names each window as it was opened: its title, its program and its class."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Platform


def _window(platform: Platform, title: str):  # noqa: ANN202
    for workspace in platform.state().workspace().windows():
        if workspace.title == title:
            return workspace
    return None


def run(platform: Platform, checks: Checks) -> None:
    """Opens a window with only a title, one with a class, and one made by a program of its own,
    and checks what the state says of each: the title as given, the class that was set, and the
    program (through `exe_id`, which is its file name on the platform).

    The rules scenarios check that a rule finds a window by these; this checks that what the state
    shows of a window is what the window was given. The daemon must be running, with no window.
    """
    plain = platform.spawn("fidelity plain")
    classed = platform.spawn("fidelity class", app_id="e2e-fidelity-class")
    made = platform.spawn("fidelity exe", exe="e2e-fidelity-exe")
    platform.wait_for(lambda: platform.state().managed() == 3, timeout=30)

    found = _window(platform, plain.title)
    checks.check("the state has the title of a window as it was opened", found is not None)
    checks.check(
        "... and says what program opened it",
        found is not None and found.exe != "",
    )
    found = _window(platform, classed.title)
    checks.check(
        "the state has the class a window was given",
        found is not None and found.window_class == "e2e-fidelity-class",
    )
    found = _window(platform, made.title)
    checks.check(
        "the state has the program that opened a window, as its file name",
        found is not None and found.exe == platform.exe_id("e2e-fidelity-exe"),
    )
    for client in (plain, classed, made):
        platform.close(client)
    checks.check(
        "the three windows are closed",
        platform.wait_for(lambda: platform.state().managed() == 0, timeout=20),
    )
