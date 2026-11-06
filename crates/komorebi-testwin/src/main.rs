//! A window for the e2e scenarios to tile.
//!
//! It has a stable exe name, so that rules can match it, and a control channel on stdin so that a
//! scenario can change it while it runs. Every command is one line; every answer is one line of
//! JSON on stdout. The command line and the commands are the same on every platform (`cli.rs`);
//! the window is a Win32 one on Windows (`win32.rs`) and an AppKit one on macOS (`appkit.rs`).
//!
//! ```text
//! title TEXT        set the title
//! color RRGGBB      fill the client area with a colour
//! resize W H        resize the outer frame
//! fullscreen on|off take the window over its whole display, or bring it back
//! report            print the title and the visible frame
//! close             close the window, as the close button would
//! exit              leave without closing the window first
//! ```
//!
//! Options: `--title T`, `--class C` (the window class, `KomorebiTestWin` by default; macOS has
//! none and ignores it), `--color RRGGBB`, `--size WxH`, `--min-size WxH`, `--fixed` (not
//! resizable) and `--rename-to T` with `--after SECONDS` (the window gives itself that title after
//! that long, as a program that sets it late does).
//!
//! On Windows the message `WM_APP + 2` also takes the window fullscreen (`wparam` 1: a borderless
//! window over its monitor, as a program's own key does) or brings it back (`wparam` 0).

mod cli;
#[cfg(windows)]
mod win32;

#[cfg(windows)]
fn main() -> ::windows::core::Result<()> {
    win32::run(cli::parse(std::env::args().skip(1)))
}

#[cfg(not(windows))]
fn main() {
    eprintln!("komorebi-testwin opens a window on Windows only");
    std::process::exit(1);
}
