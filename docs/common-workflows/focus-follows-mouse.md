# Focus Follows Mouse

`komorebi` supports two focus-follows-mouse implementations; the native Windows
Xmouse implementation, which treats the desktop, the task bar, and the system
tray as windows and switches focus to them eagerly, and a custom `komorebi`
implementation, which only considers windows managed by `komorebi` as valid
targets to switch focus to when moving the mouse.

To enable the `komorebi` implementation you must start the process with the
`--ffm` flag to explicitly enable the feature. This is because the mouse
tracking required for this feature significantly increases the CPU usage of the
process (on my machine, it jumps from <1% to ~4~), and this CPU increase
persists regardless of whether focus-follows-mouse is enabled or disabled at
any given time via `komorebic`'s configuration commands.

If the `komorebi` process has been started with the `--ffm` flag, you can
enable focus follows mouse behaviour in the `komorebi.json` configuration file.

```json
{
  "focus_follows_mouse": "Komorebi"
}
```

When calling any of the `komorebic` commands related to focus-follows-mouse
functionality, the `windows` implementation will be chosen as the default
implementation. You can optionally specify the `komorebi` implementation by
passing it as an argument to the `--implementation` flag:

```powershell
komorebic.exe toggle-focus-follows-mouse --implementation komorebi
```


