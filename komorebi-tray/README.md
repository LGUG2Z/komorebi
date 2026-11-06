# Komorebi Tray

A Windows tray indicator for the selected workspace on Komorebi's focused monitor.
The first workspace displays **1**. When focus moves between monitors, the indicator
follows it. Hover over the number to see the workspace name and monitor name.

## Build and run

From the repository root on Windows with the repository's Rust toolchain:

```powershell
cargo build -p komorebi-tray --release --locked
Start-Process .\target\release\komorebi-tray.exe -WindowStyle Hidden
```

You can copy the release .exe to another directory and double-click it there.
No additional files are needed; the app opens only a tray icon.

Or install the binary into Cargo's bin directory:

```powershell
cargo install --path komorebi-tray --locked
komorebi-tray.exe
```

The existing `just build`, `just install`, and `just copy` recipes include this app.
It runs without a console window or configuration file. Launching it a second
time in the same Windows session leaves the existing instance running.

With a version of `komorebic` that supports `--tray`, you can manage both together:

```powershell
komorebic start --tray
komorebic stop --tray
komorebic kill --tray
komorebic enable-autostart --tray
```

The flag is independent of `--bar` and can be combined with `--whkd` or `--masir`.
`start --tray` also launches the indicator if Komorebi is already running. It looks
for `komorebi-tray.exe` beside `komorebic.exe`, then on `PATH`. The Windows MSI
installer includes the tray executable.

`stop --tray` lets the indicator remove its icon and clean up before exiting, with
a force-stop fallback after five seconds. `kill --tray` terminates it immediately.
`stop --tray` also stops Komorebi. The `kill` command targets only the selected
helper processes, so `kill --tray` leaves Komorebi running. Use the tray's **Exit**
menu to close only the indicator gracefully. Without `--tray`, stopping Komorebi
leaves the indicator running in its disconnected state.

`enable-autostart --tray` saves the flag in the shared Komorebi startup shortcut.
`komorebic disable-autostart` removes that shortcut. Starting the tray executable
directly does not change autostart or launch Komorebi.

Windows may initially put the icon in the tray overflow menu. Drag it into the
visible notification area, or enable it in Windows' taskbar tray settings.

Right-click the icon and select **Exit** to close the indicator. It does not change
workspaces, monitor focus, or Komorebi configuration.

## Connection and display

- Workspace numbers update from the same state notifications used by `komorebi-bar`.
- A square crossed with an X replaces the number while Komorebi is paused. The
  tooltip says **Paused** and retains available workspace and monitor details.
- `—` means there is no current workspace to display. The tooltip distinguishes
  a disconnected daemon from an unavailable focused workspace.
- If Komorebi is unavailable at launch or stops, the app stays running and retries
  every second. It queries state on startup and after five seconds without
  notifications, then refreshes its subscription when active. While paused, it
  queries state every second because Komorebi ignores new subscriptions until
  resumed. This also supports launching the indicator while already paused.
- The icon is restored after Explorer restarts and rendered for the taskbar's DPI.
- Workspace names do not replace numeric labels. Long tooltip text is truncated
  at the Windows tooltip limit without breaking Unicode characters.

## Development checks

```powershell
cargo +nightly fmt -p komorebi-tray --check
cargo clippy -p komorebi-tray --all-targets -- -D warnings
cargo test -p komorebi-tray
```

Manual acceptance checks: pause/resume without changing workspace, launch while
paused, stay paused for more than five seconds, change workspace and monitor
focus, start the indicator
before Komorebi, restart Komorebi, recreate the taskbar, open/dismiss the Exit menu,
and verify the icon at 100%, 150%, and 200% scaling on light and dark taskbars.
