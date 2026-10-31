import shlex

from PIL import Image

from komorebi_vm import Kde, RdpClient
from komorebi_vm.rdp import colors, difference

from .conftest import FakeMachine


def client(machine: FakeMachine) -> RdpClient:
    return RdpClient(machine, Kde(machine, "session"))


def test_a_picture_does_not_differ_from_itself() -> None:
    picture = Image.new("RGB", (8, 8), (30, 60, 90))

    assert difference(picture, picture) == 0


def test_the_difference_is_the_mean_over_every_channel() -> None:
    black = Image.new("RGB", (8, 8), (0, 0, 0))
    grey = Image.new("RGB", (8, 8), (30, 60, 90))

    assert difference(grey, black) == 60


def test_a_picture_of_another_size_is_resized_to_the_second() -> None:
    small = Image.new("RGB", (4, 4), (30, 60, 90))
    large = Image.new("RGB", (16, 16), (30, 60, 90))

    assert difference(small, large) == 0


def test_colors_counts_the_colors_a_picture_has() -> None:
    picture = Image.new("RGB", (4, 1), (0, 0, 0))
    picture.putpixel((1, 0), (255, 0, 0))
    picture.putpixel((2, 0), (0, 255, 0))

    assert colors(picture) == 3


def test_the_client_logs_in_as_the_user_on_its_own_x_server(machine: FakeMachine) -> None:
    command = client(machine).command("s3cret", "/size:1280x800")

    words = shlex.split(command)
    assert words[:3] == ["env", "DISPLAY=:99", "xfreerdp"]
    assert {"/u:komorebi", "/p:s3cret", "/v:127.0.0.1", "/size:1280x800"} <= set(words)


def test_a_secret_with_a_space_is_one_word(machine: FakeMachine) -> None:
    words = shlex.split(client(machine).command("two words"))

    assert "/p:two words" in words


def test_logging_in_is_what_the_client_says_with_auth_only(machine: FakeMachine) -> None:
    rdp = client(machine)
    assert rdp.logs_in("right")

    machine.reply("/p:wrong", status=1)
    assert not rdp.logs_in("wrong")
    assert all("/auth-only" in command for command in machine.commands)


def test_listening_asks_the_guest_for_krdp_on_its_port(machine: FakeMachine) -> None:
    rdp = client(machine)
    assert rdp.listening()

    machine.reply("krdpserver", status=1)
    assert not rdp.listening()
    assert "sport = :3389" in machine.commands[0]


def test_the_picture_is_copied_out_of_the_x_server(machine: FakeMachine) -> None:
    picture = client(machine).picture()

    assert machine.copied == ["/tmp/rdp.ppm"]
    assert "xwd -display :99 -root" in machine.commands[0]
    assert picture.size == (4, 4)
