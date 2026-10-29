//! The pictures of one output, from the compositor's screencast.
//!
//! The compositor puts the output's pictures on a PipeWire node ([`wayland::Stream`]), and this
//! reads the node with GStreamer's `pipewiresrc`, converts each picture to BGRA, and hands it to a
//! [`Sink`]. The pictures are stamped with the master clock: GStreamer's clock is the system's
//! monotonic clock, which is the one the recorder's master clock reads (see
//! [`super::clock`]), so a picture's running time plus the pipeline's base time is the moment it
//! was taken, on the master clock. A picture with no time is stamped when it arrives.
//!
//! The source ends when it is dropped. If the compositor ends the stream, or the pipeline fails,
//! the [`Sink`] is told why, and the segment records it as its error.

use super::wayland;
use crate::capture::gst::Frame;
use crate::capture::gst::Sink;
use crate::clock;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

/// How often the pipeline's bus and the compositor's stream are looked at for an end.
const WATCH: gst::ClockTime = gst::ClockTime::from_mseconds(250);

/// A running screencast of one output: dropping it stops it.
pub struct Source {
    pipeline: gst::Pipeline,
    stop: Arc<AtomicBool>,
    watcher: Option<JoinHandle<()>>,
    // after the pipeline in the order of dropping: the node is read until the pipeline is stopped
    _stream: Arc<wayland::Stream>,
}

impl Source {
    /// Starts the screencast of the output with the Wayland global `global`, and gives its
    /// pictures to `sink`.
    ///
    /// # Errors
    ///
    /// Returns an error when the compositor does not offer the screencast, or refuses it (see
    /// [`wayland::Stream::open`]), or when GStreamer cannot read the node (`pipewiresrc` is a
    /// plugin of PipeWire, and has to be on the plugin path).
    pub fn open(global: u32, sink: Sink) -> eyre::Result<Self> {
        crate::av::init()?;
        let stream = Arc::new(wayland::Stream::open(global)?);

        // the compositor makes pictures when the screen changes, and a picture that has not been
        // taken by the time the next arrives is not worth keeping: two buffers, the older dropped
        let description = format!(
            "pipewiresrc name=src path={} do-timestamp=true always-copy=true ! videoconvert ! \
             video/x-raw,format=BGRA ! appsink name=sink sync=false max-buffers=2 drop=true",
            stream.node()
        );
        let pipeline = gst::parse::launch(&description)
            .wrap_err("could not build the GStreamer pipeline that reads the screencast")?
            .downcast::<gst::Pipeline>()
            .map_err(|_| eyre::Report::msg("the GStreamer pipeline is not a pipeline"))?;
        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| eyre::Report::msg("the pipeline has no sink"))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| eyre::Report::msg("the pipeline's sink is not an appsink"))?;

        let clock_of = pipeline.downgrade();
        let frames = sink.clone();
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let base = clock_of.upgrade().and_then(|pipeline| pipeline.base_time());
                    match frame_of(&sample, base) {
                        Ok(frame) => frames.frame(frame),
                        Err(error) => frames.fail(format!("{error:#}")),
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        pipeline.set_state(gst::State::Playing).wrap_err(
            "GStreamer could not read the screencast (is the PipeWire plugin on the plugin path?)",
        )?;

        let stop = Arc::new(AtomicBool::new(false));
        let watcher = {
            let stop = stop.clone();
            let pipeline = pipeline.clone();
            let stream = stream.clone();
            std::thread::Builder::new()
                .name("screencast".to_string())
                .spawn(move || watch(&pipeline, &stream, &stop, &sink))?
        };

        Ok(Self {
            pipeline,
            stop,
            watcher: Some(watcher),
            _stream: stream,
        })
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.pipeline.set_state(gst::State::Null);
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
    }
}

/// Tells `sink` when the compositor ends the stream, or the pipeline fails.
fn watch(pipeline: &gst::Pipeline, stream: &wayland::Stream, stop: &AtomicBool, sink: &Sink) {
    let Some(bus) = pipeline.bus() else {
        return;
    };
    while !stop.load(Ordering::Relaxed) {
        if stream.is_closed() {
            sink.fail("the compositor ended the screencast (the output went away?)".to_string());
            return;
        }
        let message =
            bus.timed_pop_filtered(WATCH, &[gst::MessageType::Error, gst::MessageType::Eos]);
        match message.as_deref().map(gst::MessageRef::view) {
            Some(gst::MessageView::Error(error)) => {
                sink.fail(format!("the screencast failed: {}", error.error()));
                return;
            }
            Some(gst::MessageView::Eos(_)) => {
                sink.fail("the screencast ended".to_string());
                return;
            }
            _ => {}
        }
    }
}

/// One picture from the pipeline as a [`Frame`], stamped on the master clock.
fn frame_of(sample: &gst::Sample, base: Option<gst::ClockTime>) -> eyre::Result<Frame> {
    let caps = sample
        .caps()
        .ok_or_else(|| eyre::Report::msg("a picture of the screencast has no format"))?;
    let info = gst_video::VideoInfo::from_caps(caps).wrap_err("a picture has no size")?;
    let buffer = sample
        .buffer_owned()
        .ok_or_else(|| eyre::Report::msg("a picture of the screencast has no pixels"))?;
    let (width, height) = (info.width(), info.height());

    // the rows of a picture may be padded, and the segment's frames are not
    let stride = usize::try_from(info.stride()[0]).unwrap_or(0);
    let row = width as usize * 4;
    let buffer = if stride == row {
        buffer
    } else {
        let map = buffer
            .map_readable()
            .wrap_err("could not read a picture of the screencast")?;
        let mut packed = Vec::with_capacity(row * height as usize);
        for line in 0..height as usize {
            let start = line * stride;
            let end = start + row;
            if end > map.len() {
                eyre::bail!("a picture of the screencast is smaller than its size says");
            }
            packed.extend_from_slice(&map[start..end]);
        }
        gst::Buffer::from_slice(packed)
    };

    let arrived = clock::mono_now_ns();
    let pts_ns = match (buffer.pts(), base) {
        // the running time of a picture, and the moment the pipeline started, on the clock that
        // the master clock reads
        (Some(pts), Some(base)) => (pts + base).nseconds(),
        _ => arrived,
    };
    Ok(Frame {
        buffer,
        width,
        height,
        // a picture cannot be from the future
        pts_ns: pts_ns.min(arrived),
    })
}
