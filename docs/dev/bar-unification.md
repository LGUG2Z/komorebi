# One `komorebi-bar` for Windows and macOS

Status at the revision that adds this file. Everything below was measured on this tree with
`diff` (counts are lines that differ, `<`/`>` lines of `diff`, hunks in parentheses). Nothing
here was rendered: the bars cannot run on Linux. Gates that did run are named; the ones that
did not are listed at the end.

KDE has no bar: `komorebi-for-kde` has no `komorebi-bar` package and no `schema.bar.json`.
The plan below is for two platforms; a KDE bar would add a third shell and a third schema view.

## What exists

- `crates/komorebi-bar-core` (2,630 lines): the code both bars share, one copy each.
  `selected_frame`, `ui`, `statics`, `alignment`, `render`, the leaf types of the config
  (`config/`), the widgets `battery`, `cpu`, `date`, `memory`, `update`, `storage`, `time`, the
  `BarWidget` trait, `platform` (`BarPlatform`, `SystemTool`, a once-installed global) and
  `schema` (per-platform views, see below).
- Two shells, `komorebi-bar` (Windows, root workspace, version 0.1.42) and
  `komorebi-for-mac/komorebi-bar` (own workspace, own `Cargo.lock`, own flake, version 0.1.0).
  Each keeps `main.rs`, `bar.rs`, `config.rs`, `os.rs` and the widgets that differ.

## Why one package is not possible yet (the structural blocker)

The two trees are two Cargo workspaces with two lockfiles and two flakes. A package belongs to
one workspace, so a single `komorebi-bar` that builds on both platforms needs one workspace
first: the Windows root `Cargo.toml` and `komorebi-for-mac/Cargo.toml` merged, one `Cargo.lock`,
one version (0.1.42 vs 0.1.0), one `komorebi-client` (the two `lib.rs` differ in 150 lines:
Windows re-exports the daemon crate and `State`/`Window`/`Workspace`, macOS has its own
types) and `windows`/`objc2` dependencies under `[target.'cfg(...)'.dependencies]`. That is a
repository-level cutover (the plan's "final naming of the outputs and the unified app/devShell
is a cutover decision"), not a bar change. Until then the bars share the crate and converge,
they do not merge. Everything in this stack is ordered so the merge later is a package move.

## The schema is solved: views of one union

`schema.bar.json` differs between the platforms in 40 hunks (5,381 lines of 9,091 / 3,774).
Analysed by definition (`jq` over `$defs`):

- 42 definitions exist only on Windows: all reachable only through `MouseConfig` (which holds a
  `SocketMessage`, hence the closure) and the `Keyboard`/`Media`/`Systray` widgets. Removing the
  `mouse` property and the three widget variants and pruning unreachable definitions removes
  all 42.
- 14 differences remain in the 55 shared definitions, all text or default: root description
  (macOS names its own schema URL), `height` default `50` vs `50.0`, the margin and padding
  examples (`10` vs `10.0`, a sentence about defaults, a trailing comma), `show_icon`
  (deprecated flag and backticks), `KomorebiLayout` (untagged `anyOf` on Windows, externally
  tagged `oneOf` on macOS), `NetworkConfig.default_interface_options` (Windows only),
  four storage descriptions (`C:\` vs `/`), `TimeConfig.changing_icon` (`default: false` only
  on macOS), `TimeFormat` title of `Custom` (macOS says `Default`, a mislabel), the
  `WidgetConfig` variants above.

`komorebi_bar_core::schema` holds this as data (`schema/changes.rs`, 16 changes) in the same
design as `komorebi-config`: the union is the Windows text, `view_of` applies a platform's
changes and prunes. `tests/schema_oracle.rs` proves that the macOS view of the checked-in Windows
file is byte-identical to `komorebi-for-mac/schema.bar.json`, that the Windows view is the
identity, and that views are idempotent. Both bars print `--schema` through their view
(`schema::generated_text`), so the files and `nix/komorebi-bar-options.nix` do not change when
code is converged onto the Windows spelling.

The out-of-tree harness (`S/bar2/regen.sh`, real bar sources over the real
`komorebi-protocol`/`komorebi-config`) regenerates both files on Linux; the in-repo oracle
cannot yet, because the config types are not in the crate.

## What blocks each remaining file (measured)

| file | win / mac lines | differing | what differs and why it is not a pure move |
|---|---|---|---|
| `widgets/komorebi_layout.rs` | 331 / 329 | 6 (4) | `#[serde(untagged)]` on `KomorebiLayout` only on Windows. That changes which layout spellings a macOS config accepts: BEHAVIOUR on macOS. Needs the user's decision (macOS file already documents the tagged shape). |
| `widgets/network.rs` | 656 / 612 | 94 (19) | Windows has `default_interface_options` and `InterfaceDisplayType` (config surface), `std::sync::Mutex` vs `parking_lot`, `netdev` 0.46 vs 0.40 with a different interface match key (friendly name vs `name`), `cmd.exe /C ncpa` vs `open x-apple...`. Converging is a functional change on macOS: BEHAVIOUR, needs a Mac. |
| `widgets/applications.rs` | 409 / 407 | 64 (5) | icon lookup through `windows-icons` and `which` (Windows) vs `None` (macOS, the Windows code is commented out). Needs `ImageIcon`/`ImageIconId` in the core (below) and a platform hook `command_icon`. |
| `widgets/mod.rs` | 183 / 171 | 24 (8) | `ImageIconId::Hwnd(isize)`/`SystrayIcon` vs `WindowId(u32)`, `remove`, module list. One enum with all variants works; mechanical, but touches `komorebi.rs`, `applications.rs`, `systray.rs` call sites. |
| `widgets/widget.rs` | 126 / 110 | 36 (12) | `WidgetConfig` has Keyboard/Media/Systray on Windows only (macOS has them commented out). D25/D26: one enum with all variants; macOS then accepts the keys and must skip them at load with a warning (BEHAVIOUR on macOS, and the data types `KeyboardConfig`, `MediaConfig`, `SystrayConfig` move to the core first). |
| `widgets/komorebi.rs` | 1,025 / 1,068 | 125 (19) | uses `komorebi_client::{State, Window, Workspace, Container, Monitor}` (Windows) vs the macOS client types; `Window::title()/hwnd/process_id` vs `details.title/icon_path`; macOS-only `icns_to_rgba`, `device`; frame/margin drawing in `show_label`/`show_icon`. Needs the `State` -> plain snapshot split (the shared `komorebi-backend::{WindowInfo, MonitorInfo}` are the natural carrier). |
| `config.rs` | 590 / 582 | 338 (19) | Of the 338, about 285 are the Windows mouse block (`MouseConfig`, `MouseMessage`, `KomorebiMouseMessage`, `execute` with `exec_powershell`, `send_batch`) vs the same block commented out on macOS; the rest is import order, doc text (handled by the view) and `eyre`. The block needs `SocketMessage` and `exec_powershell`; moving it to the core with the view removing it for macOS is possible once `exec_powershell` is a platform hook. Blocked on widget.rs (config.rs refers to `WidgetConfig`). |
| `bar.rs` | 1,398 / 1,104 | 444 (35) | Windows: PowerShell session (`exec_powershell`), `hwnd`, virtual-desktop restore/minimize, `position_bar` through `komorebi_client::Window`, CJK font fallbacks, `scale_factor`. macOS: has the notification type unboxed, shadows and window level. Not behaviour-preserving without a screen. |
| `main.rs` | 402 / 388 | 92 (14) | `process_hwnd`/`enum_window`, DPI awareness, `set_read_timeout`; macOS activation policy and window level (AppKit), config dir `~/.config/komorebi`. Window/viewport setup belongs behind `BarPlatform` hooks (`init_process`, `viewport_builder`, `after_creation`). |
| `os.rs` | 32 / 41 | 29 (8) | the platform implementations themselves; stays per platform (cfg(target_os) after the merge). |

Windows-only and not shared: `widgets/keyboard.rs` (181), `media.rs` (333), `systray.rs` (1,301);
the macOS copies of keyboard and media were dead and are deleted.

macOS commands that spawn programs that do not exist there are preserved, not fixed:
`SystemTool::PowerSettings` (battery widget) still spawns `cmd.exe /C start ms-settings:...` and
`SystemTool::Url` (update widget) still spawns `explorer.exe`; Task Manager and Network settings
use `open`. `update.rs` still has a second GitHub API URL for the Windows repository.
These live in `komorebi-for-mac/komorebi-bar/src/os.rs`. The storage click used to be inline;
it is now `SystemTool::StorageLocation` and each shell spawns what it spawned before.

## The plan, in order

1. Done in this stack: the schema views and the oracle (`schema/`), `--schema` through the
   views, storage and time converged and moved (no schema change).
2. `ImageIcon`/`ImageIconId` and `widgets/mod.rs` helpers into the core with one enum; then
   `applications.rs` with a `command_icon` hook. Mechanical; no schema effect.
3. Data types of Keyboard/Media/Systray/mouse into the core; `WidgetConfig` with every variant;
   `config.rs` into the core; the macOS view already removes them from the schema. Decision
   needed: macOS accepts these keys and skips them with a warning (D26) or rejects them.
4. `BarPlatform` hooks for the process/window setup of `main.rs`/`bar.rs`; `Komobar<P>` and the
   shared `app` body into the core; shells reduce to `platform.rs` + `main.rs`.
5. `komorebi.rs` after the snapshot split (depends on step 8/11 of the main plan).
6. Workspace merge (above), then the two shell directories collapse into one package with
   `cfg(target_os)` modules for `os.rs`, the window code and the Windows-only widgets.
7. The oracle then compares generated text with the files (no harness).

## What to check by hand

Windows (coordinator, screenshot e2e): the bar starts, shows the storage and time widgets with
a click on storage opening Explorer on the drive, and `komorebi-bar --schema` equals
`schema.bar.json`. macOS (also record in `docs/dev/macos-handcheck.md` if it exists): the bar
starts, the time widget renders (format variants, `changing_icon`), clicking a storage widget
opens the mounted volume in Finder (`open <path>`), `komorebi-bar --schema` equals
`komorebi-for-mac/schema.bar.json`, and the clicks that spawn `cmd.exe`/`explorer.exe`
(battery, update) still fail the way they did before (preserved, one-function fix).
