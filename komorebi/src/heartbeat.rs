//! Evidence of a stall of the whole daemon.
//!
//! A thread that sleeps 100 ms in a loop and says so when it wakes up much later than that. If
//! it is late in the same moments in which every other thread of the daemon is silent, the
//! process (or the guest it runs in) was not scheduled; if only some threads are silent, one of
//! them holds something (see [`crate::lock_holder`]).

use std::time::Duration;
use std::time::Instant;

/// How long the heartbeat thread sleeps between two beats
const BEAT: Duration = Duration::from_millis(100);

/// A beat later than this after the one before is logged
const LATE: Duration = Duration::from_millis(500);

/// Starts the heartbeat thread
pub fn start() {
    let _ = std::thread::Builder::new()
        .name("heartbeat".to_string())
        .spawn(|| {
            let mut last = Instant::now();

            loop {
                std::thread::sleep(BEAT);

                let now = Instant::now();
                let late = now.duration_since(last).saturating_sub(BEAT);
                last = now;

                if late >= LATE {
                    tracing::warn!(
                        "heartbeat was {} ms late: the daemon was not scheduled for that long",
                        late.as_millis()
                    );
                }
            }
        });
}
