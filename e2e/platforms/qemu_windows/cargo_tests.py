"""Runs the cargo tests of the workspace in the Windows guest.

usage: python -m platforms.qemu_windows.cargo_tests GOLDEN OUT STATE MESSAGES

MESSAGES is the `--message-format json` output of
`cargo test --workspace --no-run` for the Windows target, which names every
test executable. Each is uploaded to the guest, asked for its tests, and every
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

GUEST_DIR = "C:\\e2e\\tests"


def executables(messages: Path) -> list[tuple[str, Path]]:
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


def main(argv: list[str]) -> int:
    golden, out, state, messages = (Path(a) for a in argv[:4])
    out.mkdir(parents=True, exist_ok=True)
    results: dict[str, dict] = {}
    with Guest(golden, out / "guest", state=state if str(state) != "-" else None) as guest:
        guest.agent.exec("cmd", "/c", f"mkdir {GUEST_DIR} 2>nul")
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
                        "output": (run["stdout"] + run["stderr"])[-1500:],
                    })
            results[name] = {
                "tests": len(names), "passed": passed, "failed": failed,
                "seconds": round(time.monotonic() - began, 1),
            }
            print(f"{name}: {passed} of {len(names)} passed"
                  f"{'' if not failed else f', {len(failed)} FAILED'}"
                  f" ({results[name]['seconds']} s)", flush=True)
            for f in failed:
                first = [l for l in f["output"].splitlines() if l.strip()][-3:]
                print(f"   FAIL {f['test']} (status {f['status']}): {' | '.join(first)}", flush=True)
    (out / "cargo-tests.json").write_text(json.dumps(results, indent=2))
    total = sum(r["tests"] for r in results.values())
    failed = sum(len(r["failed"]) for r in results.values())
    print(f"all: {total - failed} of {total} passed, {failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
