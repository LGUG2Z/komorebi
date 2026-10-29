from PIL import Image, ImageDraw

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.pixels import leftmost, near, pixel, ring

BLUE = (66, 165, 245)
GRAY = (128, 128, 128)


def window_in_a_frame(frame: Rect, color: tuple[int, int, int], width: int) -> Image.Image:
    """A screen of 200x200 with a black window and a border of `width` pixels outside it."""
    image = Image.new("RGB", (200, 200), (0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw.rectangle(
        (
            frame.x - width,
            frame.y - width,
            frame.x + frame.w + width - 1,
            frame.y + frame.h + width - 1,
        ),
        fill=color,
    )
    draw.rectangle((frame.x, frame.y, frame.x + frame.w - 1, frame.y + frame.h - 1), fill=(9, 9, 9))
    return image


def test_a_pixel_is_red_green_and_blue() -> None:
    image = Image.new("RGBA", (4, 4), (1, 2, 3, 255))

    assert pixel(image, 2, 2) == (1, 2, 3)


def test_near_allows_a_difference_on_every_channel_up_to_the_tolerance() -> None:
    assert near((10, 10, 10), (22, 0, 10))
    assert not near((10, 10, 10), (23, 10, 10))
    assert near((10, 10, 10), (30, 10, 10), tolerance=20)


def test_a_border_all_round_the_window_is_a_ring() -> None:
    frame = Rect(60, 60, 80, 80)

    assert ring(window_in_a_frame(frame, BLUE, 10), frame, BLUE) == []


def test_a_side_with_no_border_is_named_with_the_pixel_it_has() -> None:
    frame = Rect(60, 60, 80, 80)
    image = window_in_a_frame(frame, BLUE, 10)
    ImageDraw.Draw(image).rectangle((130, 50, 150, 150), fill=(0, 0, 0))

    assert ring(image, frame, BLUE) == ["right: (145, 100) is (0, 0, 0), not (66, 165, 245)"]


def test_the_ring_of_another_color_is_not_the_ring_asked_for() -> None:
    frame = Rect(60, 60, 80, 80)
    image = window_in_a_frame(frame, GRAY, 10)

    assert len(ring(image, frame, BLUE)) == 4


def test_a_thin_border_is_not_found_further_out_than_it_is() -> None:
    frame = Rect(60, 60, 80, 80)
    image = window_in_a_frame(frame, BLUE, 3)

    assert len(ring(image, frame, BLUE)) == 4
    assert ring(image, frame, BLUE, margin=2) == []


def test_leftmost_is_where_a_row_first_has_the_color() -> None:
    image = Image.new("RGB", (100, 10), (0, 0, 0))
    ImageDraw.Draw(image).rectangle((30, 4, 60, 4), fill=BLUE)

    assert leftmost(image, 4, BLUE) == 30
    assert leftmost(image, 5, BLUE) is None
    assert leftmost(image, 4, GRAY) is None
