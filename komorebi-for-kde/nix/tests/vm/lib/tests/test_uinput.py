import struct

from komorebi_vm import uinput
from komorebi_vm.uinput import ABS_X, ABS_Y, BTN_LEFT, EV_ABS, EV_KEY, EV_SYN, MAX, Device


def test_every_pixel_is_sent_as_a_value_that_comes_back_as_that_pixel() -> None:
    for pixels in (800, 1080, 1280, 1920):
        for pixel in range(pixels):
            value = uinput.axis(pixel, pixels)
            assert 0 <= value <= MAX
            assert value * pixels // (MAX + 1) == pixel, (pixel, pixels, value)


def test_a_glide_goes_in_even_steps_and_ends_where_it_was_asked() -> None:
    assert uinput.path((0, 0), (100, 50), 4) == [(25, 12), (50, 25), (75, 38), (100, 50)]
    assert uinput.path((10, 10), (10, 10), 3) == [(10, 10), (10, 10), (10, 10)]
    assert uinput.path((5, 5), (-5, 9), 1) == [(-5, 9)]


def recorder() -> tuple[Device, list[list[tuple[int, int, int]]], list[float]]:
    writes: list[list[tuple[int, int, int]]] = []
    sleeps: list[float] = []
    device = Device(1280, 800, writes.append, sleeps.append)
    return device, writes, sleeps


def test_a_move_is_both_axes_and_a_sync() -> None:
    device, writes, _ = recorder()

    assert device.command("move 0 799")

    assert writes == [
        [
            (EV_ABS, ABS_X, uinput.axis(0, 1280)),
            (EV_ABS, ABS_Y, uinput.axis(799, 800)),
            (EV_SYN, 0, 0),
        ]
    ]
    assert device.at == (0, 799)


def test_down_and_up_press_and_release_the_left_button() -> None:
    device, writes, _ = recorder()

    assert device.command("down")
    assert device.command("up")

    assert writes == [
        [(EV_KEY, BTN_LEFT, 1), (EV_SYN, 0, 0)],
        [(EV_KEY, BTN_LEFT, 0), (EV_SYN, 0, 0)],
    ]


def test_a_glide_starts_where_the_pointer_is_and_waits_between_its_moves() -> None:
    device, writes, sleeps = recorder()
    device.command("move 100 100")
    writes.clear()

    assert device.command("glide 200 140 4 30")

    assert [(w[0][2], w[1][2]) for w in writes] == [
        (uinput.axis(x, 1280), uinput.axis(y, 800))
        for x, y in [(125, 110), (150, 120), (175, 130), (200, 140)]
    ]
    assert sleeps == [0.03] * 4
    assert device.at == (200, 140)


def test_a_line_that_is_not_a_command_does_nothing() -> None:
    device, writes, _ = recorder()

    for line in ("teleport 1 2", "move 1", "move one two", "glide 1 2 3", ""):
        assert not device.command(line), line

    assert writes == []


def test_an_event_is_packed_as_the_kernel_reads_it() -> None:
    packed = uinput.pack([(EV_KEY, BTN_LEFT, 1), (EV_SYN, 0, 0)])

    size = struct.calcsize("llHHi")
    assert len(packed) == 2 * size
    assert struct.unpack("llHHi", packed[:size]) == (0, 0, EV_KEY, BTN_LEFT, 1)
