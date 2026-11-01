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
import re
import time
from pathlib import Path

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client

from ..windows_platform import GUEST_BIN, WindowsPlatform

BAR_BINARY = f"{GUEST_BIN}\\komorebi-bar.exe"
BAR_CONFIG = "C:\\e2e\\bar.json"
BAR_OUTPUT = "C:\\e2e\\bar.out"
# the title eframe gives the window (`run_native("komorebi-bar", ...)`)
ANSI = re.compile(r"\x1b\[[0-9;]*m")
BAR_LOG = "info,wgpu_hal=debug,wgpu_core=warn"
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


# what the bar prints when glow cannot make its context (komorebi-bar's `run_native` error), as the gate
# printed it: "egui_glow requires opengl 2.0+". The bar then tries wgpu (Direct3D 12, WARP) and, when that
# fails too, ends with "glow: <error>; wgpu: <error>".
NO_OPENGL_TEXT = "requires opengl"


def classify(output: str) -> str | None:
    """What the bar's own output says about its start: "no_renderer" (neither glow nor wgpu
    could make a window: no OpenGL 2.0 and no usable Direct3D 12 adapter), "failed" (it ended with any other error: a configuration it
    could not read, a crash) or None (no error, it runs)."""
    if "Error:" not in output and "panicked" not in output:
        return None
    if NO_OPENGL_TEXT in output.lower():
        return "no_renderer"
    return "failed"


def _renderer_errors(output: str) -> str:
    """The line that names what glow and wgpu said, for a skip."""
    for line in output.splitlines():
        if "wgpu:" in line and "glow:" in line:
            return line.strip()
    for line in output.splitlines():
        if NO_OPENGL_TEXT in line.lower():
            return f"glow: {line.strip()}; (no wgpu attempt: an older bar)"
    return "no renderer"


def _bar_output(platform: WindowsPlatform) -> str:
    try:
        text = platform.agent_get_file(BAR_OUTPUT).decode(errors="replace")
    except Exception:
        return ""
    return ANSI.sub("", text)


def _started(platform: WindowsPlatform) -> bool:
    """The bar runs, or it has already ended with something to say."""
    return bool(_bar_pids(platform)) or classify(_bar_output(platform)) is not None


def _outcome(platform: WindowsPlatform) -> str:
    """Waits for the bar's window or its end: "running" (the window is there), "no_renderer",
    "failed", or "timeout" (neither within a minute)."""
    deadline = time.monotonic() + 60 * platform._scale
    while time.monotonic() < deadline:
        if _bar_window(platform) is not None:
            return "running"
        if not _bar_pids(platform):
            why = classify(_bar_output(platform))
            if why is not None:
                return why
        time.sleep(1)
    return classify(_bar_output(platform)) or "timeout"


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
    # the log of the first run keeps what wgpu says about its adapters
    platform.agent_spawn(
        "cmd", "/c",
        f"set RUST_LOG={BAR_LOG}&& {BAR_BINARY} --config {BAR_CONFIG} > {BAR_OUTPUT} 2>&1",
    )

    checks.check("the bar process starts", platform.wait_for(lambda: _started(platform), timeout=30))
    outcome = _outcome(platform)
    text = _bar_output(platform)
    (platform._out / "bar.out").write_text(text)
    # the configuration is parsed before the window is made, so a bar that got as far as
    # opening a window (or failing to) has read all of it, the Keyboard and Systray
    # widgets and the mouse block included
    checks.check(
        "the bar reads its configuration (every widget, the mouse block)",
        outcome != "failed" and "found configuration file" in text,
    )

    if outcome == "no_renderer":
        # the bar tried glow and then wgpu and neither could make a window: not what a change of the
        # bar did, and not a pass
        reason = f"guest cannot draw: {_renderer_errors(text)}"
        for what in (
            f"the bar's window titled {BAR_TITLE!r} appears",
            f"the bar keeps running for {ALIVE_SECONDS} s",
            "the bar is still running after the daemon stopped",
            "the bar runs on after the daemon is back",
        ):
            checks.skip(what, reason)
        checks.check(
            "`komorebic state` answers after the bar has started",
            platform.wait_for(lambda: platform.komorebic_status("state")[0] == 0),
        )
        platform.komorebic("stop", "--bar")
        checks.check(
            "`komorebic stop --bar` stops the daemon",
            platform.wait_for(lambda: platform.komorebic_status("state")[0] != 0, timeout=20),
        )
        platform.start_komorebi()
        return
    if outcome != "running":
        checks.bad(f"the bar did not open its window ({outcome}); it printed: {text[:600]!r}")
        platform.komorebic("stop", "--bar")
        platform.start_komorebi()
        return

    used = "wgpu" if "trying wgpu" in text else "glow"
    print(f"     the bar draws with {used}")
    window = _bar_window(platform)
    checks.check(f"the bar's window titled {BAR_TITLE!r} appears", window is not None)
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
    # left as the other scenarios leave it: the daemon runs
    platform.start_komorebi()
