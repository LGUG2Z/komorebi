"""Runs the cargo tests of the workspace in the Windows guest.

usage: python -m platforms.qemu_windows.cargo_tests GOLDEN OUT STATE TESTS

TESTS is a directory with a `tests.tsv` (name, tab, file under `tests/`: the
output of the flake check `windows-nextest-build`), or the
`--message-format json` output of `cargo test --workspace --no-run` for the
Windows target, which names every test executable. STATE is a directory to
restore the guest from (made if missing, as `KOMOREBI_E2E_STATE` does for the
scenarios), or `-` to boot. Each is uploaded to the guest, asked for its tests, and every
test is run in a process of its own (as nextest does: komorebi's tests share
global state), with its stdout kept. Prints a line per executable and the
tests that failed; OUT/cargo-tests.json has every result. Exits 1 if any test
failed or an executable could not be run.
"""

import json
import sys
import time
from pathlib import Path

from .guest import Guest
from .state import ensure

GUEST_DIR = "C:\\e2e\\tests"


def executables(messages: Path) -> list[tuple[str, Path]]:
    if messages.is_dir():
        listing = (messages / "tests.tsv").read_text().splitlines()
        return sorted(
            (name, messages / "tests" / file)
            for name, file in (line.split("\t") for line in listing)
        )
    found = {}
    for line in messages.read_text().splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("executable") and message.get("profile", {}).get("test"):
            kinds = ",".join(message["target"]["kind"])
            found[f"{message['target']['name']}-{kinds}"] = Path(message["executable"])
    return sorted(found.items())


def tests_of(guest: Guest, exe: str) -> list[str]:
    listing = guest.agent.exec(exe, "--list", "--format", "terse")
    return [
        line[: -len(": test")]
        for line in listing["stdout"].splitlines()
        if line.endswith(": test")
    ]


def upload_data(guest: Guest, tests: Path) -> None:
    """Puts the data files the tests read (the schemas, fixtures) in the guest
    at the path the build baked into the executables (CARGO_MANIFEST_DIR, the
    build directory of the check, as a path on the guest's C: drive)."""
    if not (tests.is_dir() and (tests / "srcroot").exists()):
        return
    root = "C:" + (tests / "srcroot").read_text().strip().replace("/", "\\")
    data = tests / "data"
    files = sorted(p for p in data.rglob("*") if p.is_file())
    made: set[str] = set()
    for path in files:
        remote = root + "\\" + str(path.relative_to(data)).replace("/", "\\")
        directory = remote.rsplit("\\", 1)[0]
        if directory not in made:
            guest.agent.exec("cmd", "/c", f"mkdir {directory} 2>nul")
            made.add(directory)
        guest.agent.put_file(path, remote)
    print(f"data: {len(files)} files under {root}", flush=True)


def reason(output: str) -> str:
    """The lines that say why a test failed: the panic message, else the end."""
    lines = [l for l in output.splitlines() if l.strip()]
    for i, line in enumerate(lines):
        if "panicked at" in line:
            return " | ".join(lines[i : i + 2])
    return " | ".join(lines[-3:])


def main(argv: list[str]) -> int:
    golden, out, state, messages = (Path(a) for a in argv[:4])
    out.mkdir(parents=True, exist_ok=True)
    saved = ensure(golden, state) if str(state) != "-" else None
    results: dict[str, dict] = {}
    with Guest(golden, out / "guest", state=saved) as guest:
        guest.agent.exec("cmd", "/c", f"mkdir {GUEST_DIR} 2>nul")
        upload_data(guest, messages)
        for name, path in executables(messages):
            exe = f"{GUEST_DIR}\\{name}.exe"
            began = time.monotonic()
            guest.agent.put_file(path, exe)
            names = tests_of(guest, exe)
            passed, failed = 0, []
            for test in names:
                run = guest.agent.exec(exe, "--exact", test, "--test-threads", "1")
                if run["status"] == 0:
                    passed += 1
                else:
                    failed.append({
                        "test": test, "status": run["status"],
                        "output": (run["stdout"] + run["stderr"])[-4000:],
                    })
            results[name] = {
                "tests": len(names), "passed": passed, "failed": failed,
                "seconds": round(time.monotonic() - began, 1),
            }
            print(f"{name}: {passed} of {len(names)} passed"
                  f"{'' if not failed else f', {len(failed)} FAILED'}"
                  f" ({results[name]['seconds']} s)", flush=True)
            for f in failed:
                print(f"   FAIL {f['test']} (status {f['status']}): {reason(f['output'])}", flush=True)
    (out / "cargo-tests.json").write_text(json.dumps(results, indent=2))
    total = sum(r["tests"] for r in results.values())
    failed = sum(len(r["failed"]) for r in results.values())
    print(f"all: {total - failed} of {total} passed, {failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
