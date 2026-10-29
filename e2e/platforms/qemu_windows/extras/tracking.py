"""kfw's focus follows mouse is the setting of Windows itself."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client

from ..windows_platform import WindowsPlatform

# SPI_GETACTIVEWINDOWTRACKING
GET = (
    "Add-Type -Name S -Namespace K -MemberDefinition "
    "'[DllImport(\"user32.dll\")] public static extern bool "
    "SystemParametersInfo(uint a, uint b, ref int c, uint d);'; "
    "$v = 0; [void][K.S]::SystemParametersInfo(0x1000, 0, [ref]$v, 0); $v"
)


def _tracking(platform: WindowsPlatform) -> str:
    return platform._agent.exec("powershell", "-NoProfile", "-Command", GET)["stdout"].strip()


def run(platform: WindowsPlatform, checks: Checks, clients: list[Client]) -> None:
    """Turns focus follows mouse on and off and reads the system's active
    window tracking each time. It is left off."""
    checks.check("window tracking is off to begin with", _tracking(platform) == "0")
    platform.komorebic("focus-follows-mouse", "enable")
    checks.check(
        "`focus-follows-mouse enable` turns on the system's window tracking",
        platform.wait_for(lambda: _tracking(platform) == "1"),
    )
    platform.komorebic("focus-follows-mouse", "disable")
    checks.check(
        "`focus-follows-mouse disable` turns it off",
        platform.wait_for(lambda: _tracking(platform) == "0"),
    )
