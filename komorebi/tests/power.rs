//! The machine waking up and the screen being unlocked reach komorebi as the signals logind and the
//! lock screen send, and each makes the window manager look at the desktop again.
//!
//! The buses here are private ones, with a test standing in for logind and for KDE's lock screen.
//! It needs `dbus-daemon`, and says nothing when there is none.

use komorebi::kwin::power;
use komorebi::monitor_reconciliator;
use komorebi::monitor_reconciliator::MonitorNotification;
use std::io::BufRead;
use std::io::BufReader;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use zbus::Connection;

struct Bus {
    daemon: Child,
    address: String,
}

impl Bus {
    fn start() -> Option<Self> {
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;

        let mut address = String::new();
        BufReader::new(daemon.stdout.take()?)
            .read_line(&mut address)
            .ok()?;

        Some(Self {
            daemon,
            address: address.trim().to_string(),
        })
    }

    async fn connect(&self, name: Option<&str>) -> Connection {
        let mut builder = zbus::connection::Builder::address(self.address.as_str()).unwrap();
        if let Some(name) = name {
            builder = builder.name(name.to_string()).unwrap();
        }
        builder.build().await.unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn woke(timeout: Duration) -> bool {
    matches!(
        monitor_reconciliator::channel().1.recv_timeout(timeout),
        Ok(MonitorNotification::Wake)
    )
}

#[test]
fn waking_and_unlocking_make_the_window_manager_look_again_and_going_to_sleep_and_locking_do_not() {
    let (Some(session_bus), Some(system_bus)) = (Bus::start(), Bus::start()) else {
        eprintln!("no dbus-daemon: skipping");
        return;
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        let watcher_session = session_bus.connect(None).await;
        let watcher_system = system_bus.connect(None).await;
        power::watch_on(watcher_session, Some(watcher_system)).await;

        // logind, and the lock screen
        let logind = system_bus.connect(Some("org.freedesktop.login1")).await;
        let screen_saver = session_bus
            .connect(Some("org.freedesktop.ScreenSaver"))
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let emit = |connection: Connection,
                    path: &'static str,
                    interface: &'static str,
                    member: &'static str,
                    on: bool| async move {
            connection
                .emit_signal(None::<&str>, path, interface, member, &(on,))
                .await
                .unwrap();
        };

        // going to sleep is nothing to do yet: nothing has changed
        emit(
            logind.clone(),
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "PrepareForSleep",
            true,
        )
        .await;
        // and neither is the lock screen coming up
        emit(
            screen_saver.clone(),
            "/ScreenSaver",
            "org.freedesktop.ScreenSaver",
            "ActiveChanged",
            true,
        )
        .await;
        assert!(
            !tokio::task::spawn_blocking(|| woke(Duration::from_millis(1200)))
                .await
                .unwrap(),
            "going to sleep, or locking, made it look"
        );

        // the lock screen goes
        emit(
            screen_saver.clone(),
            "/ScreenSaver",
            "org.freedesktop.ScreenSaver",
            "ActiveChanged",
            false,
        )
        .await;
        assert!(
            tokio::task::spawn_blocking(|| woke(Duration::from_secs(4)))
                .await
                .unwrap(),
            "unlocking did not make it look"
        );

        // the machine wakes (a moment later: the monitors are found one at a time)
        emit(
            logind.clone(),
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "PrepareForSleep",
            false,
        )
        .await;
        assert!(
            tokio::task::spawn_blocking(|| woke(Duration::from_secs(6)))
                .await
                .unwrap(),
            "waking did not make it look"
        );

        // somebody that is not logind saying the same thing is not believed
        let impostor = system_bus.connect(None).await;
        emit(
            impostor,
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "PrepareForSleep",
            false,
        )
        .await;
        assert!(
            !tokio::task::spawn_blocking(|| woke(Duration::from_secs(4)))
                .await
                .unwrap(),
            "a signal from something other than logind was taken for logind's"
        );
    });
}
