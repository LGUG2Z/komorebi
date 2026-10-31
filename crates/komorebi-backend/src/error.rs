use crate::backend::Capability;
use crate::id::WindowId;
use std::fmt;

/// What a [`crate::Backend`] call can fail with.
pub type Result<T> = std::result::Result<T, Error>;

/// Why a backend call failed.
///
/// Every platform has its own errors (`eyre` reports on KDE, Win32 `HRESULT`s, macOS
/// `AccessibilityError`); this is what the shared code can act on. In particular it can tell
/// "the window is gone", which macOS uses as its only signal that a window has died (a failed
/// write asks the reaper whether the window still exists, `docs/dev/seam.md` finding 9), from
/// every other failure.
///
/// # Example
///
/// ```rust
/// use komorebi_backend::{Error, WindowId};
///
/// let error = Error::WindowGone(WindowId::from_u32(9));
/// assert!(error.is_window_gone());
/// assert_eq!(error.to_string(), "no such window: 9");
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The window is not known to the backend, or has been destroyed. Every window command may
    /// fail with this, and (see [`crate::Backend::set_rect`]) with nothing else that the caller
    /// should treat as a property of the request.
    WindowGone(WindowId),
    /// The platform does not have this operation: the [`Capability`] is not
    /// [supported](crate::Backend::supports).
    Unsupported(Capability),
    /// The platform refused or failed, with its own error as the cause.
    Platform(Box<dyn std::error::Error + Send + Sync>),
}

impl Error {
    /// Returns true if and only if this is [`Error::WindowGone`].
    pub fn is_window_gone(&self) -> bool {
        matches!(self, Self::WindowGone(_))
    }

    /// Returns true if and only if this is [`Error::Unsupported`].
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported(_))
    }

    /// A [`Error::Platform`] failure that is only a message.
    pub fn platform(message: impl Into<String>) -> Self {
        Self::Platform(message.into().into())
    }

    /// A [`Error::Platform`] failure with the platform's own error as the cause.
    pub fn from_platform(cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Platform(Box::new(cause))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowGone(id) => write!(f, "no such window: {id}"),
            Self::Unsupported(capability) => {
                write!(f, "{capability:?} is not supported on this platform")
            }
            Self::Platform(cause) => cause.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(cause) => Some(cause.as_ref()),
            _ => None,
        }
    }
}
