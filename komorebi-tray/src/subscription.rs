use crate::state::DisplayState;
use crate::state::IconKind;
use komorebi_client::SocketMessage;
use komorebi_client::SubscribeOptions;
use komorebi_client::UnixListener;
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

const RETRY_INTERVAL: Duration = Duration::from_secs(1);
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const SOCKET_TIMEOUT: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub struct Worker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(publish: impl Fn(DisplayState) + Send + 'static) -> io::Result<Self> {
        let data_dir = dirs::data_local_dir()
            .ok_or_else(|| io::Error::other("No local application data directory"))?
            .join("komorebi");
        std::fs::create_dir_all(&data_dir)?;
        let name = format!("komorebi-tray-{}", std::process::id());
        let path = data_dir.join(&name);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("komorebi-tray-subscription".to_owned())
            .spawn(move || {
                let subscription = Subscription {
                    name,
                    path,
                    listener: None,
                };
                run(subscription, &worker_stop, publish);
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

struct Subscription {
    name: String,
    path: PathBuf,
    listener: Option<UnixListener>,
}

// Keep transport separate from scheduling so pause/reconnect behavior can be
// exercised without sending commands to the user's window manager.
trait Transport {
    fn register(&mut self) -> io::Result<()>;
    fn query(&mut self) -> io::Result<DisplayState>;
    fn receive(&mut self) -> io::Result<Option<Vec<u8>>>;
}

impl Transport for Subscription {
    fn register(&mut self) -> io::Result<()> {
        let options = SubscribeOptions {
            filter_state_changes: true,
        };
        if self.listener.is_some() && self.path.exists() {
            komorebi_client::send_message(&SocketMessage::AddSubscriberSocketWithOptions(
                self.name.clone(),
                options,
            ))
        } else {
            self.listener = None;
            let listener = komorebi_client::subscribe_with_options(&self.name, options)?;
            listener.set_nonblocking(true)?;
            self.listener = Some(listener);
            Ok(())
        }
    }

    fn query(&mut self) -> io::Result<DisplayState> {
        let response = komorebi_client::send_query(&SocketMessage::State)?;
        DisplayState::from_snapshot(response.as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn receive(&mut self) -> io::Result<Option<Vec<u8>>> {
        let Some(listener) = &self.listener else {
            return Ok(None);
        };
        match listener.accept() {
            Ok((mut stream, _)) => {
                // Winsock can inherit nonblocking mode from the listener.
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
                let mut bytes = Vec::new();
                stream.read_to_end(&mut bytes)?;
                Ok(Some(bytes))
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => {
                self.listener = None;
                Err(error)
            }
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = komorebi_client::send_message(&SocketMessage::RemoveSubscriberSocket(
            self.name.clone(),
        ));
        self.listener = None;
        let _ = std::fs::remove_file(&self.path);
    }
}

struct Connection {
    current: DisplayState,
    next_probe: Instant,
}

impl Connection {
    fn new(now: Instant) -> Self {
        Self {
            current: DisplayState::disconnected(),
            next_probe: now,
        }
    }

    fn update(&mut self, display: DisplayState, publish: &impl Fn(DisplayState)) {
        if self.current != display {
            self.current = display.clone();
            publish(display);
        }
    }

    fn interval(&self) -> Duration {
        if self.current.icon == IconKind::Paused {
            RETRY_INTERVAL
        } else {
            REFRESH_INTERVAL
        }
    }

    fn step(
        &mut self,
        transport: &mut impl Transport,
        now: Instant,
        stop: &AtomicBool,
        publish: &impl Fn(DisplayState),
    ) {
        let mut resumed_in_batch = false;
        // Bound each batch so busy notifications cannot starve shutdown or probes.
        for _ in 0..16 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            match transport.receive() {
                Ok(Some(bytes)) if !bytes.is_empty() => {
                    if let Ok(display) = DisplayState::from_notification(&bytes) {
                        let resumed = self.current.icon == IconKind::Paused
                            && display.icon != IconKind::Paused;
                        resumed_in_batch |= resumed;
                        self.update(display, publish);
                        self.next_probe = now + self.interval();
                    }
                }
                Ok(None) => break,
                // EOF or a broken notification connection does not necessarily
                // mean the daemon is gone. Ask State before showing disconnected.
                Ok(Some(_)) | Err(_) => {
                    self.next_probe = now;
                    break;
                }
            }
        }

        if resumed_in_batch {
            self.next_probe = now;
        }
        if stop.load(Ordering::Relaxed) || now < self.next_probe {
            return;
        }
        // State remains available while paused, unlike subscriber registration.
        // send_query bounds both reading and writing to one second each.
        match transport.query() {
            Ok(display) => {
                self.update(display, publish);
                self.next_probe = now + self.interval();
                if self.current.icon != IconKind::Paused && transport.register().is_err() {
                    // The successful query proves the daemon is alive. Retry
                    // registration without flickering to the disconnected icon.
                    self.next_probe = now + RETRY_INTERVAL;
                }
            }
            Err(_) => {
                self.update(DisplayState::disconnected(), publish);
                self.next_probe = now + RETRY_INTERVAL;
            }
        }
    }
}

fn run(mut subscription: Subscription, stop: &AtomicBool, publish: impl Fn(DisplayState)) {
    let mut connection = Connection::new(Instant::now());
    while !stop.load(Ordering::Relaxed) {
        connection.step(&mut subscription, Instant::now(), stop, &publish);
        thread::park_timeout(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct FakeTransport {
        paused: bool,
        online: bool,
        invalid_state: bool,
        ignore_registration: bool,
        registration_fails: bool,
        registrations: usize,
        queries: usize,
        notifications: VecDeque<Vec<u8>>,
    }

    impl FakeTransport {
        fn new(paused: bool) -> Self {
            Self {
                paused,
                online: true,
                invalid_state: false,
                ignore_registration: false,
                registration_fails: false,
                registrations: 0,
                queries: 0,
                notifications: VecDeque::new(),
            }
        }

        fn snapshot(&self) -> String {
            format!(
                r#"{{"is_paused":{},"monitors":{{"focused":0,"elements":[{{
                "name":"Left","workspaces":{{"focused":1,"elements":[{{}},{{}}]}}
            }}]}}}}"#,
                self.paused
            )
        }

        fn notify(&mut self) {
            self.notifications
                .push_back(format!(r#"{{"state":{}}}"#, self.snapshot()).into_bytes());
        }
    }

    impl Transport for FakeTransport {
        fn query(&mut self) -> io::Result<DisplayState> {
            self.queries += 1;
            if !self.online {
                return Err(io::Error::from(io::ErrorKind::ConnectionRefused));
            }
            let response = if self.invalid_state {
                "{}".to_owned()
            } else {
                self.snapshot()
            };
            DisplayState::from_snapshot(response.as_bytes())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        }

        fn register(&mut self) -> io::Result<()> {
            self.registrations += 1;
            if self.registration_fails {
                return Err(io::Error::from(io::ErrorKind::ConnectionRefused));
            }
            // Match the real daemon: paused registration produces no snapshot.
            if !self.paused && !self.ignore_registration {
                self.notify();
            }
            Ok(())
        }

        fn receive(&mut self) -> io::Result<Option<Vec<u8>>> {
            Ok(self.notifications.pop_front())
        }
    }

    #[test]
    fn startup_paused_stays_paused_and_resume_restores_subscription() {
        let start = Instant::now();
        let mut connection = Connection::new(start);
        let mut daemon = FakeTransport::new(true);
        let states = RefCell::new(Vec::new());
        let publish = |state| states.borrow_mut().push(state);
        let stop = AtomicBool::new(false);
        for second in 0..=12 {
            connection.step(
                &mut daemon,
                start + Duration::from_secs(second),
                &stop,
                &publish,
            );
            assert_eq!(connection.current.icon, IconKind::Paused);
        }
        assert_eq!(daemon.queries, 13);
        assert_eq!(daemon.registrations, 0);
        assert_eq!(states.borrow().len(), 1);
        daemon.paused = false;
        connection.step(
            &mut daemon,
            start + Duration::from_secs(13),
            &stop,
            &publish,
        );
        assert_eq!(connection.current.icon, IconKind::Workspace(2));
        assert_eq!(daemon.registrations, 1);
        assert_eq!(states.borrow().len(), 2);
    }

    #[test]
    fn notifications_change_pause_state_immediately_without_workspace_change() {
        let start = Instant::now();
        let mut connection = Connection::new(start);
        let mut daemon = FakeTransport::new(false);
        let stop = AtomicBool::new(false);
        connection.step(&mut daemon, start, &stop, &|_| {});
        daemon.paused = true;
        daemon.notify();
        connection.step(&mut daemon, start + POLL_INTERVAL, &stop, &|_| {});
        assert_eq!(connection.current.icon, IconKind::Paused);
        assert_eq!(daemon.queries, 1);
        daemon.paused = false;
        daemon.notify();
        daemon.notify();
        connection.step(&mut daemon, start + POLL_INTERVAL * 2, &stop, &|_| {});
        assert_eq!(connection.current.icon, IconKind::Workspace(2));
        assert_eq!(daemon.registrations, 2);
    }

    #[test]
    fn lost_registration_snapshot_is_not_a_disconnection() {
        let start = Instant::now();
        let mut connection = Connection::new(start);
        let mut daemon = FakeTransport::new(false);
        daemon.ignore_registration = true;
        let stop = AtomicBool::new(false);
        connection.step(&mut daemon, start, &stop, &|_| {});
        // A pause races with registration; no subscription snapshot arrives.
        daemon.paused = true;
        connection.step(&mut daemon, start + REFRESH_INTERVAL, &stop, &|_| {});
        assert_eq!(connection.current.icon, IconKind::Paused);
        connection.step(&mut daemon, start + REFRESH_INTERVAL * 2, &stop, &|_| {});
        assert_eq!(connection.current.icon, IconKind::Paused);
    }

    #[test]
    fn disconnect_invalid_state_and_recovery_while_paused() {
        let start = Instant::now();
        let mut connection = Connection::new(start);
        let mut daemon = FakeTransport::new(true);
        let stop = AtomicBool::new(false);
        connection.step(&mut daemon, start, &stop, &|_| {});
        daemon.online = false;
        connection.step(&mut daemon, start + RETRY_INTERVAL, &stop, &|_| {});
        assert_eq!(connection.current, DisplayState::disconnected());
        daemon.online = true;
        daemon.invalid_state = true;
        connection.step(&mut daemon, start + RETRY_INTERVAL * 2, &stop, &|_| {});
        assert_eq!(connection.current, DisplayState::disconnected());
        daemon.invalid_state = false;
        connection.step(&mut daemon, start + RETRY_INTERVAL * 3, &stop, &|_| {});
        assert_eq!(connection.current.icon, IconKind::Paused);
    }

    #[test]
    fn registration_failure_retries_without_hiding_valid_state() {
        let start = Instant::now();
        let mut connection = Connection::new(start);
        let mut daemon = FakeTransport::new(false);
        daemon.registration_fails = true;
        let stop = AtomicBool::new(false);
        connection.step(&mut daemon, start, &stop, &|_| {});
        assert_eq!(connection.current.icon, IconKind::Workspace(2));
        daemon.registration_fails = false;
        connection.step(&mut daemon, start + RETRY_INTERVAL, &stop, &|_| {});
        assert_eq!(daemon.registrations, 2);
    }

    #[test]
    fn malformed_notifications_are_ignored_and_eof_triggers_a_probe() {
        let start = Instant::now();
        let mut connection = Connection::new(start);
        let mut daemon = FakeTransport::new(false);
        let stop = AtomicBool::new(false);
        connection.step(&mut daemon, start, &stop, &|_| {});
        daemon.notifications.push_back(b"bad json".to_vec());
        connection.step(&mut daemon, start + POLL_INTERVAL, &stop, &|_| {});
        assert_eq!(daemon.queries, 1);
        assert_eq!(connection.current.icon, IconKind::Workspace(2));
        daemon.paused = true;
        daemon.notifications.push_back(Vec::new());
        connection.step(&mut daemon, start + POLL_INTERVAL * 2, &stop, &|_| {});
        assert_eq!(daemon.queries, 2);
        assert_eq!(connection.current.icon, IconKind::Paused);
    }
}
