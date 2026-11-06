#![windows_subsystem = "windows"]
#![warn(clippy::all)]

mod icon;
mod state;
mod subscription;
mod tray;

use windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR;
use windows::Win32::UI::WindowsAndMessaging::MB_OK;
use windows::Win32::UI::WindowsAndMessaging::MessageBoxW;
use windows::core::PCWSTR;
use windows::core::w;

fn main() {
    if let Err(error) = tray::run() {
        let message: Vec<u16> = format!("Komorebi Tray could not start:\n{error:#}\0")
            .encode_utf16()
            .collect();
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(message.as_ptr()),
                w!("Komorebi Tray"),
                MB_OK | MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}
