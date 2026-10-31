from pathlib import Path

from komorebi_vm.kde import Kde
from komorebi_vm.recorder import Recorder
from tests.conftest import FakeMachine


def test_a_run_without_the_recorder_does_nothing(machine: FakeMachine) -> None:
    recorder = Recorder(Kde(machine, "main"), enabled=False)
    recorder.start("x")
    recorder.stop()
    assert machine.commands == []


def test_the_recording_is_started_stopped_and_kept(machine: FakeMachine) -> None:
    machine.reply("echo $!", output="4242\n")
    machine.reply("kill -0", status=1)
    recorder = Recorder(Kde(machine, "main"), enabled=True)
    recorder.start("core")
    recorder.stop()
    joined = "\n".join(machine.commands)
    assert "komorebi-recorder record --no-video" in joined
    assert "kill -INT 4242" in joined
    assert "komorebi-recorder brief /tmp/recording-core --compact" in joined
    assert machine.copied == ["/tmp/recording-core.tar.gz"]
    assert isinstance(machine.out_dir, Path)
