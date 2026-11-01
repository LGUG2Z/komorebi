r"""How long a retile takes to reach the windows of a real KWin, measured on the session bus: from
the moment the daemon answers the bridge's held call with a batch of new frames, to the last
frame KWin reports for one of the windows it moved, the client's answer included. Every number
here comes from KWin and real clients; the Rust tests cannot measure it. The output is large, so
that ten windows in komorebi's default layout all have room.
"""

from typing import Any

from komorebi_vm import Checks, Kde, daemon, latency

W, H = 3840, 2160
WINDOWS = 10
ROUNDS = 10
BUS_LOG = "/tmp/komorebi-bus.log"


def run(machine: Any, checks: Checks, scale: float) -> None:
    """Runs the checks of the suite `latency`, which the suite script calls."""
    SCALE = scale  # noqa: N806
    kde = Kde.headless(machine, "main", size=(W, H), scale=SCALE)
    kde.start_komorebi()
    checks.check(
        "the daemon loaded its script into KWin",
        kde.wait_for(lambda: "loaded the KWin bridge script" in kde.komorebi_log(), timeout=30),
    )
    kde.monitor_bridge(BUS_LOG)

    titles = [f"w{n}" for n in range(1, WINDOWS + 1)]
    for title in titles:
        kde.spawn(f"foot -T {title} sleep 3000")
        kde.wait_for(lambda title=title: kde.wid(title) != "", timeout=30)

    def shown() -> int:
        return sum(1 for e in daemon.events(kde.komorebi_log()) if e.kind == "Show")

    checks.check(f"the daemon tiles all {WINDOWS}", kde.wait_for(lambda: shown() >= WINDOWS))

    def measured() -> list[latency.Round]:
        return latency.rounds(machine.succeed(f"cat {BUS_LOG}"))

    def settled_after(count: int) -> bool:
        """A round after the first `count` is over: its windows all reported, and then 0.3 s passed."""
        done = measured()
        if len(done) <= count or done[-1].seconds is None:
            return False
        now = float(machine.succeed("date +%s.%N"))
        return now - (done[-1].sent + done[-1].seconds) > 0.3

    kde.wait_for(lambda: settled_after(0), timeout=10)
    start = len(measured())

    # each round takes one window out of the layout and puts it back: two retiles
    for title in titles[:ROUNDS]:
        wid = kde.wid(title)
        count = len(measured())
        kde.minimize(wid)
        kde.wait_for(lambda count=count: settled_after(count), timeout=10)
        count = len(measured())
        kde.activate(wid)
        kde.wait_for(lambda count=count: settled_after(count), timeout=10)

    # the rounds before `start` tiled the windows as they opened
    rounds = measured()[start:]
    settled = [r.seconds * 1000 for r in rounds if r.seconds is not None]
    moved = sum(r.windows for r in rounds) / max(1, len(rounds))
    print(
        f"     {len(rounds)} retiles, {moved:.1f} windows moved by each on average, scale {SCALE}"
    )
    if settled:
        p50, p95 = latency.percentile(settled, 0.5), latency.percentile(settled, 0.95)
        print(f"     from the batch to the last frame: p50 {p50:.1f} ms, p95 {p95:.1f} ms,")
        print(f"     max {max(settled):.1f} ms, min {min(settled):.1f} ms")
    checks.check(f"{2 * ROUNDS} retiles were measured", len(rounds) == 2 * ROUNDS)
    checks.check("every window of every retile reported its frame", len(settled) == len(rounds))
    checks.check(
        "a retile reaches the windows within 100 ms (times the machine's scale), at p95",
        bool(settled) and latency.percentile(settled, 0.95) <= 100 * SCALE,
    )

    kde.stop_komorebi()
    kde.stop()
