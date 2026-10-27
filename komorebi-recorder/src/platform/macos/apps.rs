//! Which process owns a window id, from the window server.
//!
//! [`window_owners`] calls `CGWindowListCopyWindowInfo(OptionAll, ...)`. `OptionAll` matters:
//! komorebi's own wrapper only lists on-screen windows, which would miss every window on another
//! space.
//!
//! The same function exists on Windows, where it enumerates top-level windows and a window id is a
//! window handle; see [`window_owners`] for how they differ.

use crate::sources::managed_apps::WindowOwner;
use objc2_core_foundation::CFArray;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFNumber;
use objc2_core_foundation::CFString;
use objc2_core_graphics::CGWindowListCopyWindowInfo;
use objc2_core_graphics::CGWindowListOption;
use objc2_core_graphics::kCGNullWindowID;
use objc2_core_graphics::kCGWindowLayer;
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
