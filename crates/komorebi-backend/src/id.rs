use std::fmt;

/// The name a backend gives a window, which is how every other part of komorebi refers to it.
///
/// It is an opaque 64-bit number. What it is made from is the backend's own business, and the
/// conversions below are the only ones to use:
///
/// - **KDE**: a counter the daemon assigns to each KWin window UUID (a `u32`):
///   [`WindowId::from_u32`], [`WindowId::to_u32`].
/// - **macOS**: the `CGWindowID` (a `u32`), same conversions.
/// - **Windows**: the `HWND`, a pointer-sized integer (`isize`). [`WindowId::from_hwnd`] keeps
///   the bits of the handle unchanged, so it never loses one and [`WindowId::to_hwnd`] gives the
///   same `isize` back, whether or not a handle fits in 32 bits (the e2e samples in
///   `docs/dev/seam.md` section 4.1 stayed below 2^32, but that is an observation, not a
///   guarantee).
///
/// # Example
///
/// ```rust
/// use komorebi_backend::WindowId;
///
/// let id = WindowId::from_u32(7);
/// assert_eq!(id.to_u32(), Some(7));
/// assert_eq!(id.to_string(), "7");
///
/// // a handle with the top bit set, as a 64-bit HWND may have
/// let hwnd = -123_isize;
/// assert_eq!(WindowId::from_hwnd(hwnd).to_hwnd(), hwnd);
/// assert_eq!(WindowId::from_hwnd(hwnd).to_u32(), None);
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WindowId(pub u64);

impl WindowId {
    /// The id of a KDE or macOS window.
    pub const fn from_u32(id: u32) -> Self {
        Self(id as u64)
    }

    /// The id as a `u32`, or `None` if it does not fit (which no KDE or macOS id can be).
    pub fn to_u32(self) -> Option<u32> {
        u32::try_from(self.0).ok()
    }

    /// The id of a Windows window: the bits of the `HWND`, unchanged.
    pub const fn from_hwnd(hwnd: isize) -> Self {
        Self(hwnd as usize as u64)
    }

    /// The `HWND` this id was made from by [`WindowId::from_hwnd`].
    pub const fn to_hwnd(self) -> isize {
        self.0 as usize as isize
    }
}

impl From<u32> for WindowId {
    fn from(id: u32) -> Self {
        Self::from_u32(id)
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The name a backend gives a monitor.
///
/// An opaque 64-bit number, like [`WindowId`]: KDE's output id and macOS's `CGDirectDisplayID`
/// are `u32` ([`MonitorId::from_u32`]), a Windows monitor handle (`HMONITOR`, an `isize`) goes
/// through [`MonitorId::from_hmonitor`] and back through [`MonitorId::to_hmonitor`], bit for bit.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MonitorId(pub u64);

impl MonitorId {
    /// The id of a KDE or macOS monitor.
    pub const fn from_u32(id: u32) -> Self {
        Self(id as u64)
    }

    /// The id as a `u32`, or `None` if it does not fit.
    pub fn to_u32(self) -> Option<u32> {
        u32::try_from(self.0).ok()
    }

    /// The id of a Windows monitor: the bits of the handle, unchanged.
    pub const fn from_hmonitor(handle: isize) -> Self {
        Self(handle as usize as u64)
    }

    /// The handle this id was made from by [`MonitorId::from_hmonitor`].
    pub const fn to_hmonitor(self) -> isize {
        self.0 as usize as isize
    }
}

impl From<u32> for MonitorId {
    fn from(id: u32) -> Self {
        Self::from_u32(id)
    }
}

impl fmt::Display for MonitorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hwnd_survives_the_round_trip_with_any_bits() {
        for hwnd in [0, 1, 0x1_0000_0000, isize::MAX, isize::MIN, -1] {
            assert_eq!(WindowId::from_hwnd(hwnd).to_hwnd(), hwnd);
            assert_eq!(MonitorId::from_hmonitor(hwnd).to_hmonitor(), hwnd);
        }
    }

    #[test]
    fn a_u32_id_round_trips_and_a_wide_one_does_not_fit() {
        assert_eq!(WindowId::from_u32(u32::MAX).to_u32(), Some(u32::MAX));
        assert_eq!(WindowId(u64::from(u32::MAX) + 1).to_u32(), None);
        assert_eq!(MonitorId::from(3).to_u32(), Some(3));
    }
}
