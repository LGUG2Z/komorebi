//! The Windows capture backend: Windows.Graphics.Capture into a Media Foundation fragmented MP4.
//!
//! This module exists on Windows only. [`WgcBackend`] is the [`CaptureBackend`] and
//! [`DisplayCapture`] the [`RunningCapture`]; [`crate::platform::capture`] re-exports `WgcBackend`
//! as the host's `Backend`.
//!
//! One [`DisplayCapture`] per monitor. Windows.Graphics.Capture hands frames over on a thread pool
//! thread of its own whenever the picture changes (and not otherwise: a still screen is silent),
//! each stamped with the time it was produced on the performance counter, which is the master
//! clock. Each frame is copied to a texture of ours (the capture's own is recycled as soon as the
//! frame is closed, and the encoder reads asynchronously) and written to the sink writer, which
//! encodes on the GPU where it can. The timestamps in the file are rebased to start at zero with
//! the segment; the exact master-clock time of every frame is in `frames-<display>.csv`, as on
//! macOS, and that is what is trusted for when a frame was shown.
//!
//! # How a segment is recorded
//!
//! 1. [`DisplayCapture::start`] makes a capture item for the monitor, creates one Direct3D 11
//!    device that the capture and the encoder share, and creates the sink writer: H.264 in a
//!    fragmented MP4 with hardware transforms enabled, throttling off, fragments about a second
//!    long and a key frame about every second, at the bitrate [`h264_bitrate`] gives for the
//!    size. It creates the frame log, a pool of three frames and the capture session, and starts
//!    it.
//! 2. Each frame is copied to a texture the capture does not recycle, placed in time by a
//!    [`Placement`], written to the
//!    sink writer with the nominal duration of one frame, and logged by a [`FrameLog`]. A frame
//!    whose content size differs from the display's size at the start ends the capture with an
//!    error: the display changed size.
//! 3. [`RunningCapture::keepalive`] first checks whether the GPU was reset or removed, which ends
//!    the capture with an error. Otherwise it repeats the last frame when the display has shown
//!    nothing new for [`KEEPALIVE_AFTER`](crate::capture::segment::KEEPALIVE_AFTER)
//!    ([`keepalive_due`], with a source that is always taken to be alive).
//! 4. [`RunningCapture::stop`] closes the session and the pool, then takes the lock that frame
//!    callbacks hold, so one in flight finishes first. A still display has produced no frames, so
//!    the segment ends with a repeat of the last frame at the segment's end time, and the sink
//!    writer is finalized.
//!
//! # What this backend does not do
//!
//! - It records H.264 only: [`Codec::Hevc`] is refused when a capture starts, because Media
//!   Foundation's MP4 writer does not write it.
//! - It reports no sign of life ([`RunningCapture::last_signal_mach_ns`] is `None`), because
//!   Windows.Graphics.Capture sends no callbacks while the picture does not change. A stalled
//!   capture is not told by silence. A display that went away is told by its capture item closing,
//!   a GPU that went away by its device, and either by the display poll.
//! - It does not count idle callbacks or dropped frames: [`DisplayResult::idle_callbacks`] and
//!   [`DisplayResult::dropped`] are always `0`.
//!
//! # Status
//!
//! Written from the documentation of the APIs. At the time of writing (2026-10-27) it type-checks
//! against the real bindings and passes clippy for the Windows target, but has not been run on a
//! Windows machine. What is tested is the placement and logging rules it relies on
//! ([`crate::capture::segment`]). `doctor --probe capture` is how it is checked on a real machine
//! (checkpoint W1 in `docs/dev/windows-checkpoints.md`), including a recording that is killed and
//! a static screen.

use super::CaptureBackend;
use super::CaptureConfig;
use super::CaptureStats;
use super::Codec;
use super::DisplayDescriptor;
use super::DisplayResult;
use super::EndReason;
use super::RunningCapture;
use super::frames_file_name;
use super::segment::FrameLog;
use super::segment::Latency;
use super::segment::Placement;
use super::segment::even;
use super::segment::h264_bitrate;
use super::segment::keepalive_due;
use super::video_file_name;
use crate::clock;
use crate::platform::media::pack;
use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::Direct3D11CaptureFramePool;
use windows::Graphics::Capture::GraphicsCaptureItem;
use windows::Graphics::Capture::GraphicsCaptureSession;
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D11::D3D11_BIND_RENDER_TARGET;
use windows::Win32::Graphics::Direct3D11::D3D11_BIND_SHADER_RESOURCE;
use windows::Win32::Graphics::Direct3D11::D3D11_BOX;
use windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_BGRA_SUPPORT;
use windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
use windows::Win32::Graphics::Direct3D11::D3D11_SDK_VERSION;
use windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC;
use windows::Win32::Graphics::Direct3D11::D3D11_USAGE_DEFAULT;
use windows::Win32::Graphics::Direct3D11::D3D11CreateDevice;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;
use windows::Win32::Graphics::Direct3D11::ID3D11Multithread;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Media::MediaFoundation::IMF2DBuffer;
use windows::Win32::Media::MediaFoundation::IMFAttributes;
use windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager;
use windows::Win32::Media::MediaFoundation::IMFMediaBuffer;
use windows::Win32::Media::MediaFoundation::IMFMediaType;
use windows::Win32::Media::MediaFoundation::IMFSample;
use windows::Win32::Media::MediaFoundation::IMFSinkWriter;
use windows::Win32::Media::MediaFoundation::MF_MPEG4SINK_MIN_FRAGMENT_DURATION;
use windows::Win32::Media::MediaFoundation::MF_MT_AVG_BITRATE;
use windows::Win32::Media::MediaFoundation::MF_MT_FRAME_RATE;
use windows::Win32::Media::MediaFoundation::MF_MT_FRAME_SIZE;
use windows::Win32::Media::MediaFoundation::MF_MT_INTERLACE_MODE;
use windows::Win32::Media::MediaFoundation::MF_MT_MAJOR_TYPE;
use windows::Win32::Media::MediaFoundation::MF_MT_MAX_KEYFRAME_SPACING;
use windows::Win32::Media::MediaFoundation::MF_MT_MPEG2_PROFILE;
use windows::Win32::Media::MediaFoundation::MF_MT_PIXEL_ASPECT_RATIO;
use windows::Win32::Media::MediaFoundation::MF_MT_SUBTYPE;
use windows::Win32::Media::MediaFoundation::MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS;
use windows::Win32::Media::MediaFoundation::MF_SINK_WRITER_D3D_MANAGER;
use windows::Win32::Media::MediaFoundation::MF_SINK_WRITER_DISABLE_THROTTLING;
use windows::Win32::Media::MediaFoundation::MF_TRANSCODE_CONTAINERTYPE;
use windows::Win32::Media::MediaFoundation::MFCreateAttributes;
use windows::Win32::Media::MediaFoundation::MFCreateDXGIDeviceManager;
use windows::Win32::Media::MediaFoundation::MFCreateDXGISurfaceBuffer;
use windows::Win32::Media::MediaFoundation::MFCreateMediaType;
use windows::Win32::Media::MediaFoundation::MFCreateSample;
use windows::Win32::Media::MediaFoundation::MFCreateSinkWriterFromURL;
use windows::Win32::Media::MediaFoundation::MFMediaType_Video;
use windows::Win32::Media::MediaFoundation::MFTranscodeContainerType_FMPEG4;
use windows::Win32::Media::MediaFoundation::MFVideoFormat_ARGB32;
use windows::Win32::Media::MediaFoundation::MFVideoFormat_H264;
use windows::Win32::Media::MediaFoundation::MFVideoInterlace_Progressive;
use windows::Win32::Media::MediaFoundation::eAVEncH264VProfile_High;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::Interface;
use windows::core::PCWSTR;

/// How many frames the capture's pool holds: enough that a frame being copied does not make the
/// next one wait.
const POOL_FRAMES: i32 = 3;

/// A fragment of the file is closed about this often (in 100 ns units): what a crash can lose.
const FRAGMENT_100NS: u64 = 10_000_000;

/// The GPU a capture works on: one device for the capture and the encoder to share.
struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// The same device as Windows Runtime sees it, which the frame pool wants.
    winrt: IDirect3DDevice,
    /// Lets the encoder use the same device, so frames stay on the GPU.
    manager: IMFDXGIDeviceManager,
}

impl Gpu {
    fn new() -> eyre::Result<Self> {
        // SAFETY: plain creation calls; every out pointer is valid for the call
        unsafe {
            let (mut device, mut context) = (None, None);
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                Some(&raw mut context),
            )
            .wrap_err("could not create a Direct3D 11 device")?;
            let device = device.ok_or_else(|| eyre::Report::msg("no Direct3D 11 device"))?;
            let context = context.ok_or_else(|| eyre::Report::msg("no Direct3D 11 context"))?;

            // the capture, the copy and the encoder all use this device, from several threads
            let multithread: ID3D11Multithread = device.cast()?;
            let _ = multithread.SetMultithreadProtected(true);

            let dxgi: IDXGIDevice = device.cast()?;
            let winrt: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)
                .wrap_err("could not wrap the device for Windows.Graphics.Capture")?
                .cast()?;

            let (mut token, mut manager) = (0_u32, None);
            MFCreateDXGIDeviceManager(&raw mut token, &raw mut manager)
                .wrap_err("could not create the encoder's device manager")?;
            let manager =
                manager.ok_or_else(|| eyre::Report::msg("no Media Foundation device manager"))?;
            manager
                .ResetDevice(&device, token)
                .wrap_err("could not give the device to the encoder")?;

            Ok(Self {
                device,
                context,
                winrt,
                manager,
            })
        }
    }

    /// Whether the GPU has been reset or removed (a driver update, an undock that took the
    /// discrete GPU): every frame after that is garbage.
    fn removed(&self) -> Option<String> {
        // SAFETY: no preconditions
        let reason = unsafe { self.device.GetDeviceRemovedReason() };
        reason
            .is_err()
            .then(|| format!("the GPU was reset or removed ({reason:?})"))
    }

    /// A texture of our own holding the top left `width` x `height` pixels of `source`, which is
    /// the capture's and goes back to its pool when the frame is closed.
    fn copy(
        &self,
        source: &ID3D11Texture2D,
        width: u32,
        height: u32,
    ) -> eyre::Result<ID3D11Texture2D> {
        let description = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            ..Default::default()
        };

        // SAFETY: the texture is created from a valid description; the copy is between textures
        // of the same device, and the immediate context is protected for use from many threads
        unsafe {
            let mut texture = None;
            self.device
                .CreateTexture2D(&raw const description, None, Some(&raw mut texture))
                .wrap_err("could not allocate a frame")?;
            let texture = texture.ok_or_else(|| eyre::Report::msg("no frame texture"))?;

            let region = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: width,
                bottom: height,
                back: 1,
            };
            self.context.CopySubresourceRegion(
                &texture,
                0,
                0,
                0,
                0,
                source,
                0,
                Some(&raw const region),
            );
            Ok(texture)
        }
    }
}

/// An encoded sink for one segment: H.264 in a fragmented MP4.
struct Writer {
    sink: IMFSinkWriter,
    stream: u32,
    /// How long one frame at the nominal rate lasts, in 100 ns units: what the last frame of a
    /// still display is given.
    frame_100ns: i64,
}

fn input_type(width: u32, height: u32, fps: u32) -> eyre::Result<IMFMediaType> {
    // SAFETY: attribute setters on a media type just created
    unsafe {
        let media = MFCreateMediaType()?;
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32)?;
        media.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height))?;
        media.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
        media.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
        Ok(media)
    }
}

fn output_type(width: u32, height: u32, fps: u32) -> eyre::Result<IMFMediaType> {
    let bitrate = h264_bitrate(width as usize, height as usize, fps);
    // SAFETY: attribute setters on a media type just created
    unsafe {
        let media = MFCreateMediaType()?;
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        media.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
        media.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height))?;
        media.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
        media.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
        media.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
        // a key frame about every second: the fragmented file is cut into fragments at key
        // frames, so this is how much of it a crash can take
        media.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, fps)?;
        Ok(media)
    }
}

impl Writer {
    fn create(path: &Path, gpu: &Gpu, width: u32, height: u32, fps: u32) -> eyre::Result<Self> {
        let fps = fps.max(1);
        let wide: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();

        // SAFETY: attribute setters and creation calls with the arguments they document; `wide`
        // outlives the call that reads it
        unsafe {
            let mut attributes: Option<IMFAttributes> = None;
            MFCreateAttributes(&raw mut attributes, 6)?;
            let attributes =
                attributes.ok_or_else(|| eyre::Report::msg("no Media Foundation attributes"))?;

            // the encoder works on the GPU's textures, on the GPU where it can
            attributes.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &gpu.manager)?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            // a writer that holds frames back to pace the producer would hold back a recording
            attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
            // fragmented, so that what was written before a crash can still be played
            attributes.SetGUID(
                &MF_TRANSCODE_CONTAINERTYPE,
                &MFTranscodeContainerType_FMPEG4,
            )?;
            attributes.SetUINT64(&MF_MPEG4SINK_MIN_FRAGMENT_DURATION, FRAGMENT_100NS)?;

            let sink = MFCreateSinkWriterFromURL(PCWSTR(wide.as_ptr()), None, &attributes)
                .wrap_err_with(|| format!("could not create the video file {}", path.display()))?;

            let stream = sink
                .AddStream(&output_type(width, height, fps)?)
                .wrap_err("the H.264 encoder does not accept this size")?;
            sink.SetInputMediaType(stream, &input_type(width, height, fps)?, None)
                .wrap_err("the encoder cannot take frames of this format")?;
            sink.BeginWriting()
                .wrap_err("could not start writing the video")?;

            Ok(Self {
                sink,
                stream,
                frame_100ns: i64::from(10_000_000 / fps),
            })
        }
    }

    /// A sample of `buffer` shown from `time` for `duration` (both in 100 ns units).
    fn sample(buffer: &IMFMediaBuffer, time: i64, duration: i64) -> eyre::Result<IMFSample> {
        // SAFETY: a sample just created, given a buffer that is live
        unsafe {
            let sample = MFCreateSample()?;
            sample.AddBuffer(buffer)?;
            sample.SetSampleTime(time)?;
            sample.SetSampleDuration(duration)?;
            Ok(sample)
        }
    }

    fn write(&self, buffer: &IMFMediaBuffer, time: i64) -> eyre::Result<()> {
        let sample = Self::sample(buffer, time, self.frame_100ns)?;
        // SAFETY: the writer is writing (`create` began it) and the sample is complete
        unsafe { self.sink.WriteSample(self.stream, &sample) }
            .wrap_err("the encoder rejected a frame")
    }

    fn finish(&self) -> eyre::Result<()> {
        // SAFETY: the writer is writing; this waits for what is queued to be encoded and the file
        // to be closed
        unsafe { self.sink.Finalize() }.wrap_err("could not finish the video file")
    }
}

/// A texture as a Media Foundation buffer the encoder can read without it leaving the GPU.
fn buffer_of(texture: &ID3D11Texture2D) -> eyre::Result<IMFMediaBuffer> {
    // SAFETY: the texture is live and has one subresource
    unsafe {
        let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, 0, false)?;
        // a surface buffer reports no length until it is told, and the converter in front of the
        // encoder reads it
        let length = buffer.cast::<IMF2DBuffer>()?.GetContiguousLength()?;
        buffer.SetCurrentLength(length)?;
        Ok(buffer)
    }
}

/// Everything a frame callback mutates, behind a lock so that `stop` can finish the file safely.
struct State {
    writer: Writer,
    placement: Placement,
    log: FrameLog,
    latency: Latency,
    /// The most recent frame, kept so that a still display can have it repeated.
    last_frame: Option<IMFMediaBuffer>,
    started: bool,
    finished: bool,
}

struct Shared {
    stats: Arc<CaptureStats>,
    state: Mutex<State>,
    gpu: Gpu,
    /// The size the segment is recorded at, in pixels (the source's, made even).
    width: u32,
    height: u32,
    /// The size of the display when the capture began: a frame of any other size means it changed.
    source: SizeInt32,
}

// SAFETY: the Direct3D device is created multithread-protected and free-threaded, the capture's
// callbacks and Media Foundation's sink writer are agile (they work from any thread), and every
// access to the writer, the frame log and the placement is under the `state` mutex.
unsafe impl Send for Shared {}
// SAFETY: see above
unsafe impl Sync for Shared {}

/// What one frame from the capture does: copy it, place it in time, write it, log it.
fn on_frame(shared: &Shared, pool: &Direct3D11CaptureFramePool) {
    let received_ns = clock::mono_now_ns();
    shared.stats.callbacks.fetch_add(1, Ordering::Relaxed);
    shared
        .stats
        .last_callback_mach_ns
        .store(received_ns, Ordering::Relaxed);

    // closed pools and a capture that is being stopped answer with an error: nothing to do
    let Ok(frame) = pool.TryGetNextFrame() else {
        return;
    };
    let outcome = frame_into_file(shared, &frame, received_ns);
    // hands the capture's buffer back to its pool, whatever happened
    let _ = frame.Close();

    if let Err(error) = outcome {
        shared.stats.set_error(format!("{error:#}"), received_ns);
        shared.state.lock().finished = true;
    }
}

fn frame_into_file(
    shared: &Shared,
    frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
    received_ns: u64,
) -> eyre::Result<()> {
    let size = frame.ContentSize()?;
    if size.Width != shared.source.Width || size.Height != shared.source.Height {
        // the display changed size under us (smaller would be cropped wrong, larger would be
        // cut off): the manager sees that too, and starts a new segment
        eyre::bail!(
            "the display changed size to {}x{} while recording {}x{}",
            size.Width,
            size.Height,
            shared.source.Width,
            shared.source.Height
        );
    }

    // the frame's time on the performance counter, in 100 ns units
    let pts_ns = u64::try_from(frame.SystemRelativeTime()?.Duration)
        .unwrap_or(0)
        .saturating_mul(100);

    let surface = frame.Surface()?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    // SAFETY: a capture surface is a texture of the capture's device
    let source: ID3D11Texture2D = unsafe { access.GetInterface()? };
    let texture = shared.gpu.copy(&source, shared.width, shared.height)?;
    let buffer = buffer_of(&texture)?;

    let mut state = shared.state.lock();
    if state.finished {
        return Ok(());
    }

    if !state.started {
        state.started = true;
        shared
            .stats
            .first_pts_mach_ns
            .store(pts_ns, Ordering::Relaxed);
    }

    let placed = state.placement.frame(pts_ns);
    state.writer.write(&buffer, placed.file_time)?;
    state.last_frame = Some(buffer);
    state
        .latency
        .record(received_ns as i64 - placed.pts_ns as i64);
    state.log.frame(placed.pts_ns, received_ns);

    shared.stats.frames.fetch_add(1, Ordering::Relaxed);
    shared
        .stats
        .last_pts_mach_ns
        .store(placed.pts_ns, Ordering::Relaxed);
    Ok(())
}

/// The monitor as a capture item.
fn item_for(display: &DisplayDescriptor) -> eyre::Result<GraphicsCaptureItem> {
    let monitor = crate::platform::capture::monitor_handle(display.id).ok_or_else(|| {
        eyre::Report::msg(format!(
            "display {} is not available for capture (yet)",
            display.id
        ))
    })?;

    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .wrap_err("Windows.Graphics.Capture is not available (Windows 10 1803 or later)")?;
    // SAFETY: a monitor handle that was attached a moment ago
    unsafe { interop.CreateForMonitor(monitor) }.wrap_err("Windows would not capture that display")
}

/// An in-progress capture of one display: one segment, recorded by Windows.Graphics.Capture into a
/// Media Foundation sink writer.
///
/// Built by [`DisplayCapture::start`] and used through [`RunningCapture`]. The video is H.264 at
/// the display's size in physical pixels, rounded to even numbers, and [`DisplayResult::scale`] is
/// `1.0`. The first error a capture reports (its item closing, the GPU being removed, a frame of
/// another size, the encoder rejecting a frame) ends the segment as
/// [`EndReason::StreamError`] when [`DisplayManager`](crate::capture::manager::DisplayManager)
/// next polls. [`stop`](RunningCapture::stop) always returns a [`DisplayResult`]: a segment that
/// never received a frame has its empty file removed and the error `no frames were captured`.
pub struct DisplayCapture {
    descriptor: DisplayDescriptor,
    segment: u32,
    width_px: usize,
    height_px: usize,
    codec: Codec,
    file: String,
    frames_file: String,
    path: std::path::PathBuf,
    /// Kept alive for as long as the capture runs: its `Closed` event is how a removed display is
    /// noticed.
    _item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    shared: Arc<Shared>,
}

impl DisplayCapture {
    /// Starts capturing `display` into `dir` as the given segment.
    ///
    /// Returns once the capture session has started, without waiting for a frame:
    /// `video_t0_mach_ns` is `None` until one arrives. The video is the display's size in physical
    /// pixels rounded to even numbers (a frame is cropped to the top left of the display if the
    /// display's size is odd). The files are `video-<display>[-<n>].mp4` and
    /// `frames-<display>[-<n>].csv` in `dir`.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    ///
    /// - `config.codec` is [`Codec::Hevc`]: only H.264 is recorded on Windows;
    /// - Windows Runtime or Media Foundation cannot be started;
    /// - `display` is no longer attached (`display <id> is not available for capture (yet)`), or
    ///   Windows would not capture it;
    /// - Windows.Graphics.Capture is not available (Windows 10 version 1803 or later is needed);
    /// - Direct3D 11 cannot create a hardware device, or the device cannot be given to the capture
    ///   or the encoder;
    /// - the video file cannot be created, the H.264 encoder does not accept the size, or the
    ///   encoder cannot take frames of the input format, or writing cannot begin;
    /// - the frame log cannot be created in `dir`;
    /// - the capture's frame pool or session cannot be created, or Windows refuses to start the
    ///   capture.
    ///
    /// A failure after the sink writer was created leaves the video file behind. A caller that
    /// retries gets the same segment number again ([`CaptureBackend::start`]), and so the same file
    /// name.
    pub fn start(
        display: &DisplayDescriptor,
        segment: u32,
        config: &CaptureConfig,
        dir: &Path,
    ) -> eyre::Result<Self> {
        if config.codec == Codec::Hevc {
            eyre::bail!(
                "HEVC cannot be recorded on Windows yet (Media Foundation's MP4 writer does not \
                 write it); use --codec h264"
            );
        }

        crate::platform::media::start()?;
        let item = item_for(display)?;
        let size = item.Size()?;
        let (width, height) = (
            u32::try_from(even(f64::from(size.Width)))?,
            u32::try_from(even(f64::from(size.Height)))?,
        );

        let gpu = Gpu::new()?;
        let file = video_file_name(display.id, segment);
        let frames_file = frames_file_name(display.id, segment);
        let path = dir.join(&file);
        let writer = Writer::create(&path, &gpu, width, height, config.fps)?;
        let log = FrameLog::create(&dir.join(&frames_file))
            .wrap_err_with(|| format!("could not create {frames_file}"))?;

        let shared = Arc::new(Shared {
            stats: Arc::new(CaptureStats::default()),
            state: Mutex::new(State {
                writer,
                placement: Placement::default(),
                log,
                latency: Latency::default(),
                last_frame: None,
                started: false,
                finished: false,
            }),
            gpu,
            width,
            height,
            source: size,
        });

        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &shared.gpu.winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            POOL_FRAMES,
            SizeInt32 {
                Width: size.Width,
                Height: size.Height,
            },
        )
        .wrap_err("could not create the capture's frame pool")?;

        let on_arrived = shared.clone();
        pool.FrameArrived(&TypedEventHandler::new(
            move |pool: windows::core::Ref<Direct3D11CaptureFramePool>, _| {
                if let Some(pool) = pool.as_ref() {
                    on_frame(&on_arrived, pool);
                }
                Ok(())
            },
        ))?;

        // the display going away closes its item
        let on_closed = shared.clone();
        item.Closed(&TypedEventHandler::new(move |_, _| {
            on_closed.stats.set_error(
                "the display was closed (removed, or its mode changed)".to_string(),
                clock::mono_now_ns(),
            );
            Ok(())
        }))?;

        let session = pool
            .CreateCaptureSession(&item)
            .wrap_err("could not begin a capture session")?;
        // best effort: each of these needs a newer Windows than the capture itself
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(true);
        session
            .StartCapture()
            .wrap_err("Windows refused to start the capture")?;

        Ok(Self {
            descriptor: display.clone(),
            segment,
            width_px: width as usize,
            height_px: height as usize,
            codec: Codec::H264,
            file,
            frames_file,
            path,
            _item: item,
            pool,
            session,
            shared,
        })
    }

    fn result(
        &self,
        reason: Option<EndReason>,
        end_ns: Option<u64>,
        error: Option<String>,
    ) -> DisplayResult {
        let state = self.shared.state.lock();
        let stats = &self.shared.stats;
        DisplayResult {
            display: self.descriptor.clone(),
            segment: self.segment,
            // frames are native pixels
            scale: 1.0,
            width_px: self.width_px,
            height_px: self.height_px,
            codec: self.codec,
            file: self.file.clone(),
            frames_file: self.frames_file.clone(),
            video_t0_mach_ns: stats.first_pts_mach_ns(),
            video_end_mach_ns: end_ns,
            end_reason: reason,
            frames: state.log.frames(),
            dropped: stats.dropped.load(Ordering::Relaxed),
            // Windows.Graphics.Capture does not call back for a frame with nothing new
            idle_callbacks: 0,
            latency: state.latency.finish(),
            error,
        }
    }
}

impl RunningCapture for DisplayCapture {
    fn display_id(&self) -> u32 {
        self.descriptor.id
    }

    fn segment(&self) -> u32 {
        self.segment
    }

    fn stats(&self) -> Arc<CaptureStats> {
        self.shared.stats.clone()
    }

    fn partial(&self) -> DisplayResult {
        self.result(None, None, self.shared.stats.error())
    }

    /// Windows.Graphics.Capture says nothing while the picture does not change, so silence is not a
    /// sign of death here and a stalled stream cannot be told by it. A display that went away is
    /// told by its item closing, a GPU that went away by its device, and either by the display
    /// poll.
    fn last_signal_mach_ns(&self) -> Option<u64> {
        None
    }

    fn keepalive(&self, now_ns: u64) {
        if let Some(reason) = self.shared.gpu.removed() {
            self.shared.stats.set_error(reason, now_ns);
            self.shared.state.lock().finished = true;
            return;
        }

        let mut state = self.shared.state.lock();
        if !state.started || state.finished {
            return;
        }
        if !keepalive_due(now_ns, state.placement.last_ns(), None) {
            return;
        }

        let Some(last) = state.last_frame.clone() else {
            return;
        };
        let Some(placed) = state.placement.repeat(now_ns) else {
            return;
        };

        if let Err(error) = state.writer.write(&last, placed.file_time) {
            self.shared.stats.set_error(format!("{error:#}"), now_ns);
            state.finished = true;
            return;
        }

        // not a new frame, and marked so
        state.log.repeat(placed.pts_ns, now_ns);
    }

    fn stop(self: Box<Self>, reason: EndReason, end_mach_ns: Option<u64>) -> DisplayResult {
        // no more frames: ends the session, then the pool, then waits out a callback in flight by
        // taking the lock below
        let _ = self.session.Close();
        let _ = self.pool.Close();

        let mut error = None;
        let end_ns = {
            let mut state = self.shared.state.lock();
            state.finished = true;
            state.log.flush();

            // A still display produced no frames, so without an explicit end the file would stop
            // at the last change and late timecodes would fall off it. A segment that ended early
            // (a dead capture) ends when it died, never before its last frame.
            let now = clock::mono_now_ns();
            let end_ns = end_mach_ns
                .unwrap_or(now)
                .min(now)
                .max(state.placement.last_ns());

            if state.started {
                if let Some(last) = state.last_frame.clone()
                    && end_ns > state.placement.last_ns()
                    && let Some(placed) = state.placement.repeat(end_ns)
                {
                    match state.writer.write(&last, placed.file_time) {
                        Ok(()) => state.log.repeat(placed.pts_ns, now),
                        Err(write_error) => error = Some(format!("{write_error:#}")),
                    }
                    state.log.flush();
                }

                if let Err(finish_error) = state.writer.finish() {
                    error.get_or_insert_with(|| format!("{finish_error:#}"));
                }
            } else {
                // never began: there is no video, and the empty file is no use to anyone
                drop(state);
                let _ = std::fs::remove_file(&self.path);
                error = Some("no frames were captured".to_string());
                return self.result(
                    Some(reason),
                    Some(end_ns),
                    error.or_else(|| self.shared.stats.error()),
                );
            }

            end_ns
        };

        let error = error.or_else(|| self.shared.stats.error());
        self.result(Some(reason), Some(end_ns), error)
    }
}

/// The real Windows [`CaptureBackend`].
///
/// [`active_displays`](CaptureBackend::active_displays) is
/// [`crate::platform::capture::active_displays`], which lists the monitors from the window system
/// and needs no permission, and [`start`](CaptureBackend::start) starts a [`DisplayCapture`]. On
/// Windows, [`crate::platform::capture::Backend`] is this type.
pub struct WgcBackend;

impl CaptureBackend for WgcBackend {
    fn active_displays(&self) -> eyre::Result<Vec<DisplayDescriptor>> {
        crate::platform::capture::active_displays()
    }

    fn start(
        &self,
        display: &DisplayDescriptor,
        segment: u32,
        config: &CaptureConfig,
        dir: &Path,
    ) -> eyre::Result<Box<dyn RunningCapture>> {
        Ok(Box::new(DisplayCapture::start(
            display, segment, config, dir,
        )?))
    }
}
