import shlex
from pathlib import Path

import pytest

from komorebi_vm import Pointer

from .conftest import FakeMachine


class GuestMachine(FakeMachine):
    """A machine whose `komorebi-pointer` says it is ready, and answers each command it is sent."""

    def __init__(self, out_dir: Path, answer: str = "ok") -> None:
        super().__init__(out_dir)
        self.answer = answer
        self.log: list[str] = []
        self.sent: list[str] = []

    def execute(self, command: str, /) -> tuple[int, str]:
        self.commands.append(command)
        if command.startswith("runuser"):
            self.log.append("ready")
        elif command.startswith("echo "):
            words = shlex.split(command)
            self.sent.append(words[1])
            self.log.append(self.answer if self.answer == "ok" else f"bad {words[1]}")
        elif command.startswith("cat "):
            return 0, "\n".join(self.log) + "\n"
        return 0, ""


def test_start_runs_the_program_as_the_user_on_a_fifo_and_moves_to_the_middle(
    tmp_path: Path,
) -> None:
    machine = GuestMachine(tmp_path)

    Pointer.start(machine, (1280, 800))

    script = shlex.split(machine.commands[0])
    assert script[:4] == ["runuser", "-u", "komorebi", "--"]
    assert "mkfifo /run/user/1000/komorebi-pointer" in script[-1]
    assert "komorebi-pointer 1280 800 < /run/user/1000/komorebi-pointer" in script[-1]
    assert machine.sent == ["move 640 400"]


def test_a_drag_is_a_press_a_glide_and_a_release(tmp_path: Path) -> None:
    machine = GuestMachine(tmp_path)
    pointer = Pointer.start(machine, (1280, 800))

    pointer.drag((100, 200), (172, 200), steps=10, ms=20)

    assert machine.sent[1:] == ["move 100 200", "down", "glide 172 200 10 20", "up"]


def test_a_command_the_program_refuses_fails_the_test(tmp_path: Path) -> None:
    machine = GuestMachine(tmp_path)
    pointer = Pointer.start(machine, (1280, 800))
    machine.answer = "bad"

    with pytest.raises(AssertionError, match="did not do 'down'"):
        pointer.down()


def test_a_program_that_does_not_start_fails_the_test(machine: FakeMachine) -> None:
    with pytest.raises(AssertionError, match="did not start"):
        Pointer.start(machine, (1280, 800), scale=0.001)
