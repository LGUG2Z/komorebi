# ETW spike: what an ETW tap would tell us about window management

Date 2026-10-28, Windows 11 26200, **not elevated** shell (user is in Performance Log Users). No recorder
code was written. Traces and scripts live in the session scratchpad, not the repo.

## Method

`logman create trace <n> -p <guid> 0xffffffffffffffff 0xff -o <n>.etl -ets` for Win32k, User32, Dwm-Core
and Shell-Core (one file session each, plus a real-time `-rt` Win32k session). Then a throwaway
WinForms window owned by a spawned `pwsh` (hwnd 0x2A80880, pid 10404) was driven with `SetWindowPos`
(move, resize, TOPMOST, NOTOPMOST), `ShowWindow` (minimize, restore, hide, show),
`DwmSetWindowAttribute(DWMWA_CLOAK)` and `SetForegroundWindow` over about 6 s. Sessions stopped, decoded
with `tracerpt <etl> -of CSV`. Nothing outside that window was touched.

## Elevation

- **Every session started without elevation**: Win32k, User32, Dwm-Core, Shell-Core, real-time and file.
  Nothing was refused; membership of Performance Log Users is enough for user-mode provider sessions.
  (`logman query -ets` listed all five as Running.) We did not try the kernel logger (NT Kernel Logger,
  `SystemTraceProvider`); that one does need admin and is out of scope.
- `Microsoft-Windows-User32` {B0AA8734...} started fine but emitted **nothing** (2 header events only; it
  has no registered manifest on this machine: `wevtutil gp` cannot find it). It is not a window-API trace.
- If a future machine does refuse, the command to try elevated is the same one:
  `logman create trace x -p "{8C416C79-D49B-4F01-A467-E56D3AA8234C}" 0xffffffffffffffff 0xff -o x.etl -ets`.

## What the events contain

| Provider | Events in ~22 s | Notes |
|---|---|---|
| Win32k | 102,911 delivered, **817,109 lost** (default buffers) | about 4.7k/s kept, about 42k/s offered, while the desktop was idle |
| Dwm-Core | 216,834, 0 lost | about 10k/s |
| Shell-Core | 48,123, 0 lost | about 2k/s |
| User32 | 0 | nothing |

- **Win32k** is a graphics and input trace (CreateHandle/DestroyHandle/TransformHandle, FlipManager,
  touch targeting, pseudo-device, "Info" ids 164/167/301/65...). 79 events carried our pid in the header
  during the run, spread over ids 164, 166, 167, 45, 84-87, 452, 453, 2000, and none of them was a
  window-management call. The hwnd 0x2A80880 appears in **no** Win32k payload; neither did our rect
  (500x400, 600x450) or SWP flags. Payloads are untyped numbers to `tracerpt` (the manifest is in
  win32kbase.sys, see below), so even a mapping would be reverse engineering.
- **Dwm-Core** carried our hwnd in 36 events, only in two kinds, both from `dwm.exe`:
  - id 84, task `WINDOWNODE_GDISPRITE_ASSOCIATION`: hwnd, then handles. 26 hits.
  - id 209 `Stop` (task 137): hwnd, 0, 0, then a 4x4 identity transform matrix. 10 hits. No rect, no
    flags, no cloak bit, no z-order, no show/hide. The 10 hits for roughly 11 API calls cannot be tied to
    a call without the timestamp alignment.
- **Shell-Core**: no hwnd or pid match at all for our window.
- **Who called?** Header PID/TID of every Dwm-Core event is `dwm.exe` (0x834) and the thread inside it
  (0xA50/0xBFC). That is the *consumer* of the change, never the process that called `SetWindowPos`. For
  Win32k the header pid is the process that ran the kernel path (ours, 10404, for our own work), which is
  the closest to a caller found, but it showed only graphics events, not "this process moved window X".

Sample (Dwm id 209, trimmed):
`Microsoft-Windows-Dwm-Core, Stop, 209, ..., pid 0x834, tid 0xBFC, ..., 0x2A80880, 0x0, 0, 1.0 0 0 0 | 0 1.0 0 0 | 0 0 1.0 0 | 0 0 0 1.0`

## What WinEvents already give

The existing hook (`EVENT_MIN..EVENT_MAX`, `SetWinEventHook`) yields, per top-level window: hwnd plus
class and program (looked up), `EVENT_OBJECT_LOCATIONCHANGE`, `OBJECT_CLOAKED`/`UNCLOAKED`,
`OBJECT_SHOW`/`HIDE`, `SYSTEM_FOREGROUND`, `SYSTEM_MINIMIZESTART/END`, move/size start/end,
`OBJECT_REORDER`, with the event thread/time. Compared with ETW: WinEvents are *per window* and
semantic, ETW is *per frame* and about DWM internals. ETW added no hwnd-level fact the hook lacks, and
neither source yields the rect or flags (the hook consumer can call `GetWindowRect` at record time, which
is already the cheaper way).

## Cost of a tap

- Behind `platform::trace`, as a third tap beside the hook and Event Log. Crate options: `ferrisetw`
  (real-time `UserTrace` and a callback, schema decode through TDH) or the `windows` crate
  `StartTraceW/EnableTraceEx2/OpenTraceW/ProcessTrace`. About 1.5 to 2 days including dropping
  events on a hot path and a session that must be stopped at crash (a leaked session survives the
  process; use a unique name and `ControlTrace(STOP)` on startup).
- It fits the oslog record shape (`ts, source, process, message`), but only by flattening each event to
  `provider:id task` plus untyped numbers, which is not useful to read.
- Noise is the problem: 10k to 40k events/s from Win32k and Dwm-Core with nothing happening, almost all
  of it frame pacing. It would need a per-id allowlist and still shows nothing we want.

## Recommendation: **don't build it**

None of the four goals (hwnd, rect, flags, caller) come out of the providers that are available without
elevation. What would give them is not an ETW provider but a komorebi-side change:

1. Have komorebi log, at the place it applies a layout (its `WindowManager::update_*` and `Window::set_position`,
   `hide`, `cloak` paths), the hwnd, target rect, SWP flags, and `reason` (which event or command caused
   it) into its own state/notification stream; the recorder already ingests that stream.
2. If the *foreign* caller is wanted (an app fighting the tiling), the practical method is a `WH_CBT`
   or `WH_CALLWNDPROC` hook with `WM_WINDOWPOSCHANGING`, which carries the `WINDOWPOS` struct (hwnd,
   rect, flags) and runs in the caller's context. That needs a DLL injected and is a much bigger step
   than a trace tap; the cheaper step is to take a `GetWindowRect` snapshot on each ObjectLocationChange
   in the recorder (a few extra lines in `win/trace.rs`) and record the flags komorebi sets itself.
3. Revisit ETW only if a Microsoft-Windows-Win32k manifest with window-position events turns up in a
   later Windows build, or if we accept the kernel logger (admin) for a one-off diagnostic.