//! Subscribers against a window manager that listens the way the daemon does, with
//! [`listen_for_commands`], over an in-memory desktop with one monitor: each subscriber listens on a
//! socket of its own in a runtime directory of this process, subscribes with `komorebi-client`, and
//! reads the notifications the window manager sends it.
//!
//! The tests share the window manager, and take turns: every subscriber is sent every
//! notification, so a test that ran at the same time as another would read the other's
//! notifications too. Window ids are 901 and up.

use komorebi::SUBSCRIPTION_SOCKETS;
use komorebi::backend::fake::FakeBackend;
use komorebi::backend::set_backend;
use komorebi::process_command::listen_for_commands;
use komorebi::window::Window;
use komorebi::window_manager::WindowManager;
use komorebi::window_manager_event::KWinNotification;
use komorebi::window_manager_event::SystemNotification;
use komorebi::window_manager_event::WindowManagerEvent;
use komorebi::window_manager_event_listener;
use komorebi_client::DATA_DIR;
use komorebi_client::Notification;
use komorebi_client::NotificationEvent;
use komorebi_client::SocketMessage;
use komorebi_client::SubscribeOptions;
use komorebi_client::send_query;
use komorebi_client::subscribe;
use komorebi_client::subscribe_with_options;
use parking_lot::Mutex;
use std::io::ErrorKind;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::mpsc::Receiver;
use std::time::Duration;

struct Daemon {
    fake: Arc<FakeBackend>,
    wm: Arc<Mutex<WindowManager>>,
}

impl Daemon {
    /// Opens the window `id` and has the window manager handle the event of it being shown
    fn show(&self, id: u32) {
        self.fake.open_window(id, "foot", "foot", "a terminal");
        self.handle(WindowManagerEvent::Show(
            SystemNotification::KWin(KWinNotification::WindowAdded),
            Window::from(id),
        ));
    }

    /// Has the window manager handle the event of the window `id` being minimized
    fn minimize(&self, id: u32) {
        self.handle(WindowManagerEvent::Minimize(
            SystemNotification::KWin(KWinNotification::WindowMinimized),
            Window::from(id),
        ));
    }

    fn handle(&self, event: WindowManagerEvent) {
        self.wm.lock().process_event(event).unwrap();
    }
}

/// Starts the window manager once, and waits for the other tests to be done with it
fn daemon() -> (parking_lot::MutexGuard<'static, ()>, &'static Daemon) {
    static TURN: Mutex<()> = parking_lot::const_mutex(());
    static DAEMON: OnceLock<Daemon> = OnceLock::new();

    let turn = TURN.lock();
    let daemon = DAEMON.get_or_init(|| {
        // /tmp: short enough for a unix socket path on macOS
        let runtime = format!("/tmp/komorebi-subscribe-test-{}", std::process::id());
        // SAFETY: the first test to get here sets it, before any thread reads the environment
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) };
        std::fs::create_dir_all(&*DATA_DIR).unwrap();

        let fake = Arc::new(FakeBackend::with_one_monitor());
        set_backend(fake.clone());

        let mut wm = WindowManager::new(window_manager_event_listener::event_rx(), None).unwrap();
        wm.init().unwrap();
        let wm = Arc::new(Mutex::new(wm));
        listen_for_commands(wm.clone());

        Daemon { fake, wm }
    });

    (turn, daemon)
}

/// A subscriber whose notifications are read on another thread. It unsubscribes when it is
/// dropped, and waits for the window manager to have handled that.
struct Subscriber {
    name: &'static str,
    notifications: Receiver<Notification>,
}

impl Subscriber {
    fn new(name: &'static str, listener: UnixListener) -> Self {
        let (sender, notifications) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            for connection in listener.incoming() {
                let mut json = String::new();
                connection.unwrap().read_to_string(&mut json).unwrap();
                let notification = serde_json::from_str(&json).unwrap();
                if sender.send(notification).is_err() {
                    break;
                }
            }
        });

        Self {
            name,
            notifications,
        }
    }

    /// Returns the notifications received until, and with, the first one whose event `last` is
    /// true of, waiting up to five seconds for each
    fn until(&self, last: impl Fn(&NotificationEvent) -> bool) -> Vec<Notification> {
        let mut received: Vec<Notification> = vec![];
        loop {
            let notification = self
                .notifications
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| {
                    panic!("{} received {received:?}, and then nothing", self.name)
                });
            let done = last(&notification.event);
            received.push(notification);
            if done {
                return received;
            }
        }
    }

    /// Returns the next notification, which is the one of the subscription itself
    fn subscribed(&self) -> Notification {
        let notification = self
            .notifications
            .recv_timeout(Duration::from_secs(5))
            .expect("the notification of the subscription");
        assert!(
            subscribed(self.name)(&notification.event),
            "{notification:?}"
        );
        notification
    }
}

impl Drop for Subscriber {
    fn drop(&mut self) {
        // send_query returns once the window manager has handled the message and closed the
        // connection
        let unsubscribe = SocketMessage::RemoveSubscriberSocket(self.name.to_string());
        let _ = send_query(&unsubscribe);
    }
}

fn subscribed(name: &str) -> impl Fn(&NotificationEvent) -> bool + '_ {
    move |event| match event {
        NotificationEvent::Socket(SocketMessage::AddSubscriberSocket(socket))
        | NotificationEvent::Socket(SocketMessage::AddSubscriberSocketWithOptions(socket, _)) => {
            socket == name
        }
        _ => false,
    }
}

fn shown(id: u32) -> impl Fn(&NotificationEvent) -> bool {
    move |event| {
        matches!(
            event,
            NotificationEvent::WindowManager(WindowManagerEvent::Show(_, window)) if window.id == id
        )
    }
}

fn minimized(id: u32) -> impl Fn(&NotificationEvent) -> bool {
    move |event| {
        matches!(
            event,
            NotificationEvent::WindowManager(WindowManagerEvent::Minimize(_, window)) if window.id == id
        )
    }
}

fn is_state(notification: &Notification) -> bool {
    matches!(
        notification.event,
        NotificationEvent::Socket(SocketMessage::State)
    )
}

/// Returns true if and only if the first workspace of the first monitor has the window `id`
fn on_the_workspace(notification: &Notification, id: u32) -> bool {
    notification.state.monitors.elements()[0].workspaces()[0].contains_window(id)
}

#[test]
fn a_subscriber_is_sent_the_window_that_was_shown_and_the_state_after_it()
-> Result<(), Box<dyn std::error::Error>> {
    let (_turn, daemon) = daemon();
    let subscriber = Subscriber::new("shown.sock", subscribe("shown.sock")?);
    let subscription = subscriber.subscribed();
    assert!(!on_the_workspace(&subscription, 901));

    daemon.show(901);

    let received = subscriber.until(shown(901));
    assert_eq!(received.len(), 1, "{received:?}");
    assert!(on_the_workspace(&received[0], 901));
    Ok(())
}

#[test]
fn a_subscriber_that_filters_state_changes_is_not_sent_what_changed_nothing()
-> Result<(), Box<dyn std::error::Error>> {
    let (_turn, daemon) = daemon();
    let options = SubscribeOptions {
        filter_state_changes: true,
    };
    let filtered = Subscriber::new(
        "filtered.sock",
        subscribe_with_options("filtered.sock", options)?,
    );
    filtered.subscribed();
    let everything = Subscriber::new("everything.sock", subscribe("everything.sock")?);
    everything.subscribed();
    // a subscriber that filters is sent the subscription of another, which changes nothing
    assert!(subscribed("everything.sock")(
        &filtered.until(|_| true)[0].event
    ));

    // asking for the state changes nothing; a window that is shown is always sent, and one that
    // is minimized leaves its workspace, which is a change
    send_query(&SocketMessage::State)?;
    daemon.show(911);
    daemon.minimize(911);

    let received = filtered.until(minimized(911));
    assert!(!received.iter().any(is_state), "{received:?}");
    assert!(shown(911)(&received[0].event), "{received:?}");
    assert!(!on_the_workspace(&received[1], 911));
    let received = everything.until(minimized(911));
    assert!(is_state(&received[0]), "{received:?}");
    Ok(())
}

#[test]
fn a_subscriber_that_is_gone_is_dropped_and_not_tried_again()
-> Result<(), Box<dyn std::error::Error>> {
    let (_turn, _daemon) = daemon();
    let socket = DATA_DIR.join("gone.sock");
    let gone = subscribe("gone.sock")?;
    let mut json = String::new();
    gone.accept()?.0.read_to_string(&mut json)?;
    let notification: Notification = serde_json::from_str(&json)?;
    assert!(subscribed("gone.sock")(&notification.event));

    // the program exits: the socket file is left behind, and nothing listens on it
    drop(gone);
    assert!(socket.exists());
    send_query(&SocketMessage::State)?;

    assert!(!SUBSCRIPTION_SOCKETS.lock().contains_key("gone.sock"));
    assert!(!socket.exists());

    // a program that listens on the same socket again is sent nothing until it subscribes again
    let again = UnixListener::bind(&socket)?;
    again.set_nonblocking(true)?;
    send_query(&SocketMessage::State)?;
    assert_eq!(
        again.accept().map(|_| ()).map_err(|error| error.kind()),
        Err(ErrorKind::WouldBlock)
    );
    std::fs::remove_file(&socket)?;
    Ok(())
}
