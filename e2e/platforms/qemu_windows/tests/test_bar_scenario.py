"""What the `bar` scenario decides from the bar's own output, without a guest."""

from platforms.qemu_windows.extras import bar

GL = (
    "2026-10-31T07:25:02Z  INFO komorebi_bar::windows: found configuration file: C:\\e2e\\bar.json\n"
    "Error: \n   0: egui_glow: OpenGL: egui_glow requires opengl 2.0+. \n"
)
PARSE = (
    "found configuration file: C:\\e2e\\bar.json\nError: \n"
    "   0: data did not match any variant of untagged enum MouseMessage at line 59 column 3\n"
)


def test_no_renderer_is_told_from_other_errors():
    assert bar.classify(GL) == "no_renderer"


def test_any_other_error_is_a_failure():
    assert bar.classify(PARSE) == "failed"
    assert bar.classify("thread 'main' panicked at x") == "failed"


def test_a_bar_that_prints_no_error_runs():
    assert bar.classify("") is None
    assert bar.classify("found configuration file: x\nwatching configuration file\n") is None


def test_the_test_configuration_is_plain_json_values():
    # the untagged MouseMessage::Command is a string
    assert isinstance(bar.CONFIG["mouse"]["on_middle_click"], str)


BOTH = (
    "INFO komorebi_bar::windows: renderer: glow\n"
    " WARN komorebi_bar::windows: renderer: glow failed (egui_glow: OpenGL: egui_glow requires opengl 2.0+. ); trying wgpu\n"
    "Error: glow: egui_glow: OpenGL: egui_glow requires opengl 2.0+. ; wgpu: Failed to find a suitable adapter\n"
)


def test_both_renderers_failing_is_reported_with_both_errors():
    assert bar.classify(BOTH) == "no_renderer"
    line = bar._renderer_errors(BOTH)
    assert "glow:" in line and "wgpu:" in line


def test_a_bar_that_fell_back_to_wgpu_and_runs_is_not_an_error():
    # the warning about glow is not an "Error:" line
    ran = BOTH.splitlines()[0] + "\n" + BOTH.splitlines()[1] + "\nINFO wgpu_hal: adapter WARP\n"
    assert bar.classify(ran) is None
