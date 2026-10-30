"""The scenario-by-port status table, from the runs themselves.

KDE prints `== [scope] module.function` before the checks of each step of a suite, and each check
as `  ok   [scope] ...`, `  FAIL ...`, `  skip ...` or `  known failure ...`; the log of a
`nix build -L` has a prefix on every line, which is dropped. kfw writes a `result.json` for each
scenario, with the passed, failed, skipped and known checks, each with its scope. This reads both
and makes one row for each scenario (the module name: `rules_exe`, `commands`) and one column for
each port, so that the table in docs/dev/BEHAVIOUR.md says what the last run said and not what
somebody wrote. Only the checks labelled `shared` are the scenario's; what a platform labelled
with its own name is an extra, and what a scenario needed to start has no label and is left out.

Only the standard library: `scripts/scenario-status` loads this file by its path.
"""

import json
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

PREFIX = re.compile(r"^[\w.-]+> ")
STEP = re.compile(r"^== \[(?P<scope>[\w-]+)\] (?P<name>[\w.]+)(?P<left_out>: left out by \w+)?\s*$")
RESULT = re.compile(r"^  (?P<kind>ok  |FAIL|skip|known failure) ")


@dataclass
class Counts:
    passed: int = 0
    failed: int = 0
    skipped: int = 0
    known: int = 0

    def add(self, other: "Counts") -> None:
        self.passed += other.passed
        self.failed += other.failed
        self.skipped += other.skipped
        self.known += other.known

    def cell(self) -> str:
        """`pass 9`, `FAIL 2 of 9`, with `, 1 known` and `, 1 skipped` after."""
        total = self.passed + self.failed
        head = f"pass {self.passed}" if not self.failed else f"FAIL {self.failed} of {total}"
        if not total and not (self.known or self.skipped):
            head = "-"
        parts = [head]
        if self.known:
            parts.append(f"{self.known} known")
        if self.skipped:
            parts.append(f"{self.skipped} skipped")
        return ", ".join(parts)


@dataclass
class Status:
    """The counts of each scenario, for each port."""

    shared: dict[str, dict[str, Counts]] = field(default_factory=dict)
    """scenario -> port -> counts, for the steps labelled `shared` (and every kfw scenario)."""
    extras: dict[str, dict[str, Counts]] = field(default_factory=dict)
    """extra -> port -> counts, for the steps a platform labelled with its own name."""

    def _counts(self, table: dict[str, dict[str, Counts]], name: str, port: str) -> Counts:
        return table.setdefault(name, {}).setdefault(port, Counts())


def read_kde(status: Status, text: str) -> None:
    """Adds the checks of a log of a run on KDE."""
    scenario: str | None = None
    table = status.shared
    for raw in text.splitlines():
        line = PREFIX.sub("", raw)
        if step := STEP.match(line):
            if step["left_out"]:
                scenario = None
                continue
            module = step["name"].split(".")[0]
            scenario = module
            table = status.shared if step["scope"] == "shared" else status.extras
            continue
        if scenario is None:
            continue
        if result := RESULT.match(line):
            counts = status._counts(table, scenario, "kde")
            kind = result["kind"].strip()
            if kind == "ok":
                counts.passed += 1
            elif kind == "FAIL":
                counts.failed += 1
            elif kind == "skip":
                counts.skipped += 1
            else:
                counts.known += 1


def read_windows(status: Status, directory: Path) -> None:
    """Adds the latest `result.json` of each scenario under `directory`, by modification time."""
    latest: dict[str, tuple[float, dict[str, Any]]] = {}
    for path in directory.rglob("result.json"):
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        name = data.get("scenario")
        if isinstance(name, str) and path.stat().st_mtime >= latest.get(name, (0.0, {}))[0]:
            latest[name] = (path.stat().st_mtime, data)
    for name, (_, data) in latest.items():
        checks = [c for c in data.get("checks", []) if isinstance(c, dict)]
        if not any(c.get("scope") for c in checks):
            # a result with no scopes: the totals are all there is
            counts = status._counts(status.shared, name, "windows")
            counts.passed = int(data.get("passed", 0))
            counts.failed = int(data.get("failed", 0))
            counts.skipped = int(data.get("skipped", 0))
            counts.known = int(data.get("known_failures", 0))
            continue
        for check in checks:
            scope = check.get("scope")
            if not scope:
                continue  # what a scenario needed to start, not what it checks
            if scope == "shared" and not name.startswith("windows_"):
                counts = status._counts(status.shared, name, "windows")
            elif scope == "shared":
                continue  # the shared steps an extra's run starts from belong to their scenarios
            else:
                counts = status._counts(status.extras, name, "windows")
            if check.get("skipped"):
                counts.skipped += 1
            elif check.get("known_failure"):
                counts.known += 1
            elif check.get("passed"):
                counts.passed += 1
            else:
                counts.failed += 1


def _cell(counts: Counts) -> str:
    return counts.cell() if counts.passed + counts.failed + counts.known + counts.skipped else "-"


def table(status: Status) -> str:
    """The Markdown tables: the shared scenarios on each port, then the extras of each."""
    ports = ("kde", "windows")
    out = ["| scenario | kde | windows |", "|---|---|---|"]
    for name in sorted(status.shared):
        cells = [status.shared[name].get(port, Counts()) for port in ports]
        out.append(
            f"| `{name}` | "
            + " | ".join(
                c.cell() if c.passed + c.failed + c.known + c.skipped else "not run" for c in cells
            )
            + " |"
        )
    out += ["", "Only on one port:", "", "| extra | kde | windows |", "|---|---|---|"]
    for name in sorted(status.extras):
        cells = [status.extras[name].get(port, Counts()) for port in ports]
        out.append(f"| `{name}` | " + " | ".join(_cell(c) for c in cells) + " |")
    return "\n".join(out) + "\n"


BEGIN = "<!-- scenario-status:begin -->"
END = "<!-- scenario-status:end -->"


def replace_region(document: str, content: str) -> str:
    """The document with what is between the two markers replaced by `content`."""
    start, end = document.index(BEGIN) + len(BEGIN), document.index(END)
    return f"{document[:start]}\n{content}{document[end:]}"
