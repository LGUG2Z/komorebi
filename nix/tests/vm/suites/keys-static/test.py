# Static shortcuts in a real Plasma session: what `komorebic hotkeys compile` and
# `komorebic export-kde-shortcuts` write is run by KWin itself, with no komorebi
# process. The Rust tests check the files; only a KWin can say that it reads them
# and runs their commands on a key: the files in the system profile when the
# session starts, and a file written later once `--reload` has handed it over.
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


print(kde.sh("ls /run/current-system/sw/share/kglobalaccel"))
checks.check("Alt+J runs its command", press("alt-j", "keys-static-alt-j"))
checks.check(
    "Alt+Shift+K runs a command that needs a shell",
    press("alt-shift-k", "keys-static-alt-shift-k"),
)
checks.check("Alt+P runs a command with a %", press("alt-p", "keys-static-alt-p"))
date = machine.succeed("cat /tmp/keys-static-alt-p").strip()
checks.check(f"the % reached the command as one % ({date!r})", date.isdigit())

# a binding written in the middle of the session, into the user's directory
kde.sh("mkdir -p ~/.config/komorebi")
kde.sh("echo 'alt - u : touch /tmp/keys-static-alt-u' > ~/.config/komorebi/skhdrc")
print(kde.sh("komorebic export-kde-shortcuts --reload"))
written = "/home/komorebi/.local/share/kglobalaccel/komorebi-touch-tmp-keys-static-alt-u.desktop"
checks.check(
    "the file is in the user's kglobalaccel directory",
    machine.execute(f"test -e {written}")[0] == 0,
)
checks.check(
    "Alt+U runs its command once --reload has handed it over",
    press("alt-u", "keys-static-alt-u"),
)

checks.finish()
