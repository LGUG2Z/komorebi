import pytest

from komorebi_vm import Keys

from .conftest import FakeMachine


def test_a_combo_is_sent_as_qemu_names_it(machine: FakeMachine) -> None:
    keys = Keys(machine)

    keys.send("alt-h")
    keys.send("Meta-Shift-j")
    keys.send("super-1")
    keys.send("esc")

    assert machine.keys == ["alt-h", "meta_l-shift-j", "meta_l-1", "esc"]


def test_a_word_that_is_not_a_modifier_is_refused(machine: FakeMachine) -> None:
    with pytest.raises(ValueError, match="'hyper' in 'hyper-h' is not a modifier"):
        Keys(machine).send("hyper-h")
    with pytest.raises(ValueError, match="has no key"):
        Keys(machine).send("alt-")
    assert machine.keys == []
