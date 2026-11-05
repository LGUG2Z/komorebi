//! Playing the recording: a clock that can run forwards or backwards at any speed, and an engine
//! that keeps the frame of every display in step with it.
//!
//! The clock ([`transport`]) is plain arithmetic with no window and no decoder, so it is tested to
//! the millisecond. The engine ([`engine`]) owns the decoding on its own thread, since the decoder's
//! objects (`AVFoundation`'s on macOS, Media Foundation's on Windows) belong to the thread that
//! made them, and publishes the frame to show for each display. Whatever draws (the viewer) asks
//! the engine what time it is and what to show, and never decodes itself.
//!
//! | type | role |
//! |---|---|
//! | [`Transport`] | the playhead: position, rate (negative for backwards), loop region; no clock of its own |
//! | [`Playback`] | the transport behind a lock, plus the worker thread that decodes for it |
//! | [`DisplayView`] | what to draw for one display now: loading, a frame, a gap or a failure |
//! | [`Frame`] | one decoded picture, RGBA, ready to become a texture |
//!
//! # Where it sits
//!
//! This layer reads video through [`crate::analysis::video::SessionVideo`], which maps a moment of
//! session video time on a display to the segment file that has it, and through [`crate::av`], the
//! decoder of the host the viewer runs on (the host axis: macOS and Windows have one). The viewer
//! (`crate::viewer`) is the only caller. On a host with no decoder [`Playback::start`] still
//! succeeds and each display's view becomes [`DisplayView::Failed`] saying so.
//!
//! The flavor of komorebi that produced the session does not matter here: only the manifest's
//! displays and their video segments are read.

pub mod engine;
pub mod transport;

pub use engine::DisplayView;
pub use engine::Frame;
pub use engine::Playback;
pub use transport::Transport;
