//! A host with no video decoder: opening a file says so.
//!
//! This module is built on every host that is neither macOS nor Windows, so that the investigation
//! commands (which only read a session directory) build and run there. [`Asset`] and [`Reader`]
//! have the same methods as the real ones and cannot be constructed: [`Asset::open`] always fails,
//! so nothing else here can ever be called. [`crate::av::supported`] is `false` on such a host, and
//! what needs video checks it first. Everything else in a session can still be read.

use super::Frame;
use super::Scale;
use color_eyre::eyre;
use std::path::Path;

fn unsupported() -> eyre::Report {
    eyre::Report::msg(format!(
        "decoding the recorded video is not implemented on {} yet; everything else in the \
         session can still be read",
        std::env::consts::OS
    ))
}

/// An opened video file. There is no way to hold one on this host.
///
/// The type has a field that cannot exist, so no value of it can: every method that takes `&self`
/// is unreachable.
pub struct Asset {
    _never: std::convert::Infallible,
}

impl Asset {
    /// Always fails: this host has no video decoder.
    ///
    /// # Errors
    ///
    /// Always returns an error, whatever the path. The message explains that decoding the recorded
    /// video is not implemented on this OS and that everything else in the session can still be
    /// read.
    pub fn open(_path: &Path) -> eyre::Result<Self> {
        Err(unsupported())
    }

    /// Returns the size of the video. Unreachable: there is no [`Asset`] on this host.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        match self._never {}
    }

    /// Returns the duration of the video. Unreachable: there is no [`Asset`] on this host.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        match self._never {}
    }

    /// Begins a decode. Unreachable: there is no [`Asset`] on this host.
    ///
    /// # Errors
    ///
    /// Never returns: there is no [`Asset`] to call it on.
    pub fn start(&self, _from_s: f64, _to_s: Option<f64>, _scale: Scale) -> eyre::Result<Reader> {
        match self._never {}
    }
}

/// A started decode. There is no way to hold one on this host.
pub struct Reader {
    _never: std::convert::Infallible,
}

impl Reader {
    /// Returns the next frame. Unreachable: there is no [`Reader`] on this host.
    ///
    /// # Errors
    ///
    /// Never returns: there is no [`Reader`] to call it on.
    pub fn next_frame(&mut self) -> eyre::Result<Option<Frame>> {
        match self._never {}
    }
}
