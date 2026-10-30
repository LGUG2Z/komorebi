from pathlib import Path

from komorebi_vm import status

KDE_LOG = """\
vm-test-run-komorebi-x> == [kde] core_e2e.start
vm-test-run-komorebi-x>   ok   [kde] the daemon loaded its script
vm-test-run-komorebi-x> == [shared] tiling.run
vm-test-run-komorebi-x>   ok   [shared] every window is where the layout says
vm-test-run-komorebi-x>   FAIL [shared] the windows share the work area
vm-test-run-komorebi-x>    [shared] tiling.run: 3.2 s
vm-test-run-komorebi-x> == [shared] focus.run: left out by KFDE_ONLY
vm-test-run-komorebi-x>   ok   [shared] not counted: the step was left out
vm-test-run-komorebi-x> == [shared] commands.unknown_monitor
vm-test-run-komorebi-x>   known failure [shared] a window is not lost: the container is removed
vm-test-run-komorebi-x>   skip [shared] change-layout x: the port has no such layout
"""


def test_kde_counts_are_by_scenario_and_scope() -> None:
    found = status.Status()
    status.read_kde(found, KDE_LOG)
    tiling = found.shared["tiling"]["kde"]
    assert (tiling.passed, tiling.failed) == (1, 1)
    assert "focus" not in found.shared
    unknown = found.shared["commands"]["kde"]
    assert (unknown.known, unknown.skipped) == (1, 1)
    assert found.extras["core_e2e"]["kde"].passed == 1


def test_windows_counts_only_the_shared_checks_of_a_scenario(tmp_path: Path) -> None:
    (tmp_path / "a").mkdir()
    (tmp_path / "a" / "result.json").write_text(
        '{"scenario": "closing", "checks": ['
        '{"check": "setup", "passed": true},'
        '{"check": "one", "passed": true, "scope": "shared"},'
        '{"check": "two", "passed": false, "scope": "shared"},'
        '{"check": "three", "passed": true, "scope": "windows"}]}'
    )
    found = status.Status()
    status.read_windows(found, tmp_path)
    closing = found.shared["closing"]["windows"]
    assert (closing.passed, closing.failed) == (1, 1)
    assert found.extras["closing"]["windows"].passed == 1


def test_a_windows_result_without_scopes_is_its_totals(tmp_path: Path) -> None:
    (tmp_path / "result.json").write_text(
        '{"scenario": "tiling", "passed": 10, "failed": 0, "skipped": 0, "known_failures": 0}'
    )
    found = status.Status()
    status.read_windows(found, tmp_path)
    assert found.shared["tiling"]["windows"].passed == 10


def test_the_table_has_a_row_for_each_scenario_and_a_dash_for_a_port_that_did_not_run() -> None:
    found = status.Status()
    status.read_kde(found, KDE_LOG)
    out = status.table(found)
    assert "| `tiling` | FAIL 1 of 2 | not run |" in out or "| `tiling` | FAIL 1 of 2 | - |" in out


def test_the_region_between_the_markers_is_replaced() -> None:
    document = f"before\n{status.BEGIN}\nold\n{status.END}\nafter\n"
    assert status.replace_region(document, "new\n") == (
        f"before\n{status.BEGIN}\nnew\n{status.END}\nafter\n"
    )
