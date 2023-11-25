#![windows_subsystem = "windows"]

use std::io;
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

fn main() -> io::Result<()> {
    let mut current_exe = std::env::current_exe().expect("unable to get exec path");
    current_exe.pop();
    let komorebic_exe = current_exe.join("komorebic.exe");

    Command::new(komorebic_exe)
        .args(std::env::args_os().skip(1))
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|_| ())
}
