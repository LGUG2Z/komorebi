"""How long the commands of the daemon take to reach the windows of KWin.

[`Kde.monitor_bridge`](komorebi_vm.kde.Kde.monitor_bridge) records the bridge's
traffic on the session bus with `dbus-monitor`: every `Event` call the script
makes, with its JSON, and every reply on the bus, among them the daemon's answers
to `Poll`, which carry the commands. Each line `dbus-monitor` prints for a message
has the time the bus delivered it to the monitor.

A **round** starts when the daemon answers `Poll` with a batch that asks for a
window to be somewhere it was not asked to be before, and ends with the last
`geometry` event KWin reports for one of those windows before the next such batch
(or within a cut-off). Its duration is how long the windows took to be where the
daemon put them, the client's answer included.
"""

import json
import math
import re
from dataclasses import dataclass
from typing import Any

from komorebi_vm.kde import Rect

_HEADER = re.compile(r"^(method call|method return|signal|error) time=(\d+\.\d+) (.*)$")
_STRING = '   string "'


@dataclass(frozen=True)
class Message:
    """One D-Bus message, as `dbus-monitor` printed it."""

    kind: str
    """`method call`, `method return`, `signal` or `error`."""
    time: float
    """When the bus delivered it, in seconds since the epoch."""
    member: str
    """The method or signal, or the empty string for a reply."""
    body: str
    """The first string argument, or the empty string."""


@dataclass(frozen=True)
class Round:
    """The windows of one batch of commands, from the batch to the last frame KWin reported."""

    sent: float
    """When the daemon's answer to `Poll` was delivered."""
    windows: int
    """How many windows the batch moved."""
    seconds: float | None
    """How long until the last of them reported its frame, or `None` if one never did."""


def messages(text: str) -> list[Message]:
    """Parses what `dbus-monitor` printed, in its default format."""
    out: list[Message] = []
    header: re.Match[str] | None = None
    body = ""

    def flush() -> None:
        if header is not None:
            fields = header.group(3)
            member = re.search(r"member=(\S+)", fields)
            out.append(
                Message(header.group(1), float(header.group(2)), member[1] if member else "", body)
            )

    for line in text.splitlines():
        if match := _HEADER.match(line):
            flush()
            header, body = match, ""
        elif header is not None and not body and line.startswith(_STRING) and line.endswith('"'):
            body = line[len(_STRING) : -1]
    flush()
    return out


def _json(text: str) -> Any:
    try:
        return json.loads(text)
    except ValueError:
        return None


def rounds(text: str, cut_off: float = 2.0) -> list[Round]:
    """Returns every round in what `dbus-monitor` printed, in order.

    A batch that asks every window for the frame it was last asked for moves
    nothing, and is not a round. A round ends at the next round, or `cut_off`
    seconds after it started, whichever comes first.
    """
    batches: list[tuple[float, dict[str, Rect]]] = []
    frames: list[tuple[float, str]] = []
    for message in messages(text):
        value = _json(message.body)
        if message.kind == "method return" and isinstance(value, list):
            geo = {
                command["id"]: Rect(command["x"], command["y"], command["w"], command["h"])
                for batch in value
                if isinstance(batch, dict)
                for command in batch.get("cmds", [])
                if command.get("op") == "geo"
            }
            if geo:
                batches.append((message.time, geo))
        elif message.member == "Event" and isinstance(value, dict):
            if value.get("type") == "geometry":
                frames.append((message.time, value["id"]))

    asked: dict[str, Rect] = {}
    moving: list[tuple[float, set[str]]] = []
    for sent, geo in batches:
        moved = {uuid for uuid, rect in geo.items() if asked.get(uuid) != rect}
        asked.update(geo)
        if moved:
            moving.append((sent, moved))

    out = []
    for i, (sent, moved) in enumerate(moving):
        end = sent + cut_off
        if i + 1 < len(moving):
            end = min(end, moving[i + 1][0])
        last = {uuid: t for t, uuid in frames if sent < t <= end and uuid in moved}
        seconds = max(last.values()) - sent if len(last) == len(moved) else None
        out.append(Round(sent, len(moved), seconds))
    return out


def percentile(values: list[float], fraction: float) -> float:
    """The value below which `fraction` of the values are (nearest rank)."""
    ordered = sorted(values)
    rank = min(len(ordered), max(1, math.ceil(fraction * len(ordered))))
    return ordered[rank - 1]
