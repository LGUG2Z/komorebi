"""Rectangles, in the pixels of a screen."""

from dataclasses import dataclass


@dataclass(frozen=True)
class Rect:
    """A window's position and size, in pixels."""

    x: int
    y: int
    w: int
    h: int

    def overlaps(self, other: "Rect") -> bool:
        """Returns true if and only if the two rectangles share at least one pixel."""
        return (
            self.x < other.x + other.w
            and other.x < self.x + self.w
            and self.y < other.y + other.h
            and other.y < self.y + self.h
        )

    def inside(self, area: "Rect", slack: int = 0) -> bool:
        """Returns true if and only if every pixel of this rectangle is a pixel of `area`, give
        or take `slack` pixels at each edge."""
        return (
            self.x >= area.x - slack
            and self.y >= area.y - slack
            and self.x + self.w <= area.x + area.w + slack
            and self.y + self.h <= area.y + area.h + slack
        )
