"""komorebi-pointer: an absolute pointer device made with uinput, for the session node.

The session node installs this file as the program `komorebi-pointer`, and
[`Pointer`](komorebi_vm.pointer.Pointer) runs it there as the test user (who is in the group
`uinput`). `komorebi-pointer WIDTH HEIGHT` creates a device whose two axes span a screen of that
size, prints `ready`, and reads one command per line on its standard input:

* `move X Y` puts the pointer on the pixel (X, Y);
* `down` and `up` press and release the left button;
* `glide X Y STEPS MS` takes the pointer from where it is to (X, Y) in STEPS moves, MS
  milliseconds apart, without a round trip to the test between two of them.

It prints `ok` once it has written a command's events, and `bad COMMAND` for a line it does not
understand. KWin takes the device as a tablet: a position is absolute, so it does not depend on
pointer acceleration.

The test driver's machine imports this module too, for the parts that do not touch a device, which
the unit tests in `nix/tests/vm/lib/tests` check.
"""

import fcntl
import os
import struct
import sys
import time
from collections.abc import Callable

UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_SET_ABSBIT = 0x40045567
UI_DEV_SETUP = 0x405C5503
UI_ABS_SETUP = 0x401C5504
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502

EV_SYN = 0
EV_KEY = 1
EV_ABS = 3
ABS_X = 0
ABS_Y = 1
BTN_LEFT = 0x110

# the largest value of an axis; 0 is the first pixel and MAX the end of the last one
MAX = 65535

Event = tuple[int, int, int]


def axis(pixel: int, pixels: int) -> int:
    """The value of an axis that is `pixels` long for the middle of the pixel `pixel`.

    The middle, and not the edge, so that KWin's rounding of the position back to a pixel lands on
    that pixel: a value at the edge of two pixels comes back as either.
    """
    return ((2 * pixel + 1) * (MAX + 1)) // (2 * pixels)


def path(start: tuple[int, int], end: tuple[int, int], steps: int) -> list[tuple[int, int]]:
    """The points a glide from `start` to `end` in `steps` moves goes through, `end` the last."""
    (x1, y1), (x2, y2) = start, end
    return [
        (x1 + round((x2 - x1) * i / steps), y1 + round((y2 - y1) * i / steps))
        for i in range(1, steps + 1)
    ]


class Device:
    """What a command turns into: input events, written by `write` and followed by a sync."""

    def __init__(
        self,
        width: int,
        height: int,
        write: Callable[[list[Event]], None],
        sleep: Callable[[float], None] = time.sleep,
    ) -> None:
        self.width = width
        self.height = height
        self.write = write
        self.sleep = sleep
        self.at = (width // 2, height // 2)

    def move(self, x: int, y: int) -> None:
        """Puts the pointer on the pixel (x, y)."""
        self.write(
            [
                (EV_ABS, ABS_X, axis(x, self.width)),
                (EV_ABS, ABS_Y, axis(y, self.height)),
                (EV_SYN, 0, 0),
            ]
        )
        self.at = (x, y)

    def button(self, down: bool) -> None:
        """Presses the left button, or lets go of it."""
        self.write([(EV_KEY, BTN_LEFT, 1 if down else 0), (EV_SYN, 0, 0)])

    def glide(self, x: int, y: int, steps: int, ms: int) -> None:
        """Takes the pointer from where it is to (x, y) in `steps` moves, `ms` apart."""
        for point in path(self.at, (x, y), max(steps, 1)):
            self.sleep(ms / 1000)
            self.move(*point)

    def command(self, line: str) -> bool:
        """Carries out one line of input; returns false if it is not a command."""
        words = line.split()
        try:
            match words:
                case ["move", x, y]:
                    self.move(int(x), int(y))
                case ["down"]:
                    self.button(True)
                case ["up"]:
                    self.button(False)
                case ["glide", x, y, steps, ms]:
                    self.glide(int(x), int(y), int(steps), int(ms))
                case _:
                    return False
        except ValueError:
            return False
        return True


def pack(events: list[Event]) -> bytes:
    """The events as the kernel reads them: a `struct input_event` each, with no time."""
    return b"".join(struct.pack("llHHi", 0, 0, kind, code, value) for kind, code, value in events)


def create(fd: int) -> None:
    """Makes `fd`, an open `/dev/uinput`, an absolute pointer with a left button."""
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
    fcntl.ioctl(fd, UI_SET_KEYBIT, BTN_LEFT)
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_ABS)
    for code in (ABS_X, ABS_Y):
        fcntl.ioctl(fd, UI_SET_ABSBIT, code)
        fcntl.ioctl(fd, UI_ABS_SETUP, struct.pack("H2xiiiiii", code, 0, 0, MAX, 0, 0, 0))
    setup = struct.pack("HHHH80sI", 0x03, 0x1209, 0x4B4D, 1, b"komorebi-pointer", 0)
    fcntl.ioctl(fd, UI_DEV_SETUP, setup)
    fcntl.ioctl(fd, UI_DEV_CREATE)


def main() -> None:
    width, height = int(sys.argv[1]), int(sys.argv[2])
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    create(fd)
    # the compositor needs a moment to open a new device; events before that are lost
    time.sleep(1.5)

    def write(events: list[Event]) -> None:
        os.write(fd, pack(events))

    device = Device(width, height, write)
    print("ready", flush=True)
    for line in sys.stdin:
        if not line.strip():
            continue
        print("ok" if device.command(line) else f"bad {line.strip()}", flush=True)
    fcntl.ioctl(fd, UI_DEV_DESTROY)


if __name__ == "__main__":
    main()
