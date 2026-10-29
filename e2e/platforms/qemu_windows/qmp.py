"""A QMP client: the host-side hands and eyes of a QEMU guest.

Keys, pointer and screenshots go through QEMU, not through the guest, so they
need no permission in it and work at the lock screen.
"""

import json
import socket
import time
from pathlib import Path
from typing import Any

# the absolute pointer axes of QEMU's `usb-tablet` run from 0 to this
POINTER_MAX = 0x7FFF


class QmpError(Exception):
    pass


class Qmp:
    def __init__(self, path: str | Path, timeout: float = 30) -> None:
        self._sock = socket.socket(socket.AF_UNIX)
        self._sock.settimeout(timeout)
        deadline = time.monotonic() + timeout
        while True:
            try:
                self._sock.connect(str(path))
                break
            except (FileNotFoundError, ConnectionRefusedError):
                if time.monotonic() > deadline:
                    raise
                time.sleep(0.2)
        self._file = self._sock.makefile("rw", encoding="utf-8")
        self._file.readline()  # the greeting
        self.execute("qmp_capabilities")

    def execute(self, command: str, **arguments: Any) -> Any:
        request: dict[str, Any] = {"execute": command}
        if arguments:
            request["arguments"] = arguments
        self._file.write(json.dumps(request) + "\n")
        self._file.flush()
        while True:
            reply = json.loads(self._file.readline())
            if "event" in reply:
                continue
            if "error" in reply:
                raise QmpError(f"{command}: {reply['error']['desc']}")
            return reply["return"]

    def status(self) -> str:
        return self.execute("query-status")["status"]

    def screendump(self, path: str | Path, device: str | None = None) -> None:
        """Writes the display as a PNG, from the head `device` if given."""
        extra = {"device": device} if device else {}
        self.execute("screendump", filename=str(path), format="png", **extra)

    def key(self, *qcodes: str, hold_ms: int = 50) -> None:
        """Presses the keys together, then releases them.

        `key("ctrl", "alt", "delete")` is the chord.
        """
        keys = [{"type": "qcode", "data": q} for q in qcodes]
        self.execute("send-key", keys=keys, **{"hold-time": hold_ms})

    def pointer_move(self, x: int, y: int, width: int, height: int) -> None:
        """Moves the pointer to a pixel of a `width` x `height` display."""

        def axis(name: str, value: int, extent: int) -> dict[str, Any]:
            scaled = value * POINTER_MAX // max(extent - 1, 1)
            return {"type": "abs", "data": {"axis": name, "value": scaled}}

        events = [axis("x", x, width), axis("y", y, height)]
        self.execute("input-send-event", events=events)

    def pointer_button(self, down: bool, button: str = "left") -> None:
        events = [{"type": "btn", "data": {"button": button, "down": down}}]
        self.execute("input-send-event", events=events)

    def close(self) -> None:
        self._file.close()
        self._sock.close()
