"""komorebi-bar next to the daemon: the process, its window, its schema and what
it does to the daemon.

Windows only (the bar is built per platform), run after the tiling windows like the
other extras. It needs `komorebi-bar.exe` next to the other binaries (`BAR_BINARY`);
without it the scenario says so and fails, the others do not need it.

What it exercises is what a bar change can break at runtime and the unit tests cannot
see: the configuration is read (storage, time, applications, komorebi workspaces and
layout widgets, a `mouse` block, and the Windows-only Systray and Keyboard widgets),
the window opens, the bar keeps running while the daemon answers, it survives the daemon
going away and coming back (it reconnects: it does not exit when the daemon stops), and
`komorebic stop --bar` ends it. There are no pixel assertions; the screenshot is kept as
an artefact (`bar.png`, `bar-after-restart.png`).
"""

import hashlib
import json
import os
import time
from pathlib import Path

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client

from ..windows_platform import GUEST_BIN, WindowsPlatform

BAR_BINARY = f"{GUEST_BIN}\\komorebi-bar.exe"
BAR_CONFIG = "C:\\e2e\\bar.json"
BAR_OUTPUT = "C:\\e2e\\bar.out"
# the title eframe gives the window (`run_native("komorebi-bar", ...)`)
BAR_TITLE = "komorebi-bar"
# how long the bar must keep running, in seconds
ALIVE_SECONDS = 15

CONFIG = {
    "height": 40,
    "left_widgets": [
        {"Komorebi": {
            "workspaces": {"enable": True, "hide_empty_workspaces": False},
            "layout": {"enable": True},
        }},
        {"Applications": {
            "enable": True,
            "items": [{"name": "Notepad", "command": "notepad.exe"}],
        }},
    ],
    "right_widgets": [
        {"Storage": {"enable": True}},
        {"Time": {"enable": True, "format": "TwentyFourHour"}},
        {"Keyboard": {"enable": True}},
        {"Systray": {"enable": True}},
    ],
    # one option of each kind: a message to the daemon and a command, which starts the
    # bar's PowerShell session
    "mouse": {
        "on_extra2_click": {"message": {"type": "Retile"}},
        # `MouseMessage::Command(String)` is untagged: a plain string, not {"Command": ...}
        "on_middle_click": "echo komorebi-bar-e2e",
    },
}


def _expected_schema() -> bytes:
    """The checked-in Windows schema.bar.json: from `KOMOREBI_E2E_BAR_SCHEMA` (the nix app
    sets it), otherwise the one at the root of this checkout."""
    given = os.environ.get("KOMOREBI_E2E_BAR_SCHEMA")
    path = Path(given) if given else Path(__file__).resolve().parents[4] / "schema.bar.json"
    return path.read_bytes()


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()[:16]


def _bar_pids(platform: WindowsPlatform) -> list[str]:
    listing = platform.agent_exec(
        "cmd", "/c", 'tasklist /fi "imagename eq komorebi-bar.exe" /fo csv /nh'
    )["stdout"]
    return [
        line.split(",")[1].strip('"')
        for line in listing.splitlines()
        if line.lower().startswith('"komorebi-bar.exe"')
    ]


def _bar_window(platform: WindowsPlatform) -> dict | None:
    return platform.os_window_by_title(BAR_TITLE)


def _containers(platform: WindowsPlatform) -> int:
    return len(platform.state().containers())


def _alive_for(platform: WindowsPlatform, seconds: float) -> bool:
    """The bar process runs now and at every second of `seconds`."""
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if not _bar_pids(platform):
            return False
        time.sleep(1)
    return bool(_bar_pids(platform))


def run(platform: WindowsPlatform, checks: Checks, clients: list[Client]) -> None:
    """Starts the bar on the tiled windows of `tiling` and walks through its life. The
    daemon runs at the end and the bar does not."""
    present = platform.agent_exec("cmd", "/c", f"if exist {BAR_BINARY} (echo yes) else (echo no)")
    if "yes" not in present["stdout"]:
        checks.bad(f"komorebi-bar.exe is in the guest ({BAR_BINARY}): the binaries lack it")
        return

    # the schema the bar prints is the checked-in one, byte for byte
    expected = _expected_schema()
    printed = platform.agent_exec(BAR_BINARY, "--schema")
    got = printed["stdout"].replace("\r\n", "\n").encode()
    (platform._out / "bar-schema.json").write_bytes(got)
    checks.check(
        "`komorebi-bar --schema` prints schema.bar.json "
        f"(printed {_sha(got)}, checked in {_sha(expected)})",
        printed["status"] == 0 and got == expected,
    )

    before = _containers(platform)
    local = platform._out / "bar.json"
    local.write_text(json.dumps(CONFIG, indent=2))
    platform._agent.put_file(local, BAR_CONFIG)
    platform.agent_spawn(
        "cmd", "/c", f"{BAR_BINARY} --config {BAR_CONFIG} > {BAR_OUTPUT} 2>&1"
    )

    checks.check(
        "the bar process starts", platform.wait_for(lambda: bool(_bar_pids(platform)), timeout=30)
    )
    appeared = platform.wait_for(lambda: _bar_window(platform) is not None, timeout=60)
    checks.check(f"the bar's window titled {BAR_TITLE!r} appears", appeared)
    window = _bar_window(platform)
    if window is not None:
        frame = window["frame"]
        checks.check(
            "the bar's window is visible and has a size",
            bool(window["visible"])
            and frame["right"] - frame["left"] > 0
            and frame["bottom"] - frame["top"] > 0,
        )
    time.sleep(3)
    try:
        platform.screenshot("bar")
    except Exception as e:
        print(f"     no screenshot: {e}")

    checks.check(
        f"the bar keeps running for {ALIVE_SECONDS} s with this configuration (every widget)",
        _alive_for(platform, ALIVE_SECONDS * platform._scale),
    )
    checks.check(
        "`komorebic state` answers while the bar runs",
        platform.wait_for(lambda: platform.komorebic_status("state")[0] == 0),
    )
    checks.check(
        "komorebi does not tile the bar (it ignores komorebi-bar.exe)",
        _containers(platform) == before,
    )
    # a command that changes the state reaches the bar through its subscription; the bar
    # has no way to be asked, so it is that nothing breaks
    platform.komorebic("toggle-pause")
    platform.komorebic("toggle-pause")
    checks.check(
        "the daemon answers commands that notify the bar, and the bar runs on",
        platform.komorebic_status("state")[0] == 0 and bool(_bar_pids(platform)),
    )

    # the daemon stops and comes back: the bar reconnects, it does not exit
    pids = _bar_pids(platform)
    platform.komorebic("stop")
    platform.wait_for(lambda: platform.komorebic_status("state")[0] != 0, timeout=20)
    checks.check(
        "the bar is still running after the daemon stopped (it waits to reconnect)",
        _alive_for(platform, 5 * platform._scale),
    )
    platform.start_komorebi()
    checks.check(
        "the bar runs on after the daemon is back, the same process",
        _alive_for(platform, 5 * platform._scale) and _bar_pids(platform) == pids,
    )
    try:
        platform.screenshot("bar-after-restart")
    except Exception as e:
        print(f"     no screenshot: {e}")

    # what stops the bar is `komorebic stop --bar`
    platform.komorebic("stop", "--bar")
    checks.check(
        "`komorebic stop --bar` stops the daemon",
        platform.wait_for(lambda: platform.komorebic_status("state")[0] != 0, timeout=20),
    )
    checks.check(
        "`komorebic stop --bar` ends the bar process",
        platform.wait_for(lambda: not _bar_pids(platform), timeout=20),
    )
    try:
        text = platform.agent_get_file(BAR_OUTPUT).decode(errors="replace")
        (platform._out / "bar.out").write_text(text)
    except Exception as e:
        print(f"     no bar output: {e}")
    # left as the other scenarios leave it: the daemon runs
    platform.start_komorebi()
