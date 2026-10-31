"""No extra repeats a check a shared scenario makes.

The text of every `checks.check(...)` (and `ok`, `bad`, `skip`, `known_failure`) is read from the
sources of the shared scenarios and of the platform extras. A check made with an f-string counts
with each `{...}` as a gap, so that `f"{layout}: ok"` is `"{}: ok"`. An extra that says what a
shared scenario already says is a duplicate: it should be one of them, or say something else.
"""

import ast
from pathlib import Path
from types import ModuleType

import komorebi_vm.core.scenarios as scenarios
import komorebi_vm.platforms.kde.extras as kde_extras

CHECKS = {"check", "ok", "bad", "skip", "known_failure"}


def _text(node: ast.expr) -> str | None:
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    if isinstance(node, ast.JoinedStr):
        return "".join(
            str(part.value) if isinstance(part, ast.Constant) else "{}" for part in node.values
        )
    return None


def _checks(path: Path) -> dict[str, int]:
    """The text of each check in the file, and the line it is on."""
    found: dict[str, int] = {}
    for node in ast.walk(ast.parse(path.read_text())):
        if not (isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)):
            continue
        owner = node.func.value
        named = node.func.attr in CHECKS and isinstance(owner, ast.Name) and owner.id == "checks"
        if named and node.args and (text := _text(node.args[0])) is not None:
            found[text] = node.lineno
    return found


def _directory(package: ModuleType) -> Path:
    assert package.__file__ is not None
    return Path(package.__file__).parent


# words that are the same in two places by chance, not by repeating a check
GENERIC = {"and back"}

# The checks of `stacks.py` that say what the shared `stacks` scenario says. The extra reads the
# parking desktop, the shared one the operating system's `hidden`: see docs/dev/suite-ledger.md. A
# new repeat is not allowed; one in this list goes when the shared scenario reports what it leaves
# to be read.
KNOWN_REPEATS = {
    ("stacks.py", "there are three tiles"),
    ("stacks.py", "two containers, one of them a stack of two"),
    ("stacks.py", "cycling back, the first is in view again"),
    ("stacks.py", "three tiles again"),
    ("stacks.py", "and they tile, not on each other"),
    ("stacks.py", "the window in view has the focus"),
}


def test_no_extra_repeats_a_check_of_a_shared_scenario() -> None:
    shared: dict[str, str] = {}
    for path in sorted(_directory(scenarios).glob("*.py")):
        for text in _checks(path):
            shared.setdefault(text, path.name)
    repeats = []
    for path in sorted(_directory(kde_extras).glob("*.py")):
        for text, line in _checks(path).items():
            if text in shared and text not in GENERIC and (path.name, text) not in KNOWN_REPEATS:
                repeats.append(f"{path.name}:{line} says {text!r}, as {shared[text]} does")
    assert not repeats, "\n".join(repeats)
