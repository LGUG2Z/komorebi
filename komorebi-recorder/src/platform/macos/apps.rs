//! Which process owns a window id, from the window server.
//!
//! [`window_owners`] calls `CGWindowListCopyWindowInfo(OptionAll, ...)`. `OptionAll` matters:
//! komorebi's own wrapper only lists on-screen windows, which would miss every window on another
//! space.
//!
//! The same function exists on Windows, where it enumerates top-level windows and a window id is a
//! window handle; see [`window_owners`] for how they differ.

use crate::sources::managed_apps::WindowOwner;
use crate::sources::windowtrack::WindowSample;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFBoolean;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFNumber;
use objc2_core_foundation::CFString;
use objc2_core_graphics::CGWindowListCopyWindowInfo;
use objc2_core_graphics::CGWindowListOption;
use objc2_core_graphics::kCGNullWindowID;
use objc2_core_graphics::kCGWindowBounds;
use objc2_core_graphics::kCGWindowIsOnscreen;
use objc2_core_graphics::kCGWindowLayer;
use objc2_core_graphics::kCGWindowName;
use objc2_core_graphics::kCGWindowNumber;
use objc2_core_graphics::kCGWindowOwnerName;
use objc2_core_graphics::kCGWindowOwnerPID;
use std::collections::HashMap;
use std::ptr::NonNull;

fn dictionary_value<T>(dictionary: &CFDictionary, key: &CFString) -> Option<NonNull<T>> {
    // SAFETY: `key` is a valid CFString and the dictionary is live
    let value = unsafe { CFDictionary::value(dictionary, NonNull::from(key).as_ptr().cast()) };
    NonNull::new(value.cast_mut()).map(NonNull::cast)
}

/// Returns every window the window server knows about (all spaces, on-screen or not), keyed by
/// window id, with the process that owns each.
///
/// The key is the window server's window number, which is what komorebi for Mac calls a window's
/// `id`. Each [`WindowOwner`] has the owning process id (`kCGWindowOwnerPID`), the owner's name
/// (`kCGWindowOwnerName`, empty if the window server gave none) and the window's layer
/// (`kCGWindowLayer`, `0` for an ordinary application window). A window with no number or no owning
/// process id is left out. The map is empty if the window server cannot be asked, as in a sandboxed
/// build with no window server.
///
/// # Host differences
///
/// On Windows the keys are window handles (`HWND` values), the name is the owning program's file
/// name without `.exe`, and the layer is always `0`. On a host that can only investigate a session
/// the map is always empty.
///
/// This asks the machine it runs on, so it has no doctest. It is covered by
/// `window_server_listing_works` in the unit tests of `crate::sources::managed_apps`.
#[must_use]
pub fn window_owners() -> HashMap<u32, WindowOwner> {
    let mut owners = HashMap::new();
    let Some(list) = CGWindowListCopyWindowInfo(CGWindowListOption::OptionAll, kCGNullWindowID)
    else {
        return owners;
    };

    let list: &CFArray = &list;
    for index in 0..list.count() {
        // SAFETY: every element of the window list is a CFDictionary, and `index` is in bounds
        let Some(dictionary) =
            (unsafe { list.value_at_index(index).cast::<CFDictionary>().as_ref() })
        else {
            continue;
        };

        // SAFETY: values under these keys have these types per the CGWindow documentation
        let (number, pid, name, layer) = unsafe {
            (
                dictionary_value::<CFNumber>(dictionary, kCGWindowNumber)
                    .and_then(|number| number.as_ref().as_i64()),
                dictionary_value::<CFNumber>(dictionary, kCGWindowOwnerPID)
                    .and_then(|pid| pid.as_ref().as_i32()),
                dictionary_value::<CFString>(dictionary, kCGWindowOwnerName)
                    .map(|name| name.as_ref().to_string()),
                dictionary_value::<CFNumber>(dictionary, kCGWindowLayer)
                    .and_then(|layer| layer.as_ref().as_i64()),
            )
        };

        if let (Some(number), Some(pid)) = (number, pid)
            && let Ok(number) = u32::try_from(number)
        {
            owners.insert(
                number,
                WindowOwner {
                    pid,
                    name: name.unwrap_or_default(),
                    layer: layer.unwrap_or_default(),
                },
            );
        }
    }

    owners
}

/// Reads one number out of a `kCGWindowBounds` dictionary (`X`, `Y`, `Width` or `Height`).
fn bounds_value(bounds: &CFDictionary, key: &str) -> Option<f64> {
    let key = CFString::from_str(key);
    // SAFETY: the values of a bounds dictionary are CFNumbers, per the CGWindow documentation
    unsafe { dictionary_value::<CFNumber>(bounds, &key).and_then(|n| n.as_ref().as_f64()) }
}

/// Returns the ordinary windows (layer 0, with a frame) the window server lists, front to back,
/// as [`WindowSample`]s, without this process's own windows. `rect` is `kCGWindowBounds` in
/// display points rounded to whole points, `visible` is `kCGWindowIsOnscreen`, and `cloaked` is
/// `None` because a Mac has no such thing. `border` is true for komorebi's own border windows, which
/// are ordinary-level windows owned by the `komorebi` process. This is how [`crate::platform::window::list`] is
/// answered here.
///
/// Not built or run when this was written: it was checked against the `objc2` sources only.
#[must_use]
pub fn window_list() -> Vec<WindowSample> {
    let mut windows = Vec::new();
    let Some(list) = CGWindowListCopyWindowInfo(CGWindowListOption::OptionAll, kCGNullWindowID)
    else {
        return windows;
    };

    let own = std::process::id() as i32;
    let list: &CFArray = &list;
    for index in 0..list.count() {
        // SAFETY: every element of the window list is a CFDictionary, and `index` is in bounds
        let Some(dictionary) =
            (unsafe { list.value_at_index(index).cast::<CFDictionary>().as_ref() })
        else {
            continue;
        };

        // SAFETY: values under these keys have these types per the CGWindow documentation
        let (number, pid, owner, title, layer, onscreen, bounds) = unsafe {
            (
                dictionary_value::<CFNumber>(dictionary, kCGWindowNumber)
                    .and_then(|number| number.as_ref().as_i64()),
                dictionary_value::<CFNumber>(dictionary, kCGWindowOwnerPID)
                    .and_then(|pid| pid.as_ref().as_i32()),
                dictionary_value::<CFString>(dictionary, kCGWindowOwnerName)
                    .map(|name| name.as_ref().to_string()),
                dictionary_value::<CFString>(dictionary, kCGWindowName)
                    .map(|name| name.as_ref().to_string()),
                dictionary_value::<CFNumber>(dictionary, kCGWindowLayer)
                    .and_then(|layer| layer.as_ref().as_i64()),
                dictionary_value::<CFBoolean>(dictionary, kCGWindowIsOnscreen)
                    .map(|onscreen| onscreen.as_ref().as_bool()),
                dictionary_value::<CFDictionary>(dictionary, kCGWindowBounds),
            )
        };

        let (Some(number), Some(pid), Some(bounds)) = (number, pid, bounds) else {
            continue;
        };
        if pid == own || layer.unwrap_or_default() != 0 {
            continue;
        }
        let owner = owner.unwrap_or_default();
        let Ok(id) = u32::try_from(number) else {
            continue;
        };

        // SAFETY: `bounds` came out of the live window dictionary above
        let bounds = unsafe { bounds.as_ref() };
        let (Some(x), Some(y), Some(width), Some(height)) = (
            bounds_value(bounds, "X"),
            bounds_value(bounds, "Y"),
            bounds_value(bounds, "Width"),
            bounds_value(bounds, "Height"),
        ) else {
            continue;
        };
        if width < 1.0 || height < 1.0 {
            continue;
        }

        let border = crate::sources::windowtrack::mac_border_owner(&owner);
        windows.push(WindowSample {
            id,
            owner,
            title: title.filter(|title| !title.is_empty()),
            rect: [
                x.round() as i32,
                y.round() as i32,
                (x + width).round() as i32,
                (y + height).round() as i32,
            ],
            visible: onscreen.unwrap_or(false),
            minimized: false,
            cloaked: None,
            border,
            border_of: None,
        });
    }

    windows
}
