//! Which process owns a window id: `GetWindowThreadProcessId` on its handle.
//!
//! komorebi for Windows identifies a window by its handle, so unlike on macOS the window list is
//! only needed to find out who owns each handle and what that process is called. [`window_owners`]
//! is the same function that macOS provides.

use crate::sources::managed_apps::WindowOwner;
use std::collections::HashMap;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::System::Threading::OpenProcess;
use windows::Win32::System::Threading::PROCESS_NAME_WIN32;
use windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;
use windows::Win32::System::Threading::QueryFullProcessImageNameW;
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::core::BOOL;
use windows::core::PWSTR;

/// The file name of the program a process is running, without `.exe`.
pub(crate) fn process_name(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed again, and the buffer outlives the call that fills it
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0_u16; 1024];
        let mut length = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        );
        let _ = CloseHandle(process);
        queried.ok()?;

        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        let file = path.rsplit(['\\', '/']).next()?;
        Some(
            file.strip_suffix(".exe")
                .or_else(|| file.strip_suffix(".EXE"))
                .unwrap_or(file)
                .to_string(),
        )
    }
}

unsafe extern "system" fn collect(window: HWND, found: LPARAM) -> BOOL {
    // SAFETY: `found` is the address of the vector `window_owners` passed to `EnumWindows`
    let found = unsafe { &mut *(found.0 as *mut Vec<(u32, u32)>) };
    let mut pid = 0_u32;
    // SAFETY: `window` is a handle `EnumWindows` just gave us
    let thread = unsafe { GetWindowThreadProcessId(window, Some(&raw mut pid)) };
    if thread != 0 && pid != 0 {
        // a window handle is a 32-bit value even in a 64-bit process
        found.push((window.0 as usize as u32, pid));
    }

    BOOL(1)
}

/// Returns every top-level window that `EnumWindows` reports, keyed by its handle, with the process
/// that owns each.
///
/// The key is the window handle (`HWND`) as a `u32`: a handle holds a 32-bit value even in a
/// 64-bit process, and it is the number komorebi for Windows calls a window's `hwnd`. Each
/// [`WindowOwner`] has the owning process id (`0` if it does not fit in an `i32`), the program's
/// file name without `.exe` (empty if the process cannot be opened for a query) and the layer,
/// which is always `0`. A window whose owner cannot be found is left out. The program's name is
/// looked up once for each process.
///
/// # Host differences
///
/// On macOS the keys are the window server's window numbers, the name is the owner name the window
/// server gives, the layer is the window's real layer, and windows on other spaces are included.
///
/// This asks the machine it runs on, so it has no doctest; nothing tests it automatically.
#[must_use]
pub fn window_owners() -> HashMap<u32, WindowOwner> {
    let mut found: Vec<(u32, u32)> = Vec::new();
    // SAFETY: `found` outlives the call and `collect` only touches it during it
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&raw mut found as isize)) };

    let mut names: HashMap<u32, String> = HashMap::new();
    found
        .into_iter()
        .map(|(handle, pid)| {
            let name = names
                .entry(pid)
                .or_insert_with(|| process_name(pid).unwrap_or_default())
                .clone();
            (
                handle,
                WindowOwner {
                    pid: i32::try_from(pid).unwrap_or(0),
                    name,
                    layer: 0,
                },
            )
        })
        .collect()
}

/// Returns the top-level windows a user could see, front to back (see
/// [`crate::platform::window::list`], which is how the rest of the crate reaches this).
#[must_use]
pub fn window_list() -> Vec<crate::sources::windowtrack::WindowSample> {
    super::window::list()
}
