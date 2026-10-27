//! The engine's keys, through KDE's global shortcut daemon.
//!
//! [`Engine`] decides which combos komorebi holds and what a press of each does; it does no I/O.
//! This module carries its decisions out on the session bus, where KWin 6.8 runs the global
//! shortcut daemon (`org.kde.kglobalaccel`, inside `kwin_wayland`):
//!
//! ```text
//!   komorebi (this module)                         kglobalaccel, in KWin
//!   ----------------------                         ---------------------
//!   Activate(combo)    -- doRegister(id) ------->  the action exists, with no key
//!                      -- setShortcutKeys(id, ->  the key is grabbed: swallowed, and reported
//!                         [[combo]], 6)
//!   Deactivate(combo)  -- setInactive(id) ------>  the key reaches the focused window again
//!   engine.on_pressed  <- globalShortcutPressed --  a press of a grabbed key
//!   engine.on_released <- globalShortcutReleased -  its release
//! ```
//!
//! komorebi owns the component [`COMPONENT`] in kglobalaccel, with one action per combo, named
//! after the combo as KDE writes it (`Alt+H`): an action's id is `["komorebi", "Alt+H", "komorebi",
//! "Alt+H"]`. The engine's effects become [`Call`]s ([`Registry`]), and the calls of one event are
//! sent together, without waiting for each answer; only the calls for one action wait for each
//! other. kglobalaccel also sends `globalShortcutRepeated` while a key is held; nothing listens to
//! it, so a held key runs its binding once.
//!
//! A command the engine runs is started as a process of its own, which komorebi does not wait
//! for ([`command`]): `komorebic` with the words of a `komorebic` command, when they need no shell,
//! and `sh -c` with anything else.
//!
//! # Modes
//!
//! A mode is the engine's alone: entering one is a set of deactivations (the keys of the mode
//! left, which reach the focused window again) and activations (the keys of the mode entered),
//! carried out like any others, and the command the mode runs when it is entered. The daemon
//! logs the mode it is in after each change (`key bindings: the mode is now resize`). The keys
//! of a mode, a bare `H` or `Escape` among them, are swallowed only while the mode is the one
//! the engine is in. A start puts the engine in the default mode, whatever mode an earlier
//! komorebi was in.
//!
//! # Chord chains
//!
//! The first combo of a chain is a key like any other. When it is pressed, the engine lets go
//! of every key but those that can come next and Escape, and the transport sleeps until the
//! chain's [deadline](Engine::deadline), the chord timeout after the last combo of the chain:
//! then [`Engine::on_tick`] abandons the chain and the keys of the mode are grabbed again. A key
//! that is not the next of the chain is not grabbed, so it reaches the focused window and the
//! chain goes on until the timeout. kglobalaccel's own key sequences are not used: they do not
//! fire on KWin 6.8 (`docs/dev/PLATFORM.md`).
//!
//! # Start and stop
//!
//! kglobalaccel keeps an action, and the grab of its key, until it is unregistered: a komorebi
//! that was killed leaves its keys swallowed and reporting to nobody. So a start is an idempotent
//! reconcile ([`Registry::startup`]): the actions of the component that a combo of the keymap
//! names are taken over, and released unless the default mode wants them, and every other action
//! of the component is unregistered. A stop ([`stop`]) releases every key and unregisters every
//! action, so that nothing is left behind; [`KWinBackend`](super::KWinBackend) stops the engine
//! when it shuts down, which every way the daemon stops goes through.
//!
//! `docs/dev/PLATFORM.md` has what was measured of kglobalaccel, and the VM test `vm-keys-engine`
//! presses keys in a Plasma session with the daemon running.

use color_eyre::eyre;
use futures_util::StreamExt;
use futures_util::future::join_all;
use komorebi_hotkeys::Keymap;
use komorebi_hotkeys::engine::Effect;
use komorebi_hotkeys::engine::Engine;
use komorebi_hotkeys::engine::EngineOptions;
use komorebi_hotkeys::qt::Combo;
use komorebi_hotkeys::qt::Key;
use komorebi_hotkeys::qt::Mods;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::mpsc;
use zbus::Connection;
use zbus::proxy::Proxy;
use zbus::zvariant::OwnedObjectPath;

/// The name of komorebi's component in kglobalaccel.
pub const COMPONENT: &str = "komorebi";

const SERVICE: &str = "org.kde.kglobalaccel";
const PATH: &str = "/kglobalaccel";
const IFACE: &str = "org.kde.KGlobalAccel";
const COMPONENT_IFACE: &str = "org.kde.kglobalaccel.Component";

/// `SetPresent | NoAutoloading`: the key is grabbed now, and not replaced by one saved in
/// kglobalaccel's configuration
const FLAGS: u32 = 6;

/// A call komorebi makes on kglobalaccel's `/kglobalaccel`, for the action of one combo, or of an
/// action it does not know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    /// `doRegister`: makes the action known, with no key.
    Register(Combo),
    /// `setShortcutKeys(id, [[combo]], 6)`: gives the action its key, and grabs it.
    Grab(Combo),
    /// `setInactive`: lets the key go, so that it reaches the focused window.
    Release(Combo),
    /// `unRegister`: removes the action of that name from the component.
    Unregister(String),
}

impl Call {
    /// Returns the name of the action the call is for: the combo as KDE writes it.
    ///
    /// # Example
    ///
    /// ```rust
    /// use komorebi::kwin::hotkeys::Call;
    /// use komorebi_hotkeys::Format;
    /// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
    ///
    /// let alt_h = Combo::new(Mods::ALT, key_to_qt(Format::Skhd, "h").unwrap());
    /// assert_eq!(Call::Grab(alt_h).action(), "Alt+H");
    /// assert_eq!(Call::Unregister("Meta+Q".into()).action(), "Meta+Q");
    /// ```
    pub fn action(&self) -> String {
        match self {
            Call::Register(combo) | Call::Grab(combo) | Call::Release(combo) => combo.to_string(),
            Call::Unregister(name) => name.clone(),
        }
    }
}

/// The actions komorebi has registered, and the calls that keep kglobalaccel in step with the
/// engine.
///
/// An [`Effect::Activate`] is a [`Call::Grab`], preceded by a [`Call::Register`] the first time
/// the combo is activated; an [`Effect::Deactivate`] is a [`Call::Release`]. Commands are not
/// calls.
///
/// # Example
///
/// ```rust
/// use komorebi::kwin::hotkeys::{Call, Registry};
/// use komorebi_hotkeys::Format;
/// use komorebi_hotkeys::engine::Effect;
/// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
///
/// let alt_h = Combo::new(Mods::ALT, key_to_qt(Format::Skhd, "h").unwrap());
/// let mut registry = Registry::default();
///
/// assert_eq!(
///     registry.calls(&[Effect::Activate(alt_h)]),
///     [Call::Register(alt_h), Call::Grab(alt_h)]
/// );
/// assert_eq!(registry.calls(&[Effect::Deactivate(alt_h)]), [Call::Release(alt_h)]);
/// // registered once: activating it again only grabs it
/// assert_eq!(registry.calls(&[Effect::Activate(alt_h)]), [Call::Grab(alt_h)]);
/// assert_eq!(registry.unregister_all(), [Call::Unregister("Alt+H".into())]);
/// ```
#[derive(Debug, Default)]
pub struct Registry {
    registered: BTreeSet<Combo>,
}

impl Registry {
    /// Returns the calls that take over what an earlier komorebi left in the component.
    ///
    /// `existing` is the names of the component's actions, as kglobalaccel lists them;
    /// `known` maps the name of every combo the keymap can activate to the combo; `start` is
    /// what [`Engine::start`] returned. An existing action that `known` names is taken over:
    /// it is registered already, and released unless `start` activates it. Any other existing
    /// action is unregistered. Then come the calls of `start` itself.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::collections::BTreeMap;
    /// use komorebi::kwin::hotkeys::{Call, Registry};
    /// use komorebi_hotkeys::Format;
    /// use komorebi_hotkeys::engine::Effect;
    /// use komorebi_hotkeys::qt::{Combo, Mods, key_to_qt};
    ///
    /// let combo = |key| Combo::new(Mods::ALT, key_to_qt(Format::Skhd, key).unwrap());
    /// let (h, j, k) = (combo("h"), combo("j"), combo("k"));
    /// let known = BTreeMap::from([h, j, k].map(|c| (c.to_string(), c)));
    /// // a komorebi that was killed left Alt+J grabbed, and Alt+X from another keymap
    /// let existing = ["Alt+J".to_string(), "Alt+X".to_string()];
    ///
    /// let calls = Registry::default().startup(&existing, &known, &[Effect::Activate(h)]);
    ///
    /// assert_eq!(
    ///     calls,
    ///     [
    ///         Call::Release(j),
    ///         Call::Unregister("Alt+X".into()),
    ///         Call::Register(h),
    ///         Call::Grab(h),
    ///     ]
    /// );
    /// ```
    pub fn startup(
        &mut self,
        existing: &[String],
        known: &BTreeMap<String, Combo>,
        start: &[Effect],
    ) -> Vec<Call> {
        let activated: BTreeSet<Combo> = start
            .iter()
            .filter_map(|effect| match effect {
                Effect::Activate(combo) => Some(*combo),
                _ => None,
            })
            .collect();
        let mut calls = Vec::new();
        for name in existing {
            match known.get(name) {
                Some(combo) => {
                    self.registered.insert(*combo);
                    if !activated.contains(combo) {
                        calls.push(Call::Release(*combo));
                    }
                }
                None => calls.push(Call::Unregister(name.clone())),
            }
        }
        calls.extend(self.calls(start));
        calls
    }

    /// Returns the calls that carry out `effects`, in their order.
    pub fn calls(&mut self, effects: &[Effect]) -> Vec<Call> {
        let mut calls = Vec::new();
        for effect in effects {
            match effect {
                Effect::Activate(combo) => {
                    if self.registered.insert(*combo) {
                        calls.push(Call::Register(*combo));
                    }
                    calls.push(Call::Grab(*combo));
                }
                Effect::Deactivate(combo) => calls.push(Call::Release(*combo)),
                Effect::Komorebic(_) | Effect::Shell(_) => {}
            }
        }
        calls
    }

    /// Returns the calls that unregister every action komorebi registered, and forgets them.
    pub fn unregister_all(&mut self) -> Vec<Call> {
        std::mem::take(&mut self.registered)
            .into_iter()
            .map(|combo| Call::Unregister(combo.to_string()))
            .collect()
    }
}

/// Returns every combo the engine can activate for `keymap`, by the name of its action.
///
/// That is every combo of every chord of every mode, the pause binding's, and Escape, which ends
/// a chord chain.
///
/// # Example
///
/// ```rust
/// use komorebi::kwin::hotkeys::combos;
/// use komorebi_hotkeys::{Format, ParseOptions, parse};
///
/// let parsed = parse(
///     Format::Sxhkd,
///     "super + w ; h\n    komorebic focus left\n",
///     &ParseOptions::default(),
/// );
///
/// let names: Vec<String> = combos(&parsed.keymap).into_keys().collect();
///
/// assert_eq!(names, ["Esc", "H", "Meta+W"]);
/// ```
pub fn combos(keymap: &Keymap) -> BTreeMap<String, Combo> {
    let mut combos: BTreeSet<Combo> = keymap
        .modes
        .iter()
        .flat_map(|mode| &mode.bindings)
        .flat_map(|binding| binding.chord.iter().copied())
        .collect();
    combos.extend(keymap.pause.as_ref().map(|pause| pause.combo));
    combos.insert(Combo::new(Mods::NONE, Key::ESCAPE));
    combos.into_iter().map(|c| (c.to_string(), c)).collect()
}

/// Returns the process that runs the command of an effect, or `None` for an effect that is not
/// a command.
///
/// An [`Effect::Komorebic`] runs `komorebic` with the words of the command, when they are made of
/// letters, digits and `_./:=,+@-` only, and through `sh -c` otherwise. `komorebic` is the one
/// next to the running `komorebi`, when there is one, so that a key runs the `komorebic` of the
/// same build; otherwise it is looked up on `PATH`. An [`Effect::Shell`] runs through `sh -c`.
///
/// # Example
///
/// ```rust
/// use komorebi::kwin::hotkeys::command;
/// use komorebi_hotkeys::engine::Effect;
///
/// let words = |effect| {
///     let command = command(&effect).unwrap();
///     let program = command.get_program().to_string_lossy().into_owned();
///     let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
///     (program, args)
/// };
///
/// let (program, args) = words(Effect::Komorebic("focus left".into()));
/// assert!(program.ends_with("komorebic"));
/// assert_eq!(args, ["focus", "left"]);
///
/// let (program, args) = words(Effect::Komorebic("workspace-name 0 \"my one\"".into()));
/// assert_eq!((program.as_str(), args[0].as_str()), ("sh", "-c"));
/// assert_eq!(args[1], "komorebic workspace-name 0 \"my one\"");
///
/// let (program, args) = words(Effect::Shell("notify-send hi".into()));
/// assert_eq!((program.as_str(), args), ("sh", vec!["-c".into(), "notify-send hi".into()]));
/// ```
pub fn command(effect: &Effect) -> Option<std::process::Command> {
    match effect {
        Effect::Komorebic(args)
            if args.chars().all(|c| {
                c.is_ascii_alphanumeric() || c.is_whitespace() || "_./:=,+@-".contains(c)
            }) =>
        {
            let mut command = std::process::Command::new(komorebic());
            command.args(args.split_whitespace());
            Some(command)
        }
        Effect::Komorebic(args) => Some(shell(&format!("komorebic {args}"))),
        Effect::Shell(text) => Some(shell(text)),
        Effect::Activate(_) | Effect::Deactivate(_) => None,
    }
}

fn shell(text: &str) -> std::process::Command {
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg(text);
    command
}

fn komorebic() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("komorebic"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("komorebic"))
}

/// Starts a command and returns at once; a thread waits for it, so that it leaves no zombie.
fn spawn(effect: &Effect) {
    let Some(mut command) = command(effect) else {
        return;
    };
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(error) => tracing::error!("could not run the command of a key ({effect:?}): {error}"),
    }
}

/// What the thread of the keys is told
enum Message {
    Stop(std::sync::mpsc::Sender<()>),
}

/// The engine that runs, if one does: the way to its thread (`kwin-hotkeys`)
static RUNNING: Mutex<Option<mpsc::UnboundedSender<Message>>> = Mutex::new(None);

/// Returns true if and only if an engine runs, started by [`start`] and not stopped since.
pub fn is_running() -> bool {
    RUNNING
        .lock()
        .as_ref()
        .is_some_and(|sender| !sender.is_closed())
}

/// Releases every key, unregisters every action of the component and ends the engine's thread,
/// and returns when that is done, or after 2 seconds. Does nothing when no engine runs.
pub fn stop() {
    let Some(messages) = RUNNING.lock().take() else {
        return;
    };
    let (done, wait) = std::sync::mpsc::channel();
    if messages.send(Message::Stop(done)).is_ok() {
        let _ = wait.recv_timeout(Duration::from_secs(2));
    }
}

/// Starts the engine for `keymap` against kglobalaccel on the session bus, on a thread of its
/// own (`kwin-hotkeys`), and returns once its keys are registered. An engine that runs already
/// is [stopped](stop) first.
///
/// The engine runs until [`stop`] or until the session bus goes away.
///
/// # Errors
///
/// - The session bus cannot be reached.
/// - kglobalaccel is not on it (a KWin started with `--no-global-shortcuts`, or no KWin), or
///   refuses a call.
pub fn start(keymap: Keymap, options: EngineOptions) -> eyre::Result<()> {
    stop();
    let (messages, receiver) = mpsc::unbounded_channel();
    let (started, wait) = std::sync::mpsc::channel::<eyre::Result<()>>();

    std::thread::Builder::new()
        .name("kwin-hotkeys".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = started.send(Err(error.into()));
                    return;
                }
            };
            runtime.block_on(async move {
                if let Err(error) = serve(keymap, options, receiver, started).await {
                    tracing::error!("the keys stopped: {error}");
                }
            });
        })?;

    wait.recv()
        .map_err(|_| eyre::Report::msg("the thread of the keys exited before starting"))??;
    *RUNNING.lock() = Some(messages);
    Ok(())
}

fn action_id(action: &str) -> Vec<String> {
    vec![
        COMPONENT.to_string(),
        action.to_string(),
        COMPONENT.to_string(),
        action.to_string(),
    ]
}

/// Returns at `deadline`, or never
async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending().await,
    }
}

/// Sends the calls, those of different actions without waiting for each other
async fn execute(kglobalaccel: &Proxy<'_>, calls: Vec<Call>) {
    let mut by_action: BTreeMap<String, Vec<Call>> = BTreeMap::new();
    for call in calls {
        by_action.entry(call.action()).or_default().push(call);
    }
    join_all(by_action.into_iter().map(|(action, calls)| async move {
        let id = action_id(&action);
        for call in calls {
            let result = match &call {
                Call::Register(_) => kglobalaccel
                    .call::<_, _, ()>("doRegister", &(&id,))
                    .await
                    .map(|()| None),
                Call::Grab(combo) => {
                    let keys = vec![(vec![combo.to_qt() as i32],)];
                    kglobalaccel
                        .call::<_, _, Vec<(Vec<i32>,)>>("setShortcutKeys", &(&id, &keys, FLAGS))
                        .await
                        .map(|set| (set != keys).then_some(set))
                }
                Call::Release(_) => kglobalaccel
                    .call::<_, _, ()>("setInactive", &(&id,))
                    .await
                    .map(|()| None),
                Call::Unregister(_) => kglobalaccel
                    .call_method("unRegister", &(&id,))
                    .await
                    .map(|_| None),
            };
            match result {
                Ok(None) => tracing::trace!("{call:?}"),
                // kglobalaccel gives a key to one action only
                Ok(Some(set)) => tracing::warn!(
                    "{action} is another shortcut's key, so komorebi does not get it (kglobalaccel set {set:?})"
                ),
                Err(error) => tracing::error!("{call:?} failed: {error}"),
            }
        }
    }))
    .await;
}

async fn serve(
    keymap: Keymap,
    options: EngineOptions,
    mut messages: mpsc::UnboundedReceiver<Message>,
    started: std::sync::mpsc::Sender<eyre::Result<()>>,
) -> eyre::Result<()> {
    let setup = async {
        let connection = Connection::session().await?;
        let kglobalaccel = Proxy::new(&connection, SERVICE, PATH, IFACE).await?;
        Ok::<_, eyre::Report>((connection, kglobalaccel))
    };
    let (connection, kglobalaccel) = match setup.await {
        Ok(setup) => setup,
        Err(error) => {
            let _ = started.send(Err(error));
            return Ok(());
        }
    };

    let known = combos(&keymap);
    let mut engine = Engine::new(keymap, options);
    let mut registry = Registry::default();

    let existing: Vec<String> = kglobalaccel
        .call::<_, _, Vec<Vec<String>>>("allActionsForComponent", &(action_id(""),))
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|id| id.get(1).cloned())
        .collect();
    let start = engine.start();
    let calls = registry.startup(&existing, &known, &start);
    let registered = calls.iter().filter(|c| matches!(c, Call::Grab(_))).count();
    execute(&kglobalaccel, calls).await;

    // The component's object exists once it has an action, which a start with no key to grab (every
    // binding is for another application than the one with the focus) has not given it, so
    // `getComponent` would refuse. A signal is matched by the path it comes from: kglobalaccel puts
    // the object at the name of the component, and the engine hears the presses once there is one.
    let component = async {
        let path = OwnedObjectPath::try_from(format!("/component/{COMPONENT}"))?;
        let component = Proxy::new(&connection, SERVICE, path, COMPONENT_IFACE).await?;
        let pressed = component.receive_signal("globalShortcutPressed").await?;
        let released = component.receive_signal("globalShortcutReleased").await?;
        Ok::<_, eyre::Report>((pressed, released))
    };
    let (mut pressed, mut released) = match component.await {
        Ok(streams) => streams,
        Err(error) => {
            let _ = started.send(Err(error));
            return Ok(());
        }
    };
    tracing::info!(
        "grabbed {registered} keys in kglobalaccel, of the {} the key bindings use",
        known.len()
    );
    let _ = started.send(Ok(()));

    loop {
        let mode = engine.mode().to_string();
        let effects = tokio::select! {
            Some(signal) = pressed.next() => {
                let (component, action, _): (String, String, i64) = signal.body().deserialize()?;
                match known.get(&action) {
                    Some(combo) if component == COMPONENT => {
                        tracing::debug!("{action} pressed");
                        engine.on_pressed(*combo, Instant::now())
                    }
                    _ => continue,
                }
            }
            Some(signal) = released.next() => {
                let (component, action, _): (String, String, i64) = signal.body().deserialize()?;
                match known.get(&action) {
                    Some(combo) if component == COMPONENT => engine.on_released(*combo),
                    _ => continue,
                }
            }
            () = until(engine.deadline()) => engine.on_tick(Instant::now()),
            message = messages.recv() => match message {
                Some(Message::Stop(done)) => {
                    let mut calls = registry.calls(&engine.stop());
                    calls.extend(registry.unregister_all());
                    execute(&kglobalaccel, calls).await;
                    tracing::info!("gave every key back to kglobalaccel");
                    let _ = done.send(());
                    return Ok(());
                }
                None => return Ok(()),
            },
            else => return Ok(()),
        };

        if engine.mode() != mode {
            tracing::info!("key bindings: the mode is now {}", engine.mode());
        }
        for effect in &effects {
            spawn(effect);
        }
        execute(&kglobalaccel, registry.calls(&effects)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use komorebi_hotkeys::Format;
    use komorebi_hotkeys::ParseOptions;
    use komorebi_hotkeys::parse;

    fn keymap(text: &str) -> Keymap {
        parse(Format::Skhd, text, &ParseOptions::default()).keymap
    }

    #[test]
    fn a_start_takes_over_the_keys_of_a_killed_komorebi_and_drops_the_rest() {
        let keymap = keymap("alt - h : komorebic focus left\n:: resize\nresize < h : echo h\n");
        let known = combos(&keymap);
        let mut engine = Engine::new(keymap, EngineOptions::default());
        let start = engine.start();
        let mut registry = Registry::default();

        // Alt+H and H are this keymap's; Alt+Q was another's
        let existing = ["Alt+H", "H", "Alt+Q"].map(String::from);
        let calls = registry.startup(&existing, &known, &start);

        let alt_h = known["Alt+H"];
        assert_eq!(
            calls,
            [
                // H is in the mode resize, which is not the one the engine starts in
                Call::Release(known["H"]),
                Call::Unregister("Alt+Q".into()),
                // registered already: grabbed again, not registered again
                Call::Grab(alt_h),
            ]
        );
        // a stop releases and unregisters what was taken over, too
        let mut stop = registry.calls(&engine.stop());
        stop.extend(registry.unregister_all());
        assert_eq!(
            stop,
            [
                Call::Release(alt_h),
                // in the order of the combos, where no modifier comes first
                Call::Unregister("H".into()),
                Call::Unregister("Alt+H".into()),
            ]
        );
    }

    #[test]
    fn a_start_with_nothing_left_behind_registers_and_grabs_the_default_mode() {
        let keymap = keymap("alt - h : echo h\nalt - l : echo l\n");
        let known = combos(&keymap);
        let mut engine = Engine::new(keymap, EngineOptions::default());

        let calls = Registry::default().startup(&[], &known, &engine.start());

        let (h, l) = (known["Alt+H"], known["Alt+L"]);
        assert_eq!(
            calls,
            [
                Call::Register(h),
                Call::Grab(h),
                Call::Register(l),
                Call::Grab(l)
            ]
        );
    }

    #[test]
    fn entering_a_mode_lets_the_keys_of_the_other_go_and_grabs_its_own() {
        let keymap = keymap(
            ":: resize : notify-send resize\nalt - r ; resize\nalt - h : echo h\n\
             resize < h : echo resize\nresize < escape ; default\n",
        );
        let known = combos(&keymap);
        let mut engine = Engine::new(keymap, EngineOptions::default());
        let mut registry = Registry::default();
        registry.startup(&[], &known, &engine.start());
        let now = Instant::now();

        let effects = engine.on_pressed(known["Alt+R"], now);

        assert_eq!(engine.mode(), "resize");
        assert_eq!(effects[0], Effect::Shell("notify-send resize".into()));
        let (h, escape) = (known["H"], known["Esc"]);
        assert_eq!(
            registry.calls(&effects),
            [
                Call::Release(known["Alt+H"]),
                Call::Release(known["Alt+R"]),
                Call::Register(h),
                Call::Grab(h),
                Call::Register(escape),
                Call::Grab(escape),
            ]
        );

        // back in the default mode: registered once, so only grabbed again
        let effects = engine.on_pressed(escape, now);
        assert_eq!(engine.mode(), "default");
        assert_eq!(
            registry.calls(&effects),
            [
                Call::Release(h),
                Call::Release(escape),
                Call::Grab(known["Alt+H"]),
                Call::Grab(known["Alt+R"]),
            ]
        );
    }

    #[test]
    fn a_chain_grabs_its_next_keys_until_its_deadline() {
        let keymap = parse(
            Format::Sxhkd,
            "alt + w ; h\n    komorebic focus left\nalt + q\n    komorebic close\n",
            &ParseOptions::default(),
        )
        .keymap;
        let known = combos(&keymap);
        let options = EngineOptions {
            chord_timeout: Duration::from_millis(1500),
            ..EngineOptions::default()
        };
        let mut engine = Engine::new(keymap, options);
        let mut registry = Registry::default();
        registry.startup(&[], &known, &engine.start());
        let (alt_q, alt_w, h, escape) = (known["Alt+Q"], known["Alt+W"], known["H"], known["Esc"]);
        let start = Instant::now();

        let effects = engine.on_pressed(alt_w, start);

        assert_eq!(
            registry.calls(&effects),
            [
                Call::Release(alt_q),
                Call::Release(alt_w),
                Call::Register(h),
                Call::Grab(h),
                Call::Register(escape),
                Call::Grab(escape),
            ]
        );
        let deadline = engine.deadline().unwrap();
        assert_eq!(deadline, start + Duration::from_millis(1500));
        assert!(
            engine
                .on_tick(deadline - Duration::from_millis(1))
                .is_empty()
        );

        let effects = engine.on_tick(deadline);

        assert_eq!(engine.deadline(), None);
        assert_eq!(
            registry.calls(&effects),
            [
                Call::Release(h),
                Call::Release(escape),
                Call::Grab(alt_q),
                Call::Grab(alt_w),
            ]
        );
    }

    #[test]
    fn a_press_runs_the_command_and_changes_no_key() {
        let keymap = keymap("alt - h : komorebic retile\n");
        let known = combos(&keymap);
        let mut engine = Engine::new(keymap, EngineOptions::default());
        let mut registry = Registry::default();
        registry.startup(&[], &known, &engine.start());

        let effects = engine.on_pressed(known["Alt+H"], Instant::now());

        assert_eq!(effects, [Effect::Komorebic("retile".into())]);
        assert!(registry.calls(&effects).is_empty());
        let command = command(&effects[0]).unwrap();
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["retile"]);
    }
}
