//! A record of the windows komorebi has cloaked, kept on disk.
//!
//! A daemon that is killed does not get to uncloak the windows it hid, and a window that is
//! cloaked looks the same to the next daemon as one on another virtual desktop or a suspended
//! app, so it cannot be told apart by looking at it. The next daemon uncloaks the windows that
//! the last one wrote down.

use crate::DATA_DIR;
use crate::WindowsApi;
use crate::com::SetCloak;

use std::path::PathBuf;
use windows::Win32::Foundation::HWND;

fn record_path() -> PathBuf {
    DATA_DIR.join("komorebi.cloaked.json")
}

/// Write down the windows that are cloaked
///
/// Unit tests hide windows that are not real, and must not touch the record of a daemon that is
/// running
pub fn record(hidden: &[isize]) {
    if cfg!(test) {
        return;
    }

    let path = record_path();
    let result = if hidden.is_empty() {
        match std::fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    } else {
        serde_json::to_string(hidden)
            .map_err(std::io::Error::other)
            .and_then(|json| std::fs::write(&path, json))
    };

    if let Err(error) = result {
        tracing::error!("could not record the cloaked windows: {error}");
    }
}

fn parse(contents: &str) -> Vec<isize> {
    serde_json::from_str(contents).unwrap_or_default()
}

/// Of the recorded windows, the ones that are still there and are still cloaked
fn still_cloaked(recorded: Vec<isize>, is_cloaked: impl Fn(isize) -> bool) -> Vec<isize> {
    recorded
        .into_iter()
        .filter(|hwnd| is_cloaked(*hwnd))
        .collect()
}

/// Uncloak the windows that a daemon that did not exit cleanly left cloaked, before the windows
/// are looked for
pub fn uncloak_left_behind() {
    let path = record_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };

    let left_behind = still_cloaked(parse(&contents), |hwnd| {
        WindowsApi::is_window(hwnd) && WindowsApi::is_window_cloaked(hwnd).unwrap_or_default()
    });

    for hwnd in left_behind {
        tracing::info!("uncloaking {hwnd}, which a previous run left cloaked");
        SetCloak(HWND(hwnd as *mut core::ffi::c_void), 1, 0);
    }

    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_that_cannot_be_read_names_no_windows() {
        assert!(parse("").is_empty());
        assert!(parse("{not json").is_empty());
        assert_eq!(parse("[12, 34]"), vec![12, 34]);
    }

    #[test]
    fn only_a_window_that_is_still_cloaked_is_left_behind() {
        let left = still_cloaked(vec![1, 2, 3], |hwnd| hwnd != 2);
        assert_eq!(left, vec![1, 3]);
    }
}
