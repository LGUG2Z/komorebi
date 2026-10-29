"""kfw watches the configuration file it was started with."""

import json

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client

from ..windows_platform import CONFIG_PATH, WindowsPlatform


def _config(name: str) -> dict:
    return {"monitors": [{"workspaces": [{"name": name, "layout": "BSP"}]}]}


def _name(platform: WindowsPlatform) -> str | None:
    return platform.state().monitors[0].workspaces[0].name


def run(platform: WindowsPlatform, checks: Checks, clients: list[Client]) -> None:
    """Starts the daemon from a file and changes the file on disk. The daemon
    runs at the end, on the new file."""
    platform.restart_with(_config("before"))
    checks.check("the daemon starts with the name in the file", _name(platform) == "before")

    changed = platform._out / "komorebi-changed.json"
    changed.write_text(json.dumps(_config("after")))
    platform._agent.put_file(changed, CONFIG_PATH)
    checks.check(
        "a changed file is read by the daemon on its own",
        platform.wait_for(lambda: _name(platform) == "after", timeout=30),
    )
    platform.komorebic("replace-configuration", CONFIG_PATH)
    checks.check(
        "`replace-configuration` reads it again and the name stays",
        platform.wait_for(lambda: _name(platform) == "after", timeout=10),
    )
