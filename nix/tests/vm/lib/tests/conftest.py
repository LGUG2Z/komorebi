"""A fake machine that records commands and answers from a script."""

from collections.abc import Callable
from pathlib import Path

import pytest


class FakeMachine:
    """Answers `execute` with the first matching reply, and records every command."""

    out_dir = Path("/tmp")

    def __init__(self) -> None:
        self.commands: list[str] = []
        self.replies: list[tuple[Callable[[str], bool], tuple[int, str]]] = []

    def reply(self, contains: str, status: int = 0, output: str = "") -> None:
        """Makes any command containing `contains` answer with `status` and `output`."""
        self.replies.append((lambda command: contains in command, (status, output)))

    def execute(self, command: str, /) -> tuple[int, str]:
        self.commands.append(command)
        for matches, answer in self.replies:
            if matches(command):
                return answer
        return 0, ""

    def succeed(self, *commands: str) -> str:
        output = ""
        for command in commands:
            status, output = self.execute(command)
            assert status == 0, f"{command!r} exited with {status}: {output}"
        return output

    def fail(self, *commands: str) -> str:
        output = ""
        for command in commands:
            status, output = self.execute(command)
            assert status != 0, f"{command!r} succeeded: {output}"
        return output


@pytest.fixture
def machine() -> FakeMachine:
    return FakeMachine()
