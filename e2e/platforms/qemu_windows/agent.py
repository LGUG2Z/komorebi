"""The host side of `komorebi-e2e-agent`: JSON lines over a forwarded port."""

import json
import socket
import time
from itertools import count
from pathlib import Path
from typing import Any


class AgentError(Exception):
    pass


class AgentLost(AgentError):
    """The connection dropped during a call: the call is not repeated, as it
    may have run, and the caller names it, so that a dead agent is not taken
    for the command having broken what it was testing."""


class Agent:
    def __init__(self, host: str = "127.0.0.1", port: int = 7878) -> None:
        self._addr = (host, port)
        self._ids = count(1)
        self._sock: socket.socket | None = None
        self._file: Any = None

    def connect(self, timeout: float = 120) -> None:
        """Waits for the guest to boot and the agent to listen."""
        deadline = time.monotonic() + timeout
        while True:
            try:
                self._sock = socket.create_connection(self._addr, timeout=5)
                self._sock.settimeout(60)
                self._file = self._sock.makefile("rwb")
                if self.call("ping") == "pong":
                    return
            except (OSError, ValueError, AgentLost):
                # QEMU accepts the forwarded connection before the guest
                # listens, then closes it: an empty answer
                if self._sock:
                    self._sock.close()
            if time.monotonic() > deadline:
                raise AgentError(f"no agent at {self._addr} after {timeout}s")
            time.sleep(1)

    def call(self, verb: str, **fields: Any) -> Any:
        request_id = next(self._ids)
        line = json.dumps({"id": request_id, "verb": verb, **fields}) + "\n"
        try:
            self._file.write(line.encode())
            self._file.flush()
            return self._read_reply(request_id)
        except (OSError, ValueError) as e:
            what = " ".join([verb, *map(str, fields.get("args", []))])
            raise AgentLost(f"agent connection lost during {what}: {e}") from e

    def _read_reply(self, request_id: int) -> Any:
        reply = json.loads(self._file.readline())
        if reply.get("id") != request_id:
            got = reply.get("id")
            raise AgentError(f"answer to {got}, expected {request_id}")
        if "error" in reply:
            raise AgentError(reply["error"])
        return reply["ok"]

    def exec(self, *args: str) -> dict[str, Any]:
        return self.call("exec", args=list(args))

    def spawn(self, *args: str) -> int:
        return self.call("spawn", args=list(args))["pid"]

    def os_windows(self) -> list[dict[str, Any]]:
        return self.call("os_windows")

    def displays(self) -> list[dict[str, Any]]:
        return self.call("displays")

    def put_file(self, local: Path, remote: str) -> None:
        data = local.read_bytes()
        request_id = next(self._ids)
        header = {
            "id": request_id,
            "verb": "put_file",
            "path": remote,
            "size": len(data),
        }
        self._file.write((json.dumps(header) + "\n").encode() + data)
        self._file.flush()
        self._read_reply(request_id)

    def get_file(self, remote: str) -> bytes:
        request_id = next(self._ids)
        header = {"id": request_id, "verb": "get_file", "path": remote}
        self._file.write((json.dumps(header) + "\n").encode())
        self._file.flush()
        size = self._read_reply(request_id)["size"]
        return self._file.read(size)

    def close(self) -> None:
        if self._sock:
            self._sock.close()
