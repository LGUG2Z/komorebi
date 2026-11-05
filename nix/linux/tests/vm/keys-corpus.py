# vm-keys-skhd, vm-keys-whkd and vm-keys-sxhkd: the case `vm` of the key binding corpus
# (crates/komorebi-hotkeys/tests/fixtures/vm.*), in the format of the suite, installed by the NixOS module
# (`services.komorebi.keybindings`, mode auto). Alt+J can be a static shortcut, which KWin runs
# from the system profile; Alt+, cannot, so the module's daemon registers it and runs it. The Rust
# tests and `keybindings-fixtures` check the plan; only a real session can say that what the
# module builds and starts runs on a key, from each of the three formats.

from komorebi_vm import Checks
from komorebi_vm.core.suite import platform_only, run_suite
from komorebi_vm.platforms.kde.extras import keys_corpus as kde_keys_corpus

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()

run_suite(checks, platform_only("kde", kde_keys_corpus.run, machine, checks, SCALE))
checks.finish()
