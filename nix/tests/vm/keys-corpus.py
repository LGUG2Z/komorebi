# vm-keys-skhd, vm-keys-whkd and vm-keys-sxhkd: the case `vm` of the key binding corpus
# (komorebi-hotkeys/tests/fixtures/vm.*), in the format of the suite, installed by the NixOS module
# (`services.komorebi.keybindings`, mode auto). Alt+J can be a static shortcut, which KWin runs
# from the system profile; Alt+, cannot, so the module's daemon registers it and runs it. The Rust
# tests and `keybindings-fixtures` check the plan; only a real session can say that what the
# module builds and starts runs on a key, from each of the three formats.
from komorebi_vm import Checks, Kde, Keys

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)
keys = Keys(machine)


def ran(marker: str) -> bool:
    return machine.execute(f"test -e /tmp/{marker}")[0] == 0


def press(combo: str, marker: str) -> bool:
    keys.send(combo)
    return kde.wait_for(lambda: ran(marker), timeout=10)


shortcuts = machine.succeed("ls /run/current-system/sw/share/kglobalaccel")
checks.check(
    "the module installed the static shortcut in the system profile",
    "komorebi-touch-tmp-keys-corpus-alt-j.desktop" in shortcuts.split(),
)
checks.check(
    "the module's daemon registered the key of the engine, and only that one",
    kde.wait_for(lambda: kde.shortcut_actions() == ["Alt+,"], timeout=60),
)
checks.check("Alt+J runs its command, as a static shortcut", press("alt-j", "keys-corpus-alt-j"))
checks.check(
    "Alt+, runs its command, through the engine", press("alt-comma", "keys-corpus-alt-comma")
)

checks.finish()
