"""Key presses on the test machine's keyboard, as a person types them."""

from komorebi_vm.machine import Machine

# QEMU names the left Super key `meta_l`; the others are named as KDE writes them
_MODIFIERS = {
    "alt": "alt",
    "ctrl": "ctrl",
    "shift": "shift",
    "meta": "meta_l",
    "super": "meta_l",
}


class Keys:
    """The keyboard of a test machine.

    A key press goes through QEMU's virtual keyboard (`machine.send_key`), so it
    reaches the guest's kernel as a key of a real keyboard and KWin sees it as
    it sees any other: a global shortcut fires on it, and a key that is not one
    goes to the focused window. QEMU holds the keys for 100 ms, less than the
    delay before a key repeats.

    A combo is written as in KDE, in lower case, with `-` between the keys:
    `alt-h`, `meta-shift-j`, `esc`. The key names after the modifiers are
    QEMU's (`h`, `1`, `esc`, `ret`, `bracket_left`).
    """

    def __init__(self, machine: Machine) -> None:
        self.machine = machine

    @staticmethod
    def qemu(combo: str) -> str:
        """Returns `combo` as QEMU's `sendkey` writes it.

        A modifier is any of `alt`, `ctrl`, `shift`, `meta` and `super`; the
        last word is the key, which QEMU must know.
        """
        *modifiers, key = combo.lower().split("-")
        for modifier in modifiers:
            if modifier not in _MODIFIERS:
                raise ValueError(f"{modifier!r} in {combo!r} is not a modifier")
        if not key:
            raise ValueError(f"{combo!r} has no key")
        return "-".join([*(_MODIFIERS[m] for m in modifiers), key])

    def send(self, combo: str) -> None:
        """Presses the keys of `combo` together, and releases them."""
        self.machine.send_key(self.qemu(combo))
