//! Hearing about sleep, wake and the session lock from D-Bus as they happen.
//!
//! logind announces that the machine is about to sleep, and that it woke, with
//! `PrepareForSleep` on the system bus, and the screen saver says that the session was locked or
//! unlocked with `ActiveChanged` on the session bus (KDE answers on `org.freedesktop.ScreenSaver`,
//! which komorebi for KDE listens to as well). A thread for each reads its signals, stamps each
//! with a clock anchor on arrival and hands it to the recording loop over a channel, which drains
//! it with [`Watcher::drain`].
//!
//! | D-Bus says | becomes |
//! |---|---|
//! | `PrepareForSleep(true)` | `Suspend` |
//! | `PrepareForSleep(false)` | `Resume` |
//! | `ActiveChanged(true)` | `SessionLocked` |
//! | `ActiveChanged(false)` | `SessionUnlocked` |
//!
//! The lid and the displays' power are not read: the displays are polled, as on a Mac. A bus that
//! cannot be reached (no session bus in a bare shell) is left out, and the other is still heard.
//! The threads end when the next signal finds the watcher gone.

use crate::clock::Anchor;
use crate::power::PowerEvent;
use crate::power::PowerKind;
use color_eyre::eyre;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use zbus::blocking::Connection;
use zbus::blocking::Proxy;

/// Listens for power events until dropped, and holds what it heard until it is drained.
pub struct Watcher {
    events: Receiver<PowerEvent>,
}

impl Watcher {
    /// Returns what has been heard since the last call, oldest first. Each [`PowerEvent`] carries
    /// the clock [`Anchor`] sampled when the signal arrived.
    #[must_use]
    pub fn drain(&self) -> Vec<PowerEvent> {
        self.events.try_iter().collect()
    }
}

/// Reads the boolean argument of each signal `name` that `interface` sends at `path`, and sends
/// the [`PowerKind`] that `kind` makes of it, until the receiver is gone.
fn listen(
    connection: Connection,
    destination: &'static str,
    path: &'static str,
    interface: &'static str,
    name: &'static str,
    kind: fn(bool) -> PowerKind,
    events: Sender<PowerEvent>,
) -> eyre::Result<()> {
    let proxy = Proxy::new(&connection, destination, path, interface)?;
    let signals = proxy.receive_signal(name)?;
    std::thread::Builder::new()
        .name(format!("komorebi-recorder-{name}"))
        .spawn(move || {
            // keeps the bus connection for as long as the signals are read
            let _connection = connection;
            for signal in signals {
                let Ok(argument) = signal.body().deserialize::<bool>() else {
                    continue;
                };
                let event = PowerEvent {
                    kind: kind(argument),
                    anchor: Anchor::sample(),
                };
                if events.send(event).is_err() {
                    return;
                }
            }
        })?;
    Ok(())
}

/// Starts listening, and returns the watcher.
///
/// # Errors
///
/// Never fails: a bus that cannot be reached, or a service that is not there, means no events
/// of that kind. The result is `None` only if neither could be listened to.
pub fn start() -> eyre::Result<Option<Watcher>> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut listening = 0;

    if let Ok(system) = Connection::system()
        && listen(
            system,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "PrepareForSleep",
            |sleeping| {
                if sleeping {
                    PowerKind::Suspend
                } else {
                    PowerKind::Resume
                }
            },
            tx.clone(),
        )
        .is_ok()
    {
        listening += 1;
    }

    if let Ok(session) = Connection::session()
        && listen(
            session,
            "org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver",
            "ActiveChanged",
            |locked| {
                if locked {
                    PowerKind::SessionLocked
                } else {
                    PowerKind::SessionUnlocked
                }
            },
            tx,
        )
        .is_ok()
    {
        listening += 1;
    }

    Ok((listening > 0).then_some(Watcher { events: rx }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_watcher_starts_with_or_without_buses_and_has_heard_nothing() {
        let watcher = super::start().expect("start never fails");
        if let Some(watcher) = watcher {
            assert!(watcher.drain().is_empty());
        }
    }
}
