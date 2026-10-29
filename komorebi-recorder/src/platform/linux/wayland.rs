//! The recorder's connection to the Wayland compositor: which outputs there are, and the
//! compositor's own screencast of one.
//!
//! One connection is made on the first use and kept for the life of the process, with a thread of
//! its own that reads the compositor's events. That thread keeps the list of outputs, including
//! the ones that are plugged in or removed later, so that asking for it ([`outputs`]) is cheap, as
//! the display poll needs: it is made several times a second. A screencast of an output is asked
//! of the compositor from any thread ([`Stream::open`]) and answered by that thread.
//!
//! An output is described by `wl_output`, and by `xdg_output` where the compositor has it: that is
//! where its position and size in the layout (logical pixels, which are komorebi's) come from, and
//! `wl_output` gives the size of its mode (physical pixels, which are the video's).
//!
//! The screencast is KDE's own protocol, `zkde_screencast_unstable_v1`, which gives a PipeWire
//! node and asks the user nothing. KWin tells only the clients it trusts that it has it (see
//! [`screencast_offered`]), so on a compositor that does not trust this program, or is not KWin,
//! there is no screencast, and the error says so.

use color_eyre::eyre;
use parking_lot::Condvar;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use wayland_client::Connection;
use wayland_client::Dispatch;
use wayland_client::Proxy;
use wayland_client::QueueHandle;
use wayland_client::protocol::wl_output;
use wayland_client::protocol::wl_registry;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1;
use wayland_protocols_plasma::screencast::v1::client::zkde_screencast_stream_unstable_v1;
use wayland_protocols_plasma::screencast::v1::client::zkde_screencast_unstable_v1;

/// How long to wait for the compositor to answer a request for a screencast.
const STREAM_WAIT: Duration = Duration::from_secs(10);

/// The cursor is drawn into the picture (`zkde_screencast_unstable_v1.pointer.embedded`).
const POINTER_EMBEDDED: u32 = 2;

/// One output (a monitor) as the compositor describes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Output {
    /// The compositor's number for the output's global, which is what the output is asked for by.
    pub global: u32,
    /// The output's name, which follows the port it is plugged into (`DP-1`). Empty until the
    /// compositor has said it, and always empty on a compositor older than `wl_output` version 4
    /// that has no `xdg_output` to say it.
    pub name: String,
    /// What the output is: its make and model.
    pub description: String,
    /// The size of the output's current mode, in physical pixels.
    pub mode: (i32, i32),
    /// The integer scale the output asks clients for.
    pub scale: i32,
    /// Where the output is in the layout, in logical pixels, if `xdg_output` says.
    pub position: Option<(i32, i32)>,
    /// How large the output is in the layout, in logical pixels, if `xdg_output` says.
    pub logical_size: Option<(i32, i32)>,
}

impl Output {
    /// Returns the name to use for the output: its own, or one made from its global.
    #[must_use]
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("output-{}", self.global)
        } else {
            self.name.clone()
        }
    }

    /// Returns `true` if the compositor has said enough about the output to capture it.
    #[must_use]
    pub fn is_known(&self) -> bool {
        self.mode.0 > 0 && self.mode.1 > 0
    }
}

/// What a request for a screencast is answered with, from the thread that reads the events.
#[derive(Default)]
struct Slot {
    node: Mutex<Option<Result<u32, String>>>,
    answered: Condvar,
    closed: AtomicBool,
}

struct Inner {
    outputs: Mutex<BTreeMap<u32, Output>>,
    wl_outputs: Mutex<HashMap<u32, wl_output::WlOutput>>,
    screencast: Mutex<Option<zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1>>,
    qh: QueueHandle<Events>,
    connection: Connection,
}

/// The state the thread that reads the compositor's events works on.
struct Events {
    inner: Arc<Inner>,
    xdg: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
}

static COMPOSITOR: OnceLock<Result<Arc<Inner>, String>> = OnceLock::new();

fn inner() -> eyre::Result<Arc<Inner>> {
    COMPOSITOR
        .get_or_init(|| connect().map_err(|error| format!("{error:#}")))
        .clone()
        .map_err(|why| eyre::Report::msg(why))
}

fn connect() -> eyre::Result<Arc<Inner>> {
    let connection = Connection::connect_to_env().map_err(|error| {
        eyre::Report::msg(format!("no Wayland compositor to connect to: {error}"))
    })?;
    let mut queue = connection.new_event_queue::<Events>();
    let qh = queue.handle();
    let inner = Arc::new(Inner {
        outputs: Mutex::new(BTreeMap::new()),
        wl_outputs: Mutex::new(HashMap::new()),
        screencast: Mutex::new(None),
        qh: qh.clone(),
        connection: connection.clone(),
    });
    let mut events = Events {
        inner: inner.clone(),
        xdg: None,
    };
    connection.display().get_registry(&qh, ());
    // the globals, then what each output says about itself
    queue.roundtrip(&mut events)?;
    queue.roundtrip(&mut events)?;

    std::thread::Builder::new()
        .name("wayland".to_string())
        .spawn(
            move || {
                while queue.blocking_dispatch(&mut events).is_ok() {}
            },
        )?;
    Ok(inner)
}

impl Dispatch<wl_registry::WlRegistry, ()> for Events {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_output" => {
                    let output =
                        registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, name);
                    if let Some(manager) = &state.xdg {
                        manager.get_xdg_output(&output, qh, name);
                    }
                    state.inner.outputs.lock().insert(
                        name,
                        Output {
                            global: name,
                            scale: 1,
                            ..Output::default()
                        },
                    );
                    state.inner.wl_outputs.lock().insert(name, output);
                }
                "zxdg_output_manager_v1" => {
                    let manager = registry
                        .bind::<zxdg_output_manager_v1::ZxdgOutputManagerV1, _, _>(
                            name,
                            version.min(3),
                            qh,
                            (),
                        );
                    for (global, output) in state.inner.wl_outputs.lock().iter() {
                        manager.get_xdg_output(output, qh, *global);
                    }
                    state.xdg = Some(manager);
                }
                "zkde_screencast_unstable_v1" => {
                    *state.inner.screencast.lock() = Some(
                        registry
                            .bind::<zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1, _, _>(
                                name,
                                version.min(4),
                                qh,
                                (),
                            ),
                    );
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.inner.outputs.lock().remove(&name);
                if let Some(output) = state.inner.wl_outputs.lock().remove(&name) {
                    output.release();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for Events {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut outputs = state.inner.outputs.lock();
        let Some(output) = outputs.get_mut(global) else {
            return;
        };
        match event {
            wl_output::Event::Geometry { make, model, .. } => {
                output.description = format!("{make} {model}").trim().to_string();
            }
            wl_output::Event::Mode {
                flags,
                width,
                height,
                ..
            } => {
                let current = flags
                    .into_result()
                    .is_ok_and(|flags| flags.contains(wl_output::Mode::Current));
                if current {
                    output.mode = (width, height);
                }
            }
            wl_output::Event::Scale { factor } => output.scale = factor.max(1),
            wl_output::Event::Name { name } => output.name = name,
            _ => {}
        }
    }
}

impl Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, ()> for Events {
    fn event(
        _: &mut Self,
        _: &zxdg_output_manager_v1::ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, u32> for Events {
    fn event(
        state: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut outputs = state.inner.outputs.lock();
        let Some(output) = outputs.get_mut(global) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => output.position = Some((x, y)),
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.logical_size = Some((width, height));
            }
            // the name of an output is also said here, for a compositor whose `wl_output` is
            // older than version 4
            zxdg_output_v1::Event::Name { name } if output.name.is_empty() => output.name = name,
            _ => {}
        }
    }
}

impl Dispatch<zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1, ()> for Events {
    fn event(
        _: &mut Self,
        _: &zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1,
        _: zkde_screencast_unstable_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1, Arc<Slot>>
    for Events
{
    fn event(
        _: &mut Self,
        _: &zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1,
        event: zkde_screencast_stream_unstable_v1::Event,
        slot: &Arc<Slot>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zkde_screencast_stream_unstable_v1::Event::Created { node } => {
                *slot.node.lock() = Some(Ok(node));
                slot.answered.notify_all();
            }
            zkde_screencast_stream_unstable_v1::Event::Failed { error } => {
                *slot.node.lock() = Some(Err(error));
                slot.answered.notify_all();
                slot.closed.store(true, Ordering::Relaxed);
            }
            zkde_screencast_stream_unstable_v1::Event::Closed => {
                slot.closed.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

/// Returns the outputs the compositor has now, those it has said enough about to capture, by name.
///
/// # Errors
///
/// Returns an error when there is no compositor to connect to (`WAYLAND_DISPLAY` is not set, or
/// names a socket nobody listens on).
pub fn outputs() -> eyre::Result<Vec<Output>> {
    let inner = inner()?;
    let mut found: Vec<Output> = inner
        .outputs
        .lock()
        .values()
        .filter(|output| output.is_known())
        .cloned()
        .collect();
    found.sort_by_key(Output::label);
    Ok(found)
}

/// Returns `true` if the compositor offers a screencast to this program.
///
/// KWin offers `zkde_screencast_unstable_v1` only to clients whose desktop file asks for it, so
/// `false` on KDE usually means that the file is not installed (see `komorebi-recorder doctor`).
#[must_use]
pub fn screencast_offered() -> bool {
    inner().is_ok_and(|inner| inner.screencast.lock().is_some())
}

/// The compositor's screencast of one output: a PipeWire node that the output's pictures are on.
///
/// Dropping it ends the screencast.
pub struct Stream {
    proxy: zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1,
    slot: Arc<Slot>,
    node: u32,
}

impl Stream {
    /// Asks the compositor to stream the output with the global `global`, with the cursor drawn in.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    ///
    /// - there is no compositor to connect to;
    /// - the compositor does not offer a screencast to this program (see [`screencast_offered`]);
    /// - the output is gone;
    /// - the compositor refuses the stream, or does not answer in ten seconds.
    pub fn open(global: u32) -> eyre::Result<Self> {
        let inner = inner()?;
        let Some(screencast) = inner.screencast.lock().clone() else {
            eyre::bail!(
                "the compositor does not offer a screencast to komorebi-recorder: KWin does that \
                 only for a program whose desktop file asks for zkde_screencast_unstable_v1 \
                 (`komorebi-recorder doctor` says how to install it)"
            );
        };
        let Some(output) = inner.wl_outputs.lock().get(&global).cloned() else {
            eyre::bail!("output {global} is not there any more");
        };

        let slot = Arc::new(Slot::default());
        let proxy = screencast.stream_output(&output, POINTER_EMBEDDED, &inner.qh, slot.clone());
        // the thread that reads the events is asleep until the compositor says something
        inner.connection.flush()?;

        let mut answer = slot.node.lock();
        if answer.is_none() {
            slot.answered.wait_for(&mut answer, STREAM_WAIT);
        }
        match answer.take() {
            Some(Ok(node)) => {
                drop(answer);
                Ok(Self { proxy, slot, node })
            }
            Some(Err(why)) => eyre::bail!("the compositor refused the screencast: {why}"),
            None => eyre::bail!("the compositor did not answer the request for a screencast"),
        }
    }

    /// Returns the PipeWire node the pictures are on.
    #[must_use]
    pub fn node(&self) -> u32 {
        self.node
    }

    /// Returns `true` if the compositor has ended the stream (the output went away, or the
    /// compositor stopped it).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.slot.closed.load(Ordering::Relaxed)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if self.proxy.is_alive() {
            self.proxy.close();
        }
        if let Ok(inner) = inner() {
            let _ = inner.connection.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With a compositor to talk to (a desktop session), the outputs it lists are sound. Without
    /// one, there is nothing to say: the test ends.
    #[test]
    fn the_outputs_of_a_compositor_are_known_by_size_and_name() {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let Ok(outputs) = outputs() else {
            return;
        };
        for output in &outputs {
            assert!(output.is_known(), "{output:?}");
            assert!(output.scale >= 1, "{output:?}");
            assert!(!output.label().is_empty());
        }
    }

    #[test]
    fn an_output_with_no_name_is_labelled_by_its_global() {
        let output = Output {
            global: 12,
            ..Output::default()
        };
        assert_eq!(output.label(), "output-12");
        assert!(!output.is_known());
    }
}
