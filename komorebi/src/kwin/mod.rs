//! The KWin backend: [`KWinBackend`], made by [`connect`].
//!
//! KWin has no protocol that lets another process read and set window geometry, so komorebi runs a
//! small script inside KWin (`bridge.js`) and talks to it over D-Bus:
//!
//! ```text
//!   komorebi (this process)                              KWin
//!   -----------------------                              ----
//!   owns org.komorebi.Bridge   <--- Event(json) -------  script reports what it sees
//!                                                        (window added, focus, minimize, ...)
//!   answers Poll(json) late    ---- batch of commands -> script carries them out
//! ```
//!
//! Events are delivered in order on an interface that handles calls inline; commands go out as the
//! answer to a `Poll` call the script makes on a separate object, and that the daemon holds open
//! until it has something to send, so waiting for work never blocks an event.
//!
//! All of the decisions about what an event means are made in [`state`], which is pure and tested
//! without a compositor. This module is the plumbing around it: the D-Bus service, the held call,
//! and loading the script into KWin.
//!
//! The script is type-checked by `tsc` from its JSDoc comments and tested with `node --test`
//! against a fake KWin (`just check-js`). Its tests and the tests of this module and of [`state`]
//! read the same messages, in `fixtures/`, so that what one side sends is what the other reads.
//!
//! The backend answers every query from the [`state::Table`], turns what KWin reports into the
//! window events of [`crate::window_manager_event`], and queues every command of [`Backend`] for
//! the script, which carries it out in KWin.
//!
//! KWin can go away under a running daemon (it crashed, or `kwin_wayland --replace` took its
//! place) while the session bus stays. The daemon watches who owns `org.kde.KWin` on the bus:
//! when nobody does, every window of the old KWin is gone, and nothing will report it closing, so
//! the table reports each as destroyed ([`state::Table::compositor_lost`]); when the name has an
//! owner again, the daemon loads the script into the new KWin, which reports its desktop as it
//! does at start.
//!
//! Workspaces are KWin's own virtual desktops, per output: workspace `i` is desktop `i`, and each
//! output shows a desktop of its own, which KWin does once its per-output virtual desktops are
//! turned on. [`connect`] turns them on in the user's `kwinrc` (the exit puts the user's setting
//! back), desktops are created over KWin's `org.kde.KWin.VirtualDesktopManager` interface when a
//! workspace has none, and the script switches an output's desktop and puts a window on a desktop.
//! When something other than komorebi does either, the script reports it, and the table turns it
//! into a [`WorkspaceChange`] or a [`WorkspaceMove`] for the window manager to follow.
//!
//! Hiding a window ([`Backend::hide`]) moves it to one more desktop, `komorebi-park`, that nobody
//! looks at and that is not a workspace; [`Backend::restore`] moves it back to the desktop of its
//! workspace. KWin saves its desktops in `kwinrc` as soon as one is made, so a desktop that
//! komorebi made and did not remove would be there for every later session: komorebi removes it
//! when it stops, and one that an earlier run left (it was killed, or the session was torn down
//! under it) is found in the first snapshot, its windows are sent home and it is removed.
//!
//! [`WorkspaceChange`]: crate::window_manager_event::WindowManagerEvent::WorkspaceChange
//! [`WorkspaceMove`]: crate::window_manager_event::WindowManagerEvent::WorkspaceMove
//!
//! Borders are drawn by a KWin effect, KDE Rounded Corners, which the backend configures: it writes
//! the effect's settings into the user's `kwinrc`, loads the effect, and takes the settings back
//! out when komorebi exits ([`Backend::apply_look`]). The effect komorebi builds
//! (`nix/kde-rounded-corners.nix`) also takes a color for each window, which the backend gives it
//! by KWin's UUID of the window ([`Backend::set_border_colours`]).
//!
//! Wallpapers are not KWin's: the Plasma shell, another process, keeps them, one for each screen
//! and not for each desktop. The backend asks the shell over D-Bus to show a monitor's wallpaper
//! ([`Backend::set_wallpaper`]), without holding up the window manager, and tries again for a
//! while when the shell is not there yet.
//!
//! `docs/dev/PLATFORM.md` has the D-Bus names, the files and the environment variables involved,
//! and `docs/dev/DESIGN.md` the reasons for this design.

#![deny(missing_docs)]

mod effects;
mod kwinrc;
pub mod state;
mod wallpaper;

use crate::DATA_DIR;
use crate::backend::Backend;
use crate::backend::Look;
use crate::backend::MonitorInfo;
use crate::backend::WindowInfo;
use crate::core::Rect;
use crate::window_manager_event_listener;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use futures_util::StreamExt;
use parking_lot::Condvar;
use parking_lot::Mutex;
use serde_json::Value;
use serde_json::json;
use state::NeedsDesktops;
use state::NeedsPark;
use state::Output;
use state::PARK_DESKTOP_NAME;
use state::Table;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use tokio::runtime::Handle;
use tokio::sync::Notify;
use zbus::Connection;
use zbus::interface;
use zbus::proxy::CacheProperties;
use zbus::proxy::Proxy;

const SERVICE: &str = "org.komorebi.Bridge";
const PATH: &str = "/org/komorebi/Bridge";
const POLL_PATH: &str = "/org/komorebi/Bridge/Poll";
const IFACE: &str = "org.komorebi.Bridge";
const POLL_IFACE: &str = "org.komorebi.Bridge.Poll";
const SCRIPT_NAME: &str = "komorebi-bridge";
const BRIDGE_JS: &str = include_str!("bridge.js");

/// How long a `Poll` is held open when there is nothing to send. Below QtDBus's 25 second call
/// timeout, which would otherwise fail the call.
const HOLD_MS: u64 = 10_000;

/// Commands that arrive within this long of each other go out as one batch
const COALESCE: Duration = Duration::from_millis(1);

/// The state the D-Bus objects, the backend and the thread that runs the connection share
struct Shared {
    table: Mutex<Table>,
    /// commands for the script, oldest first
    queue: Mutex<VecDeque<Value>>,
    wake: Notify,
    batch: AtomicU64,
    /// how many times KWin has left the session bus. A `Poll` held since before then was made by a
    /// script that went with its KWin, and is answered with nothing rather than with the commands
    kwin_lost: AtomicU64,
    /// set once the script has reported its first snapshot
    ready: (Mutex<bool>, Condvar),
    cursor_waiters: Mutex<HashMap<u64, std::sync::mpsc::Sender<(i32, i32)>>>,
    next_token: AtomicU64,
    handle: Handle,
    /// false once the thread that runs the runtime has ended, which is when KWin is gone: the
    /// runtime is then being shut down, and blocking on it panics
    alive: AtomicBool,
    connection: Mutex<Option<Connection>>,
    /// the user's `kwinrc`, for the settings komorebi changes and puts back
    kwinrc: Option<kwinrc::Kwinrc>,
    /// what komorebi has told the border effect, and has to undo when it exits
    effects: tokio::sync::Mutex<effects::EffectsState>,
    /// the wallpaper last asked for on each output (by name), which is what the shell is made to
    /// show whenever it gets round to it
    wallpapers: Mutex<HashMap<String, std::path::PathBuf>>,
    /// one request to the Plasma shell at a time, so that the last one asked for is the last done
    wallpaper_turn: tokio::sync::Mutex<()>,
    /// whether a failure to set a wallpaper has been said (it is said once)
    wallpaper_failed: AtomicBool,
}

impl Shared {
    /// Run something on the runtime and wait for it, unless the runtime is gone: when KWin has gone
    /// (the session is being torn down), its thread has ended and tokio panics rather than run it
    fn block_on<F: std::future::Future>(&self, future: F) -> eyre::Result<F::Output> {
        if !self.alive.load(Ordering::SeqCst) {
            eyre::bail!("KWin is gone");
        }
        Ok(self.handle.block_on(future))
    }

    fn push(&self, command: Value) {
        self.queue.lock().push_back(command);
        self.wake.notify_one();
    }

    /// Route what the table decided to where it goes
    fn dispatch(&self, outputs: Vec<Output>) {
        for output in outputs {
            match output {
                Output::Event(event) => window_manager_event_listener::send_notification(event),
                Output::Command(command) => self.push(command),
                Output::Cursor { token, x, y } => {
                    if let Some(waiter) = self.cursor_waiters.lock().remove(&token) {
                        let _ = waiter.send((x, y));
                    }
                }
            }
        }
    }

    fn mark_ready(&self) {
        *self.ready.0.lock() = true;
        self.ready.1.notify_all();
    }

    async fn connection(&self) -> eyre::Result<Connection> {
        self.connection
            .lock()
            .clone()
            .ok_or_eyre("not connected to the session bus")
    }
}

/// script -> daemon. `spawn = false` makes zbus handle these inline and in the order received (the
/// default, a task per call, was seen to reorder events); the price is that a handler that waits
/// blocks every later call on the connection, so these never wait.
struct Events {
    shared: Arc<Shared>,
}

#[interface(name = "org.komorebi.Bridge", spawn = false)]
impl Events {
    async fn event(&self, json: String) {
        let value: Value = match serde_json::from_str(&json) {
            Ok(value) => value,
            Err(error) => {
                tracing::error!("bad event from the bridge ({error}): {json}");
                return;
            }
        };

        let is_snapshot = value["type"] == "snapshot";
        let outputs = self.shared.table.lock().handle(&value, Instant::now());
        self.shared.dispatch(outputs);

        if is_snapshot {
            self.shared.mark_ready();
            // a parking desktop in a snapshot is one nobody is using: from an earlier run, or one
            // that KWin kept in its configuration when it restarted under this one
            sweep_stale_park(&self.shared);
        }
    }
}

/// daemon -> script, on its own object so that the held call is spawned and cannot block `Events`
struct Poll {
    shared: Arc<Shared>,
}

#[interface(name = "org.komorebi.Bridge.Poll")]
impl Poll {
    /// `args` is JSON `{"hold_ms": n}`. Returns a JSON array of command batches: as soon as there
    /// are commands, or empty once `hold_ms` has passed, or empty as soon as KWin has left the bus
    /// since the call was made.
    async fn poll(&self, args: String) -> String {
        let hold_ms = serde_json::from_str::<Value>(&args)
            .ok()
            .and_then(|v| v["hold_ms"].as_u64())
            .unwrap_or(HOLD_MS);

        let deadline = tokio::time::Instant::now() + Duration::from_millis(hold_ms);
        let kwin = self.shared.kwin_lost.load(Ordering::SeqCst);

        loop {
            if self.shared.kwin_lost.load(Ordering::SeqCst) != kwin {
                // the script that made this call went with its KWin: the commands, and the wake-up
                // that came for them, are for the script of the KWin that took its place
                self.shared.wake.notify_one();
                return "[]".to_string();
            }

            if !self.shared.queue.lock().is_empty() {
                // let a burst finish arriving, so that a retile is one batch and not twenty
                tokio::time::sleep(COALESCE).await;
                let cmds: Vec<Value> = self.shared.queue.lock().drain(..).collect();
                let batch = self.shared.batch.fetch_add(1, Ordering::Relaxed) + 1;
                return json!([{ "batch": batch, "cmds": cmds }]).to_string();
            }

            if tokio::time::timeout_at(deadline, self.shared.wake.notified())
                .await
                .is_err()
            {
                return "[]".to_string();
            }
        }
    }
}

/// The desktop of a running KWin, read and changed through the script komorebi loads into it.
///
/// The queries are answered from a [`state::Table`] that the script's events keep up to date, so
/// they do not wait for KWin, except [`Backend::cursor_pos`]: only the script can read the pointer,
/// so it is asked, and given 250 ms to answer.
///
/// A command ([`Backend::set_rect`], [`Backend::focus`] and the rest) is queued for the script and
/// returns: the script gets the queue as the answer to its held `Poll` call, and carries the
/// commands out in order. Commands queued within a millisecond of each other go out as one batch,
/// so a retile reaches KWin at once. A command is an error when the table does not know the
/// window; a command for a window that has just closed is sent, and the script reports that it
/// could not carry it out, which is logged.
///
/// The script cannot move the pointer, so the pointer follows the focus through KWin's own "Move
/// Mouse to Focus" action, over `kglobalaccel`: it lands in the middle of the window KWin has
/// focused, whatever rectangle [`Backend::center_cursor_in`] is given. A KWin without global
/// shortcuts leaves the pointer where it is.
///
/// It is made by [`connect`].
pub struct KWinBackend {
    shared: Arc<Shared>,
}

/// Connects to the session bus, loads the script into KWin, and waits for it to report what is on
/// the desktop. Returns once that first report has arrived, so that the window manager can read
/// the desktop straight away.
///
/// The connection is served by a thread of its own (`kwin-bridge-main`), which runs until the
/// daemon exits or the session bus goes away. The same thread watches KWin on the bus, and loads
/// the script again into a KWin that has restarted.
///
/// Before it loads the script, it turns on KWin's per-output virtual desktops in the user's
/// `kwinrc` (`[Windows] PerOutputVirtualDesktops`), unless they are on, and has KWin reload its
/// configuration. [`Backend::shutdown`] puts the user's setting back. A `kwinrc` that cannot be
/// written is logged, and komorebi runs on: with more than one monitor, a workspace switch then
/// switches every monitor.
///
/// # Errors
///
/// - The session bus cannot be reached (`DBUS_SESSION_BUS_ADDRESS` is not set, or names a bus
///   that is not there), or `org.komorebi.Bridge` is already owned on it: another komorebi is
///   running in the session.
/// - KWin is not on the session bus, or refuses the script.
/// - The script was loaded but did not report the desktop within 10 seconds. KWin logs a script's
///   errors to the journal (`journalctl --user -t kwin_wayland`).
/// - The script cannot be written to [`struct@DATA_DIR`].
///
/// # Example
///
/// This is how the daemon installs the backend. The VM test `vm-kwin-events`
/// (`nix/tests/vm/suites/kwin-events`) starts the daemon against a real KWin; the example needs
/// one, so it is only compiled here.
///
/// ```rust,no_run
/// use komorebi::backend;
/// use komorebi::kwin;
///
/// let kwin_backend = kwin::connect()?;
/// backend::set_backend(kwin_backend);
/// # Ok::<(), color_eyre::eyre::Report>(())
/// ```
pub fn connect() -> eyre::Result<Arc<KWinBackend>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("kwin-bridge")
        .enable_all()
        .build()?;

    let shared = Arc::new(Shared {
        table: Mutex::new(Table::new()),
        queue: Mutex::new(VecDeque::new()),
        wake: Notify::new(),
        batch: AtomicU64::new(0),
        kwin_lost: AtomicU64::new(0),
        ready: (Mutex::new(false), Condvar::new()),
        cursor_waiters: Mutex::new(HashMap::new()),
        next_token: AtomicU64::new(1),
        handle: runtime.handle().clone(),
        alive: AtomicBool::new(true),
        connection: Mutex::new(None),
        kwinrc: kwinrc::Kwinrc::user(&DATA_DIR),
        effects: tokio::sync::Mutex::new(effects::EffectsState::default()),
        wallpapers: Mutex::new(HashMap::new()),
        wallpaper_turn: tokio::sync::Mutex::new(()),
        wallpaper_failed: AtomicBool::new(false),
    });

    let (started_tx, started_rx) = std::sync::mpsc::channel::<eyre::Result<()>>();
    let thread_shared = shared.clone();

    std::thread::Builder::new()
        .name("kwin-bridge-main".into())
        .spawn(move || {
            let alive = thread_shared.clone();
            runtime.block_on(async move {
                match start(thread_shared.clone()).await {
                    Ok(()) => {
                        let _ = started_tx.send(Ok(()));
                        supervise(thread_shared).await;
                    }
                    Err(error) => {
                        let _ = started_tx.send(Err(error));
                    }
                }
            });
            // the runtime goes when this thread does
            alive.alive.store(false, Ordering::SeqCst);
        })?;

    started_rx
        .recv()
        .map_err(|_| eyre::Report::msg("the bridge thread exited before starting"))??;

    // the script reports the desktop as soon as it runs
    {
        let (lock, condvar) = &shared.ready;
        let mut ready = lock.lock();
        if !*ready {
            condvar.wait_for(&mut ready, Duration::from_secs(10));
        }
        if !*ready {
            eyre::bail!(
                "KWin loaded the bridge script but it never reported in; \
                 check `journalctl --user -t kwin_wayland` for script errors"
            );
        }
    }

    Ok(Arc::new(KWinBackend { shared }))
}

async fn start(shared: Arc<Shared>) -> eyre::Result<()> {
    let connection = zbus::connection::Builder::session()?
        .name(SERVICE)?
        .serve_at(
            PATH,
            Events {
                shared: shared.clone(),
            },
        )?
        .serve_at(
            POLL_PATH,
            Poll {
                shared: shared.clone(),
            },
        )?
        .build()
        .await
        .map_err(|error| {
            eyre::Report::msg(format!(
                "could not connect to the session bus as {SERVICE} \
                 (is another komorebi running, and is DBUS_SESSION_BUS_ADDRESS set?): {error}"
            ))
        })?;

    *shared.connection.lock() = Some(connection.clone());
    enable_per_output_desktops(&shared, &connection).await;
    load_script(&connection).await
}

async fn kwin_reconfigure(connection: &Connection) {
    let result = async {
        let kwin = Proxy::new(connection, "org.kde.KWin", "/KWin", "org.kde.KWin").await?;
        kwin.call_method("reconfigure", &()).await?;
        eyre::Ok(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!("could not ask KWin to reload its configuration: {error}");
    }
}

/// Turn on KWin's per-output virtual desktops, which is how every monitor gets a workspace of its
/// own: without them, switching the desktop switches it on every output. The user's setting is put
/// back when komorebi exits (see [`kwinrc::Kwinrc`]).
async fn enable_per_output_desktops(shared: &Shared, connection: &Connection) {
    let Some(kwinrc) = &shared.kwinrc else {
        tracing::warn!("there is no configuration directory to find kwinrc in");
        return;
    };

    if kwinrc.is_on("Windows", "PerOutputVirtualDesktops") {
        return;
    }

    if let Err(error) = kwinrc.set(
        "Windows",
        &[("PerOutputVirtualDesktops", "true".to_string())],
    ) {
        tracing::warn!(
            "could not turn on per-output virtual desktops ({error}): with more than one monitor, \
             switching workspace will switch them all"
        );
        return;
    }

    kwin_reconfigure(connection).await;
    // the setting is applied as KWin reloads; the script reads the desktops right after
    tokio::time::sleep(Duration::from_millis(300)).await;
    tracing::info!("turned on KWin's per-output virtual desktops");
}

/// Write the bridge to disk, where KWin can read it, and have KWin run it
async fn load_script(connection: &Connection) -> eyre::Result<()> {
    let cfg = json!({
        "service": SERVICE,
        "path": PATH,
        "iface": IFACE,
        "poll_path": POLL_PATH,
        "poll_iface": POLL_IFACE,
        "hold_ms": HOLD_MS,
    });

    std::fs::create_dir_all(&*DATA_DIR)?;
    let path = DATA_DIR.join("bridge.js");
    std::fs::write(&path, format!("var CFG = {cfg};\n{BRIDGE_JS}"))?;

    let scripting = Proxy::new(
        connection,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )
    .await
    .map_err(|e| eyre::Report::msg(format!("KWin is not on the session bus: {e}")))?;

    // a script from an earlier run (komorebi crashed, or was restarted) is replaced
    let _: bool = scripting.call("unloadScript", &(SCRIPT_NAME,)).await?;

    let path = path.to_string_lossy().to_string();
    let id: i32 = scripting
        .call("loadScript", &(path.as_str(), SCRIPT_NAME))
        .await?;

    let script = Proxy::new(
        connection,
        "org.kde.KWin",
        format!("/Scripting/Script{id}"),
        "org.kde.kwin.Script",
    )
    .await?;
    script.call_method("run", &()).await?;

    tracing::info!("loaded the KWin bridge script (script id {id})");
    Ok(())
}

/// Watch KWin itself. If it goes away (a crash, `kwin_wayland --replace`) its windows go with it;
/// when it is back the script has to be loaded again.
async fn supervise(shared: Arc<Shared>) {
    let Ok(connection) = shared.connection().await else {
        return;
    };

    let Ok(dbus) = zbus::fdo::DBusProxy::new(&connection).await else {
        return;
    };
    let Ok(mut owner_changes) = dbus.receive_name_owner_changed().await else {
        return;
    };

    while let Some(signal) = owner_changes.next().await {
        let Ok(args) = signal.args() else { continue };
        if args.name().as_str() != "org.kde.KWin" {
            continue;
        }

        if args.new_owner().is_none() {
            tracing::warn!("KWin went away; its windows are gone");
            shared.kwin_lost.fetch_add(1, Ordering::SeqCst);
            let outputs = shared.table.lock().compositor_lost(Instant::now());
            shared.dispatch(outputs);
            continue;
        }

        tracing::info!("KWin is back; loading the bridge script again");
        let started = Instant::now();
        loop {
            match load_script(&connection).await {
                Ok(()) => break,
                Err(error) if started.elapsed() < Duration::from_secs(15) => {
                    tracing::debug!("KWin is not ready for the script yet: {error}");
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(error) => {
                    tracing::error!("could not load the bridge script into the new KWin: {error}");
                    break;
                }
            }
        }
    }
}

/// Removes the parking desktop an earlier run left behind, once its windows have moved home: the
/// snapshot that found it sent them, and they take a moment to get there
fn sweep_stale_park(shared: &Arc<Shared>) {
    let Some(stale) = shared.table.lock().stale_park().map(String::from) else {
        return;
    };

    let shared = shared.clone();
    shared.handle.clone().spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Err(error) = remove_desktop(&shared, &stale).await {
            tracing::warn!("could not remove the stale parking desktop: {error}");
        }
        shared.table.lock().clear_stale_park();
    });
}

/// Have the Plasma shell show the wallpaper wanted on an output. If the shell is not there (yet),
/// try again for a while: komorebi may well have started before it.
async fn keep_wallpaper(shared: Arc<Shared>, output: String) {
    // once the shell has been found missing it is not waited for again on every request
    let attempts = if shared.wallpaper_failed.load(Ordering::SeqCst) {
        1
    } else {
        8
    };
    let mut wait = Duration::from_millis(500);

    for attempt in 1..=attempts {
        let Some(wanted) = shared.wallpapers.lock().get(&output).cloned() else {
            return;
        };

        let result = {
            let _turn = shared.wallpaper_turn.lock().await;
            match shared.connection().await {
                Ok(connection) => wallpaper::apply(&connection, &output, &wanted).await,
                Err(error) => Err(error),
            }
        };

        match result {
            Ok(changed) => {
                if changed {
                    tracing::info!("the wallpaper of {output} is {}", wanted.display());
                }
                shared.wallpaper_failed.store(false, Ordering::SeqCst);
                return;
            }
            Err(error) if attempt == attempts => {
                if !shared.wallpaper_failed.swap(true, Ordering::SeqCst) {
                    tracing::warn!(
                        "gave up setting the wallpaper of {output} ({error}): is the Plasma shell \
                         running? Wallpapers are left alone until it answers"
                    );
                }
                return;
            }
            Err(error) => {
                tracing::debug!("could not set the wallpaper of {output} yet: {error}");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(8));
            }
        }
    }
}

async fn desktop_manager(shared: &Shared) -> eyre::Result<Proxy<'static>> {
    let connection = shared.connection().await?;
    // uncached: a property read right after createDesktop must not return the list from before it
    Ok(zbus::proxy::Builder::new(&connection)
        .destination("org.kde.KWin")?
        .path("/VirtualDesktopManager")?
        .interface("org.kde.KWin.VirtualDesktopManager")?
        .cache_properties(CacheProperties::No)
        .build()
        .await?)
}

/// `(position, id, name)`; kwin's introspection says `a(iss)` but the value on the wire is `a(uss)`
async fn desktops(manager: &Proxy<'static>) -> eyre::Result<Vec<(u32, String, String)>> {
    Ok(manager.get_property("desktops").await?)
}

/// The id of the desktop windows are hidden on, which is made when there is none: not the one an
/// earlier run left, which is about to go and would take the windows with it
async fn create_park_desktop(shared: &Shared) -> eyre::Result<String> {
    let manager = desktop_manager(shared).await?;

    let stale = shared.table.lock().stale_park().map(String::from);
    let ours = |(_, id, name): &(u32, String, String)| {
        name == PARK_DESKTOP_NAME && Some(id.as_str()) != stale.as_deref()
    };

    if let Some((_, id, _)) = desktops(&manager).await?.into_iter().find(ours) {
        return Ok(id);
    }

    let count: u32 = manager.get_property("count").await?;
    manager
        .call_method("createDesktop", &(count, PARK_DESKTOP_NAME))
        .await?;

    desktops(&manager)
        .await?
        .into_iter()
        .find(ours)
        .map(|(_, id, _)| id)
        .ok_or_eyre("KWin did not create the parking desktop")
}

/// "Desktop 2" is what KWin calls a desktop nobody named, and "Workspace 2" what komorebi calls one
/// it made for a workspace that has no name: neither is anyone's choice.
fn is_placeholder(name: &str) -> bool {
    ["Desktop ", "Workspace "].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Make sure there are desktops for these workspaces, naming the ones that are created. Existing
/// desktops keep their names, if they are somebody's: they may be the user's.
async fn create_workspace_desktops(shared: &Shared, names: &[String]) -> eyre::Result<()> {
    let manager = desktop_manager(shared).await?;
    let existing = desktops(&manager).await?;
    let have = existing
        .iter()
        .filter(|(_, _, name)| name != PARK_DESKTOP_NAME)
        .count();

    for (position, name) in names.iter().enumerate().skip(have) {
        // by position, so that the parking desktop stays last
        manager
            .call_method("createDesktop", &(position as u32, name.as_str()))
            .await?;
    }

    // the ones nobody named take the workspaces' names; the ones somebody did keep theirs
    let nameable = existing
        .iter()
        .filter(|(_, _, name)| name != PARK_DESKTOP_NAME);
    for ((_, id, name), wanted) in nameable.zip(names) {
        if is_placeholder(name) && !is_placeholder(wanted) {
            manager
                .call_method("setDesktopName", &(id.as_str(), wanted.as_str()))
                .await?;
        }
    }

    let after = desktops(&manager).await?;
    shared
        .table
        .lock()
        .set_desktops(after.into_iter().map(|(_, id, name)| (id, name)).collect());
    Ok(())
}

async fn remove_desktop(shared: &Shared, id: &str) -> eyre::Result<()> {
    let manager = desktop_manager(shared).await?;
    manager.call_method("removeDesktop", &(id,)).await?;
    Ok(())
}

/// KWin's own "Move Mouse to Focus" action: the scripting API cannot move the pointer
async fn move_mouse_to_focus(shared: &Shared) -> eyre::Result<()> {
    let connection = shared.connection().await?;
    let component = Proxy::new(
        &connection,
        "org.kde.kglobalaccel",
        "/component/kwin",
        "org.kde.kglobalaccel.Component",
    )
    .await?;
    component
        .call_method("invokeShortcut", &("MoveMouseToFocus",))
        .await?;
    Ok(())
}

fn command_rect(uuid: &str, rect: &Rect) -> Value {
    json!({ "op": "geo", "id": uuid, "x": rect.left, "y": rect.top, "w": rect.right, "h": rect.bottom })
}

impl KWinBackend {
    /// A workspace was asked for that has no desktop: make desktops up to it
    fn make_desktops(&self, count: usize) -> eyre::Result<()> {
        let names: Vec<String> = (1..=count).map(|n| format!("Workspace {n}")).collect();
        self.ensure_workspaces(&names)
    }

    fn command(&self, id: u32, build: impl FnOnce(&str) -> Value) -> eyre::Result<()> {
        let uuid = self
            .shared
            .table
            .lock()
            .uuid(id)
            .ok_or_eyre("there is no such window")?;
        self.shared.push(build(&uuid));
        Ok(())
    }
}

impl Backend for KWinBackend {
    fn windows(&self) -> Vec<WindowInfo> {
        self.shared.table.lock().windows()
    }

    fn window(&self, id: u32) -> Option<WindowInfo> {
        self.shared.table.lock().window(id)
    }

    fn foreground_window(&self) -> Option<u32> {
        self.shared.table.lock().foreground()
    }

    fn monitors(&self) -> eyre::Result<Vec<MonitorInfo>> {
        let monitors = self.shared.table.lock().monitors();
        if monitors.is_empty() {
            eyre::bail!("KWin reported no outputs");
        }
        Ok(monitors)
    }

    fn cursor_pos(&self) -> Option<(i32, i32)> {
        // The script is the only thing that can read the pointer, so ask it, and fall back to where
        // it was last seen if it does not answer in time
        let token = self.shared.next_token.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = std::sync::mpsc::channel();
        self.shared.cursor_waiters.lock().insert(token, sender);
        self.shared.push(json!({ "op": "cursor", "token": token }));

        let answer = receiver.recv_timeout(Duration::from_millis(250)).ok();
        self.shared.cursor_waiters.lock().remove(&token);

        answer.or_else(|| self.shared.table.lock().cursor())
    }

    fn set_rect(&self, id: u32, rect: &Rect) -> eyre::Result<()> {
        tracing::debug!("asking for window {id} to be at {rect:?}");
        self.command(id, |uuid| command_rect(uuid, rect))?;
        self.shared.table.lock().note_requested_rect(id, *rect);
        Ok(())
    }

    fn focus(&self, id: u32, mouse_follows_focus: bool) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "activate", "id": uuid }))?;

        if mouse_follows_focus {
            // the pointer can only follow once the window really is the active one
            let shared = self.shared.clone();
            self.shared.handle.spawn(async move {
                for _ in 0..60 {
                    if shared.table.lock().foreground() == Some(id) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }

                if let Err(error) = move_mouse_to_focus(&shared).await {
                    tracing::debug!("could not move the mouse to the focused window: {error}");
                }
            });
        }

        Ok(())
    }

    fn raise(&self, id: u32) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "raise", "id": uuid }))
    }

    fn hide(&self, id: u32) -> eyre::Result<()> {
        self.shared
            .table
            .lock()
            .uuid(id)
            .ok_or_eyre("there is no such window")?;

        let commands = loop {
            let attempt = self.shared.table.lock().hide_commands(id);
            match attempt {
                Ok(commands) => break commands,
                Err(NeedsPark) => {
                    let shared = self.shared.clone();
                    let park = self
                        .shared
                        .block_on(async move { create_park_desktop(&shared).await })??;
                    self.shared.table.lock().set_park(park);
                }
            }
        };

        for command in commands {
            self.shared.push(command);
        }
        Ok(())
    }

    fn restore(&self, id: u32) -> eyre::Result<()> {
        self.shared
            .table
            .lock()
            .uuid(id)
            .ok_or_eyre("there is no such window")?;

        let commands = self.shared.table.lock().restore_commands(id);
        for command in commands {
            self.shared.push(command);
        }
        Ok(())
    }

    fn ensure_workspaces(&self, names: &[String]) -> eyre::Result<()> {
        let shared = self.shared.clone();
        let names = names.to_vec();
        self.shared
            .block_on(async move { create_workspace_desktops(&shared, &names).await })?
    }

    fn switch_workspace(&self, monitor_id: u32, idx: usize) -> eyre::Result<()> {
        let commands = loop {
            let attempt = self.shared.table.lock().switch_commands(monitor_id, idx);
            match attempt {
                Ok(commands) => break commands,
                Err(NeedsDesktops(count)) => self.make_desktops(count)?,
            }
        };

        for command in commands {
            self.shared.push(command);
        }
        Ok(())
    }

    fn set_window_workspace(&self, id: u32, idx: usize) -> eyre::Result<()> {
        let commands = loop {
            let attempt = self.shared.table.lock().window_workspace_commands(id, idx);
            match attempt {
                Ok(commands) => break commands,
                Err(NeedsDesktops(count)) => self.make_desktops(count)?,
            }
        };

        for command in commands {
            self.shared.push(command);
        }
        Ok(())
    }

    fn window_workspace(&self, id: u32) -> Option<usize> {
        self.shared.table.lock().window_workspace(id)
    }

    fn current_workspaces(&self) -> Vec<(u32, usize)> {
        self.shared.table.lock().current_workspaces()
    }

    fn minimize(&self, id: u32) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "min", "id": uuid, "v": true }))
    }

    fn unminimize(&self, id: u32) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "min", "id": uuid, "v": false }))
    }

    fn close(&self, id: u32) -> eyre::Result<()> {
        self.command(id, |uuid| json!({ "op": "close", "id": uuid }))
    }

    fn maximize(&self, id: u32, maximize: bool) -> eyre::Result<()> {
        self.command(
            id,
            |uuid| json!({ "op": "maximize", "id": uuid, "v": maximize }),
        )
    }

    fn center_cursor_in(&self, _rect: &Rect) -> eyre::Result<()> {
        // KWin can center the pointer on the active window and nothing else; the WM calls this
        // right after focusing the window in question
        let shared = self.shared.clone();
        self.shared.handle.spawn(async move {
            if let Err(error) = move_mouse_to_focus(&shared).await {
                tracing::debug!("could not move the mouse to the focused window: {error}");
            }
        });
        Ok(())
    }

    fn apply_look(&self, look: &Look) -> eyre::Result<bool> {
        let shared = self.shared.clone();
        let look = look.clone();

        self.shared.block_on(async move {
            let Some(kwinrc) = &shared.kwinrc else {
                return Ok(false);
            };
            let connection = shared.connection().await?;
            let mut effects = shared.effects.lock().await;
            effects.apply(&connection, kwinrc, &look).await
        })?
    }

    fn set_opacity(&self, id: u32, alpha: u8) -> eyre::Result<()> {
        self.command(
            id,
            |uuid| json!({ "op": "opacity", "id": uuid, "v": f64::from(alpha) / 255.0 }),
        )
    }

    fn set_focus_follows_mouse(&self, enable: bool) -> eyre::Result<()> {
        let shared = self.shared.clone();

        self.shared.block_on(async move {
            let Some(kwinrc) = &shared.kwinrc else {
                eyre::bail!("there is no configuration directory to find kwinrc in");
            };

            if enable {
                kwinrc.set(
                    "Windows",
                    &[
                        ("FocusPolicy", "1".to_string()),
                        ("DelayFocusInterval", "0".to_string()),
                    ],
                )?;
            } else {
                kwinrc.restore_keys("Windows", &["FocusPolicy", "DelayFocusInterval"])?;
            }

            let connection = shared.connection().await?;
            kwin_reconfigure(&connection).await;
            Ok(())
        })?
    }

    fn set_wallpaper(&self, monitor_id: u32, path: &std::path::Path) -> eyre::Result<()> {
        if !self.shared.alive.load(Ordering::SeqCst) {
            eyre::bail!("KWin is gone");
        }

        let output = self
            .shared
            .table
            .lock()
            .monitors()
            .into_iter()
            .find(|m| m.id == monitor_id)
            .map(|m| m.name)
            .ok_or_eyre("there is no such monitor")?;

        if !path.is_file() {
            eyre::bail!("{} is not a file", path.display());
        }

        self.shared
            .wallpapers
            .lock()
            .insert(output.clone(), path.to_path_buf());

        // Plasma's shell is another process, and may be slow, or still starting: do not hold up
        // the window manager for it. The request that runs last finds the newest wish. (Asked
        // for every time a workspace comes into view, because the user may have changed the
        // wallpaper by hand since: the shell is asked what it shows, and only told if it differs.)
        let shared = self.shared.clone();
        self.shared
            .handle
            .spawn(async move { keep_wallpaper(shared, output).await });

        Ok(())
    }

    fn set_border_colours(&self, colours: &[(u32, u32)]) -> eyre::Result<bool> {
        let shared = self.shared.clone();

        let named: Vec<(String, u32)> = {
            let table = shared.table.lock();
            colours
                .iter()
                .filter_map(|(id, rgb)| table.uuid(*id).map(|uuid| (uuid, *rgb)))
                .collect()
        };

        self.shared.block_on(async move {
            let connection = shared.connection().await?;
            let mut effects = shared.effects.lock().await;
            Ok(effects.set_window_colours(&connection, &named).await)
        })?
    }

    fn shutdown(&self) {
        // the window manager has already restored every window it hid. What is left is the parking
        // desktop, which KWin would otherwise keep in the user's configuration, and the script,
        // which KWin would otherwise keep running, reporting to nobody
        let park = self.shared.table.lock().park_desktop().map(String::from);
        let shared = self.shared.clone();

        let told_kwin = self.shared.block_on(async move {
            // let the restoring commands reach the script before the desktop goes
            for _ in 0..40 {
                if shared.queue.lock().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;

            if let Some(park) = park
                && let Err(error) = remove_desktop(&shared, &park).await
            {
                tracing::warn!("could not remove the parking desktop: {error}");
            }

            // the effects komorebi loaded, and every setting of KWin's it changed
            if let Ok(connection) = shared.connection().await
                && let Some(kwinrc) = &shared.kwinrc
            {
                shared
                    .effects
                    .lock()
                    .await
                    .finish(&connection, kwinrc)
                    .await;

                match kwinrc.restore() {
                    Ok(true) => kwin_reconfigure(&connection).await,
                    Ok(false) => {}
                    Err(error) => tracing::warn!("could not put kwinrc back as it was: {error}"),
                }
            }

            if let Ok(connection) = shared.connection().await
                && let Ok(scripting) = Proxy::new(
                    &connection,
                    "org.kde.KWin",
                    "/Scripting",
                    "org.kde.kwin.Scripting",
                )
                .await
            {
                let _: Result<bool, _> = scripting.call("unloadScript", &(SCRIPT_NAME,)).await;
            }
        });

        // KWin is gone (the session is being torn down, so this is not the way it usually ends):
        // there is nobody to tell, but what komorebi changed in `kwinrc` is the user's, and a file
        // can be put back without KWin
        if told_kwin.is_err()
            && let Some(kwinrc) = &self.shared.kwinrc
            && let Err(error) = kwinrc.restore()
        {
            tracing::warn!("could not put kwinrc back as it was: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend connected to nothing: the table, the queue and the `Poll` object, on a runtime of
    /// the test's own
    fn offline() -> (tokio::runtime::Runtime, KWinBackend) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();

        let shared = Arc::new(Shared {
            table: Mutex::new(Table::new()),
            queue: Mutex::new(VecDeque::new()),
            wake: Notify::new(),
            batch: AtomicU64::new(0),
            kwin_lost: AtomicU64::new(0),
            ready: (Mutex::new(false), Condvar::new()),
            cursor_waiters: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
            handle: runtime.handle().clone(),
            alive: AtomicBool::new(true),
            connection: Mutex::new(None),
            kwinrc: None,
            effects: tokio::sync::Mutex::new(effects::EffectsState::default()),
            wallpapers: Mutex::new(HashMap::new()),
            wallpaper_turn: tokio::sync::Mutex::new(()),
            wallpaper_failed: AtomicBool::new(false),
        });

        (runtime, KWinBackend { shared })
    }

    /// The answer the bridge's next `Poll` gets
    fn next_poll(runtime: &tokio::runtime::Runtime, backend: &KWinBackend) -> Value {
        let poll = Poll {
            shared: backend.shared.clone(),
        };
        let reply = runtime.block_on(poll.poll(json!({ "hold_ms": 0 }).to_string()));
        serde_json::from_str(&reply).unwrap()
    }

    /// The commands of a session as the backend sends them: `bridge.test.js` answers the bridge's
    /// `Poll` with each batch once the bridge has sent the events up to its `after_seq`
    #[test]
    fn the_commands_of_a_session_go_to_the_bridge_as_the_bridges_tests_answer_it() {
        let (runtime, backend) = offline();
        let events: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/events.json")).unwrap();
        let commands: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/commands.json")).unwrap();

        // what the window manager asks of the backend, batch by batch
        let asks: [fn(&KWinBackend); 4] = [
            |backend| {
                // nothing answers the query here, so the pointer is where the snapshot had it
                assert_eq!(backend.cursor_pos(), Some((481, 525)));
            },
            |backend| {
                let left = |left| Rect {
                    left,
                    top: 0,
                    right: 960,
                    bottom: 1050,
                };
                // the editor is 1, the terminal 2 and the dialog 3
                backend.set_rect(1, &left(0)).unwrap();
                backend.focus(1, false).unwrap();
                backend.raise(1).unwrap();
                backend.minimize(1).unwrap();
                backend.unminimize(1).unwrap();
                backend.maximize(1, true).unwrap();
                backend.maximize(1, false).unwrap();
                backend.close(3).unwrap();
                // the terminal has closed, and the table still knows it: the bridge gives it back
                backend.set_rect(2, &left(960)).unwrap();
            },
            |backend| {
                // the editor goes to the second workspace before the monitor is switched to it
                let monitor = backend.monitors().unwrap()[0].id;
                backend.set_window_workspace(1, 1).unwrap();
                backend.switch_workspace(monitor, 1).unwrap();
                assert_eq!(backend.window_workspace(1), Some(1));
                assert_eq!(backend.current_workspaces(), [(monitor, 1)]);
            },
            |backend| {
                // 51 of 255 is a fifth
                backend.set_opacity(1, 51).unwrap();
            },
        ];
        assert_eq!(asks.len(), commands.len());

        let mut handled = 0;
        for (command, ask) in commands.iter().zip(asks) {
            // the table, as the events the bridge had sent by then left it
            let after = command["after_seq"].as_u64().unwrap() as usize;
            for event in &events[handled..after] {
                backend.shared.table.lock().handle(event, Instant::now());
            }
            handled = after;

            ask(&backend);
            assert_eq!(next_poll(&runtime, &backend), json!([command["batch"]]));
        }
    }

    #[test]
    fn a_command_about_a_window_the_table_does_not_know_is_an_error_and_sends_nothing() {
        let (runtime, backend) = offline();

        assert!(backend.focus(7, false).is_err());
        assert!(backend.hide(7).is_err());
        assert!(backend.restore(7).is_err());
        assert_eq!(next_poll(&runtime, &backend), json!([]));
    }

    #[test]
    fn hiding_a_window_sends_it_to_the_parking_desktop_and_restoring_it_sends_it_home() {
        let (runtime, backend) = offline();
        {
            let mut table = backend.shared.table.lock();
            table.handle(
                &json!({
                    "type": "snapshot",
                    "windows": [{
                        "id": "{a}", "pid": 0, "class": "foot", "caption": "a terminal",
                        "normal": true, "minimized": false, "desktops": ["d1"],
                        "geo": { "x": 0, "y": 0, "w": 800, "h": 600 },
                    }],
                    "screens": [],
                    "desktops": [
                        { "id": "d1", "name": "Desktop 1" },
                        { "id": "p1", "name": PARK_DESKTOP_NAME },
                    ],
                    "current_desktops": [],
                    "active": null,
                    "cursor": { "x": 0, "y": 0 },
                }),
                Instant::now(),
            );
            // a snapshot that has the parking desktop has one left from an earlier run; this
            // run's is the one it makes itself
            table.clear_stale_park();
            table.set_park(String::from("p1"));
        }

        backend.hide(1).unwrap();
        assert_eq!(
            next_poll(&runtime, &backend),
            json!([{ "batch": 1, "cmds": [{ "op": "desktop", "id": "{a}", "v": "p1" }] }])
        );

        backend.restore(1).unwrap();
        assert_eq!(
            next_poll(&runtime, &backend),
            json!([{ "batch": 2, "cmds": [{ "op": "desktop", "id": "{a}", "v": "d1" }] }])
        );

        // a window that is not hidden is not sent anywhere
        backend.restore(1).unwrap();
        assert_eq!(next_poll(&runtime, &backend), json!([]));
    }

    #[test]
    fn a_wallpaper_is_refused_for_a_monitor_kwin_lacks_and_for_a_path_that_is_no_file() {
        let (_runtime, backend) = offline();

        // no snapshot yet: there is no monitor
        let nowhere = backend.set_wallpaper(1, std::path::Path::new("/nonexistent/a.png"));
        assert!(nowhere.unwrap_err().to_string().contains("no such monitor"));

        let events: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/events.json")).unwrap();
        backend
            .shared
            .table
            .lock()
            .handle(&events[0], Instant::now());
        let monitor = backend.monitors().unwrap()[0].id;

        let missing = backend.set_wallpaper(monitor, std::path::Path::new("/nonexistent/a.png"));
        assert!(missing.unwrap_err().to_string().contains("is not a file"));
        assert!(backend.shared.wallpapers.lock().is_empty());
    }

    /// zbus does not end a held call when its caller leaves the bus, so the `Poll` of the bridge of
    /// a KWin that has gone is still waiting when the next bridge calls. Were it given the
    /// commands, they would be answered to nobody and lost.
    #[test]
    fn a_poll_held_by_the_bridge_of_a_kwin_that_has_gone_gets_no_commands() {
        let (runtime, backend) = offline();
        let poll = || {
            let poll = Poll {
                shared: backend.shared.clone(),
            };
            async move { poll.poll(json!({ "hold_ms": 10_000 }).to_string()).await }
        };

        runtime.block_on(async {
            let old = tokio::spawn(poll());
            tokio::time::sleep(Duration::from_millis(10)).await;

            // KWin leaves the bus, and the bridge of the KWin that takes its place calls
            backend.shared.kwin_lost.fetch_add(1, Ordering::SeqCst);
            let new = tokio::spawn(poll());
            tokio::time::sleep(Duration::from_millis(10)).await;

            backend.shared.push(json!({ "op": "cursor", "token": 1 }));

            assert_eq!(old.await.unwrap(), "[]");
            let reply: Value = serde_json::from_str(&new.await.unwrap()).unwrap();
            assert_eq!(reply[0]["cmds"], json!([{ "op": "cursor", "token": 1 }]));
        });
    }

    #[test]
    fn a_poll_with_nothing_to_send_is_answered_with_nothing_once_its_hold_is_over() {
        let (runtime, backend) = offline();

        assert_eq!(next_poll(&runtime, &backend), json!([]));
    }

    #[test]
    fn a_desktop_nobody_named_is_a_placeholder() {
        assert!(is_placeholder("Desktop 1"));
        assert!(is_placeholder("Desktop 12"));
        assert!(is_placeholder("Workspace 3"));
    }

    #[test]
    fn a_name_somebody_chose_is_not() {
        for name in [
            "",
            "Desktop",
            "Desktop ",
            "Desktop 1b",
            "Mail",
            "My Desktop 1",
            "terminal",
        ] {
            assert!(!is_placeholder(name), "{name:?}");
        }
    }
}
