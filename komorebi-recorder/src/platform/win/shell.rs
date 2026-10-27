//! Showing a folder, asking for one, and naming the OS.
//!
//! The recorder window offers to show a finished session in the file manager, the viewer asks for
//! a session folder, and the manifest records the OS version. On Windows these are Explorer, the
//! common file dialog and `RtlGetVersion`; the same items exist on macOS.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;
use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;
use windows::Win32::System::Com::CoCreateInstance;
use windows::Win32::System::Com::CoInitializeEx;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::CoUninitialize;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::UI::Shell::FOS_FORCEFILESYSTEM;
use windows::Win32::UI::Shell::FOS_PICKFOLDERS;
use windows::Win32::UI::Shell::FileOpenDialog;
use windows::Win32::UI::Shell::IFileOpenDialog;
use windows::Win32::UI::Shell::SIGDN_FILESYSPATH;
use windows::core::PCWSTR;

/// What the OS calls its file manager, for button labels: `Explorer`.
pub const FILE_MANAGER: &str = "Explorer";

/// Returns the Windows version as `major.minor.build`, e.g. `10.0.26100`, or `None` if it cannot be
/// read. On macOS it is the product version, e.g. `26.0`.
///
/// This is `RtlGetVersion`, because the documented `GetVersionEx` lies to a program without a
/// manifest saying which Windows it was made for.
#[must_use]
pub fn os_version() -> Option<String> {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a valid, correctly sized out structure
    let status = unsafe { RtlGetVersion(&raw mut info) };
    status.is_ok().then(|| {
        format!(
            "{}.{}.{}",
            info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
        )
    })
}

/// Shows `path` in the file manager: a folder is opened, a file is selected in its folder.
///
/// This starts `explorer` and does not wait for it; Explorer exits with status 1 even when it
/// worked, so only starting it is checked, and a failure to start it is not reported. On macOS a
/// file is opened in its default application instead of being selected.
pub fn reveal(path: &Path) {
    let mut command = Command::new("explorer");
    if path.is_file() {
        let mut select = std::ffi::OsString::from("/select,");
        select.push(path);
        command.arg(select);
    } else {
        command.arg(path);
    }

    // explorer exits with 1 even when it worked; only starting it matters
    let _ = command.spawn();
}

/// Asks the user for a folder with the system's own dialog, and blocks until they answer.
///
/// Returns the folder they chose, or `None` if they canceled, the dialog could not be shown, or
/// the path came back empty. `prompt` is the dialog's title. The dialog is made on the calling
/// thread, which is given a COM apartment for the duration unless it already has one.
#[must_use]
pub fn pick_folder(prompt: &str) -> Option<PathBuf> {
    // SAFETY: COM is initialised for this thread for the duration of the dialog and released after
    // (unless some other code had initialised it differently, in which case it is theirs to release)
    unsafe {
        let initialised = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let chosen = pick(prompt);
        if initialised {
            CoUninitialize();
        }
        chosen
    }
}

/// # Safety
/// COM must be initialized on the calling thread.
unsafe fn pick(prompt: &str) -> Option<PathBuf> {
    // SAFETY: the caller initialised COM; every pointer below is owned by a COM wrapper or freed
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog
            .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
            .ok()?;

        let title: Vec<u16> = prompt.encode_utf16().chain(Some(0)).collect();
        dialog.SetTitle(PCWSTR(title.as_ptr())).ok()?;

        // cancelling is an error as far as COM is concerned
        dialog.Show(None).ok()?;

        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0.cast_const().cast()));
        path.filter(|path| !path.is_empty()).map(PathBuf::from)
    }
}
