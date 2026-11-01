"""The pointer of a session node, moved and clicked from inside the guest."""

import shlex
import time
from collections.abc import Callable

from komorebi_vm.machine import Machine


class Pointer:
    """A pointer device in the guest, which [`komorebi_vm.uinput`][] makes with uinput.

    [`Pointer.start`][] runs `komorebi-pointer` as the test user, reading its commands from a
    FIFO, and every method writes one command to it and waits for the program to say it is done.
    A drag is one press, one glide and one release: the glide's moves are made in the guest, a few
    milliseconds apart, because moves that each wait for a round trip to the test are seconds
    apart, and KWin was seen to drop the horizontal part of such a drag.

    Coordinates are pixels of the screen whose size the pointer was started with. KWin puts the
    pointer within a pixel of where it is asked.
    """

    def __init__(self, machine: Machine, *, uid: int = 1000, scale: float = 1.0) -> None:
        self.machine = machine
        # In the user's runtime directory, which is the user's: a FIFO of the user's in `/tmp`,
        # which everybody may write to, is one root may not open for writing (`protected_fifos`).
        self.fifo = f"/run/user/{uid}/komorebi-pointer"
        self.log = f"/run/user/{uid}/komorebi-pointer.log"
        self.scale = scale
        self.done = 0

    @classmethod
    def start(
        cls,
        machine: Machine,
        size: tuple[int, int],
        *,
        user: str = "komorebi",
        uid: int = 1000,
        scale: float = 1.0,
    ) -> "Pointer":
        """Starts the device for a screen of `size` and waits until KWin can have it.

        The device starts with a move to the middle of the screen, since the first move after a
        device is created was seen to be lost.
        """
        pointer = cls(machine, uid=uid, scale=scale)
        width, height = size
        f, lg = shlex.quote(pointer.fifo), shlex.quote(pointer.log)
        # The FIFO is kept open for writing by a `tail` that never writes, so that the program
        # does not see the end of its input between two commands.
        script = (
            f"rm -f {f} {lg} && mkfifo {f} && "
            f"(setsid nohup tail -f /dev/null > {f} 2>/dev/null < /dev/null &) && "
            f"(setsid nohup komorebi-pointer {width} {height} < {f} > {lg} 2>&1 &)"
        )
        machine.succeed(f"runuser -u {user} -- sh -c {shlex.quote(script)}")
        if not pointer._wait(lambda: pointer._lines().count("ready") == 1, timeout=30):
            raise AssertionError(f"komorebi-pointer did not start: {pointer._lines()}")
        pointer.move(width // 2, height // 2)
        return pointer

    def move(self, x: int, y: int) -> None:
        """Puts the pointer on the pixel (x, y)."""
        self._send(f"move {x} {y}")

    def down(self) -> None:
        """Presses the left button."""
        self._send("down")

    def up(self) -> None:
        """Lets go of the left button."""
        self._send("up")

    def glide(self, x: int, y: int, steps: int = 25, ms: int = 30) -> None:
        """Takes the pointer from where it is to (x, y) in `steps` moves, `ms` milliseconds apart.

        It returns once the pointer is there.
        """
        self._send(f"glide {x} {y} {steps} {ms}")

    def drag(
        self,
        start: tuple[int, int],
        end: tuple[int, int],
        steps: int = 25,
        ms: int = 30,
    ) -> None:
        """Presses the left button at `start`, glides to `end` with it held, and lets go there."""
        self.move(*start)
        self.down()
        self.glide(*end, steps=steps, ms=ms)
        self.up()

    def _send(self, command: str) -> None:
        self.machine.succeed(f"echo {shlex.quote(command)} > {shlex.quote(self.fifo)}")
        self.done += 1

        def answered() -> bool:
            lines = self._lines()
            return lines.count("ok") >= self.done or any(line.startswith("bad ") for line in lines)

        if not self._wait(answered, timeout=30) or self._lines().count("ok") < self.done:
            raise AssertionError(f"komorebi-pointer did not do {command!r}: {self._lines()}")

    def _lines(self) -> list[str]:
        return self.machine.execute(f"cat {shlex.quote(self.log)}")[1].splitlines()

    def _wait(self, predicate: Callable[[], bool], timeout: float) -> bool:
        deadline = time.monotonic() + timeout * self.scale
        while not predicate():
            if time.monotonic() >= deadline:
                return False
            time.sleep(0.05)
        return True
