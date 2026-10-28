//! The machine going to sleep and waking up, and the screen being locked and unlocked.
//!
//! None of it changes komorebi's state, but each can leave the desktop other than komorebi left
//! it. While the screen is locked or asleep the compositor sends the clients no frame
//! callbacks, so nothing komorebi asked for in that time was done, and the monitors can come back
//! in another order or at another size. When it is over, this module sends
//! [`MonitorNotification::Wake`], and the monitor reconciliator's
//! [`wake_up`](crate::monitor_reconciliator::wake_up) looks at the monitors again and puts every
//! window in its tile.
//!
//! Two signals are watched:
//!
//! - `PrepareForSleep` of `org.freedesktop.login1.Manager`, from logind on the system bus: `false`
//!   is the machine waking up.
//! - `ActiveChanged` of `org.freedesktop.ScreenSaver`, from KDE's lock screen on the session bus:
//!   `false` is the screen being unlocked.
//!
//! # What is guaranteed, and by what
//!
//! | Rule | Enforced by | Proof |
//! |---|---|---|
//! | Waking up and unlocking each send one `Wake`, after a delay; going to sleep and locking send nothing | the two watchers | `tests/power.rs::waking_and_unlocking_make_the_window_manager_look_again_and_going_to_sleep_and_locking_do_not` |
//! | Only logind is believed about sleep | the match rule names `org.freedesktop.login1` as the sender | no test (the test sends as logind only) |
//! | komorebi runs without a system bus or a lock screen | [`watch_on`] logs that it is not watching for sleep; a session without a lock screen sends no signal | no test |
//!
//! The lock screen's signal is taken from any sender on the session bus.
//!
//! A real suspend and resume is not tested: at time of writing (2026-10-27) the QEMU guests have
//! no way to wake from one. The signals are tested on private buses, and the lock screen against
//! a real KWin.

#![deny(missing_docs)]

use crate::monitor_reconciliator;
use crate::monitor_reconciliator::MonitorNotification;
use futures_util::StreamExt;
use std::time::Duration;
use zbus::Connection;
use zbus::MatchRule;
use zbus::MessageStream;
use zbus::message::Type;

/// How long after the screen is unlocked komorebi waits before it looks: the lock screen goes
/// before the windows can be seen.
const AFTER_UNLOCK: Duration = Duration::from_millis(600);

/// How long after the machine wakes komorebi waits before it looks: the outputs are found again
/// one after another, and the compositor moves windows about as they are.
const AFTER_WAKE: Duration = Duration::from_millis(2500);

/// Watches for the machine waking up and the screen being unlocked, for as long as komorebi runs.
///
/// The watchers are spawned on the tokio runtime this is called on, and this returns once they
/// are spawned. Neither signal has to come: a session without a lock screen, or a container with
/// no system bus, has nothing to watch, and the missing system bus is logged.
pub async fn watch(session: Connection) {
    watch_on(session, Connection::system().await.ok()).await;
}

/// Watches as [`watch`] does, on the buses it is given: the session bus for the lock screen, and
/// the system bus for logind when there is one.
///
/// A test passes private buses, with a connection standing in for logind and one for the lock
/// screen; `tests/power.rs` does.
pub async fn watch_on(session: Connection, system: Option<Connection>) {
    tokio::spawn(screen_lock(session));

    match system {
        Some(system) => {
            tokio::spawn(sleep(system));
        }
        None => tracing::debug!("not watching for the machine going to sleep: no system bus"),
    }
}

/// Sends a `Wake` [`AFTER_UNLOCK`] each time the lock screen goes, until the bus closes.
async fn screen_lock(session: Connection) {
    let result = async {
        // KDE's lock screen says whether it is up, on KWin's name for the freedesktop interface
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.ScreenSaver")?
            .member("ActiveChanged")?
            .build();
        let mut stream = MessageStream::for_match_rule(rule, &session, None).await?;

        while let Some(message) = stream.next().await {
            let Ok(message) = message else { continue };
            let Ok(locked) = message.body().deserialize::<bool>() else {
                continue;
            };

            if locked {
                tracing::info!("the screen was locked");
            } else {
                tracing::info!("the screen was unlocked");
                wake_after(AFTER_UNLOCK).await;
            }
        }

        zbus::Result::Ok(())
    }
    .await;

    if let Err(error) = result {
        tracing::debug!("not watching the lock screen: {error}");
    }
}

/// Sends a `Wake` [`AFTER_WAKE`] each time logind signals that the machine woke up, until the bus
/// closes.
async fn sleep(system: Connection) {
    let result = async {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.login1")?
            .interface("org.freedesktop.login1.Manager")?
            .member("PrepareForSleep")?
            .build();
        let mut stream = MessageStream::for_match_rule(rule, &system, None).await?;

        while let Some(message) = stream.next().await {
            let Ok(message) = message else { continue };
            let Ok(going_to_sleep) = message.body().deserialize::<bool>() else {
                continue;
            };

            if going_to_sleep {
                tracing::info!("the machine is going to sleep");
            } else {
                tracing::info!("the machine woke up");
                wake_after(AFTER_WAKE).await;
            }
        }

        zbus::Result::Ok(())
    }
    .await;

    if let Err(error) = result {
        tracing::debug!("not watching for the machine going to sleep: {error}");
    }
}

/// Waits, then sends a `Wake` to the monitor reconciliator.
async fn wake_after(delay: Duration) {
    tokio::time::sleep(delay).await;
    monitor_reconciliator::send_notification(MonitorNotification::Wake);
}
