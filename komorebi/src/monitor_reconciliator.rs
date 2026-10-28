//! Keeping komorebi's monitors in step with the compositor's outputs, as they change size.
//!
//! The KWin backend sends a [`MonitorNotification`] when KWin's list of outputs changes: an output
//! gets another resolution or scale, or a panel changes the room the windows have on it. The
//! thread [`listen_for_notifications`] starts takes them one at a time. For a
//! [`MonitorNotification::Resize`] the size and the work area of every monitor are read again
//! ([`desktop::update_monitor_work_areas`]), and the focused workspace of the focused monitor is
//! tiled in the area it has now.
//!
//! Every subscriber is sent a notification after each one is handled
//! ([`NotificationEvent::Monitor`]), and the backend is asked for the look again
//! ([`border_manager::send_notification`]), which it does nothing about unless a setting changed.
//!
//! Covered by `tests/monitor_resize.rs` against the fake backend, and by the `vm-hotplug` VM
//! suite.

use crate::Notification;
use crate::NotificationEvent;
use crate::border_manager;
use crate::desktop;
use crate::notify_subscribers;
use crate::state::State;
use crate::window_manager::WindowManager;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use std::sync::OnceLock;
use strum::Display;

/// A change of the monitors, sent to the monitor reconciliator with [`send_notification`].
///
/// The number is a KWin monitor id, which the reconciliator does not read: it reads every monitor
/// again.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum MonitorNotification {
    /// A monitor's size or work area changed (a panel moved, a resolution or scale changed): the
    /// id of the monitor that changed, or 0 when the KWin backend cannot tell which
    Resize(u32),
}

static CHANNEL: OnceLock<(Sender<MonitorNotification>, Receiver<MonitorNotification>)> =
    OnceLock::new();

/// The channel [`send_notification`] sends on and the thread of [`listen_for_notifications`]
/// receives from. It holds 20 notifications.
pub fn channel() -> &'static (Sender<MonitorNotification>, Receiver<MonitorNotification>) {
    CHANNEL.get_or_init(|| crossbeam_channel::bounded(20))
}

fn event_tx() -> Sender<MonitorNotification> {
    channel().0.clone()
}

fn event_rx() -> Receiver<MonitorNotification> {
    channel().1.clone()
}

/// Sends `notification` to the monitor reconciliator, without waiting. A notification is dropped,
/// with a warning in the log, when the channel is full.
pub fn send_notification(notification: MonitorNotification) {
    if event_tx().try_send(notification).is_err() {
        tracing::warn!("channel is full; dropping notification")
    }
}

/// Starts the thread that takes the notifications, for as long as komorebi runs, and returns at
/// once.
///
/// [`handle_notifications`] is called again when it returns, and the reason is logged.
///
/// # Errors
///
/// None: the result is always `Ok`.
pub fn listen_for_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    std::thread::spawn(move || {
        loop {
            match handle_notifications(wm.clone()) {
                Ok(()) => {
                    tracing::warn!("restarting finished thread");
                }
                Err(error) => {
                    if cfg!(debug_assertions) {
                        tracing::error!("restarting failed thread: {:?}", error)
                    } else {
                        tracing::error!("restarting failed thread: {}", error)
                    }
                }
            }
        }
    });

    Ok(())
}

/// Takes the notifications one at a time, for as long as komorebi runs.
///
/// Each is dealt with as the [module docs](self) say. The subscribers are then notified, and the
/// backend is asked for the look again.
///
/// # Errors
///
/// Returns an error if the subscribers cannot be notified ([`notify_subscribers`]);
/// [`listen_for_notifications`] then calls it again. A monitor that cannot be read again is
/// logged.
pub fn handle_notifications(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("listening");
    let receiver = event_rx();

    for notification in receiver {
        let initial_state = {
            let wm = wm.lock();
            State::from(wm.as_ref())
        };

        match notification {
            MonitorNotification::Resize(_display_id) => {
                tracing::debug!("handling resize notification");
                let mut wm = wm.lock();
                if let Err(error) = desktop::update_monitor_work_areas(&mut wm) {
                    tracing::error!("failed to update monitor work areas: {error}");
                }
            }
        }

        {
            let wm = wm.lock();
            notify_subscribers(
                Notification {
                    event: NotificationEvent::Monitor(notification),
                    state: wm.as_ref().into(),
                },
                initial_state.has_been_modified(&wm),
            )?;
        }

        border_manager::send_notification();
    }

    Ok(())
}
