# One `komorebi-bar` for Windows and macOS

Status at the revision that adds this file. Everything below was measured on this tree with
`diff` (counts are lines that differ, `<`/`>` lines of `diff`, hunks in parentheses). Nothing
here was rendered: the bars cannot run on Linux. Gates that did run are named; the ones that
did not are listed at the end.

KDE has no bar: `komorebi-for-kde` has no `komorebi-bar` package and no `schema.bar.json`.
The plan below is for two platforms; a KDE bar would add a third shell and a third schema view.


## What exists

- `crates/komorebi-bar-core` (4,592 lines): the code both bars share, one copy each.
  `selected_frame`, `ui`, `statics`, `alignment`, `render`, `config` (the leaf types and the whole
  `KomobarConfig` with the mouse options), the widgets `applications`, `battery`, `cpu`, `date`,
  `memory`, `storage`, `time`, `update`, `komorebi_layout`, the icon cache (`widgets/icons.rs`),
  all widget configuration types (`komorebi_config`, `network_config`, `platform_config` for
  keyboard, media and systray), `WidgetConfig` with a variant for every widget, the `BarWidget`
  trait, `platform` (`BarPlatform`, `SystemTool`, a once-installed global) and `schema`
  (per-platform views, see below).
- `BarPlatform` hooks: `open_system_tool`, `latest_release_api_url`, `calendar_icon`,
  `command_icon`, `exec_shell_command`, `send_komorebi_message`, `send_komorebi_batch`.
- Two shells, `komorebi-bar` (Windows, root workspace, version 0.1.42) and
  `komorebi-for-mac/komorebi-bar` (own workspace, own `Cargo.lock`, own flake, version 0.1.1).
  Each keeps `main.rs`, `bar.rs`, `os.rs`, the one-line `config.rs`, and the widgets whose
  runtime differs: `komorebi.rs`, `network.rs`, and on Windows `keyboard.rs`, `media.rs`,
  `systray.rs`. `widgets/widget.rs` is only `WidgetConfigExt` (config -> widget).
- The schema oracle regenerates the macOS file natively
  (`tests/schema_oracle.rs`, module `generated`); the Windows file is still checked by the
  out-of-tree harness because three descriptions read differently when compiled for Windows.

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
`komorebi-protocol`/`komorebi-config`) regenerates both files on Linux. Since `config.rs` moved, the
in-repo oracle regenerates the macOS file itself (`generated` tests); only Windows needs the harness.

## What is still per platform (measured at this tip)

| file | win / mac lines | differing | what differs and why it is not moved |
|---|---|---|---|
| `widgets/komorebi.rs` | 940 / 986 | 136 (17) | Two things. (1) Real rendering differences: macOS wraps `show_label` and the "all icons" painter in a `Frame` with a margin and a minimum height (`bar.show_label(...)`, a method, vs an associated function on Windows), `1.0` vs `1.0_f32`. Taking either spelling is a visual change on the other platform: needs a screen. (2) The conversion from the daemon state: Windows uses `komorebi_client::{State, Window, Workspace, Container}` (`hwnd`, `title()`, `process_id()`, windows-icons), macOS its own client types (`details.title`, `details.icon_path`, `icns_to_rgba`, `device`, no `Layout::Custom`). A State-to-snapshot split (`WindowInfo`/`MonitorInfo` of `komorebi-backend` as the carrier) is not clean yet: the macOS and Windows `komorebi_client` `State` are different types, the bar subscribes to the daemons with different notification shapes, and I cannot test either subscription here. Not attempted. |
| `widgets/network.rs` | 598 / 578 | 82 (19) | Windows reads `default_interface_options` (friendly name, IPv4, IPv6), macOS does not (the config type is shared and macOS logs a warning); `std::sync::Mutex` (poisoning handled) vs `parking_lot`; `netdev` 0.46 vs 0.40 and a different interface match key (friendly name vs `name`); `cmd.exe /C ncpa` vs `open x-apple...`. Functional on macOS: BEHAVIOUR, needs a Mac. Known difference, left as is. |
| `bar.rs` | 1,399 / 1,105 | 444 (35) | Windows: PowerShell session (`exec_powershell`, behind the hook), `hwnd`, virtual-desktop restore/minimize, `position_bar` through `komorebi_client::Window`, CJK font fallbacks, `scale_factor`, the Windows mouse handling. macOS: unboxed notification type, window shadow and level. Not behaviour-preserving without a screen. |
| `main.rs` | 402 / 388 | 92 (14) | `process_hwnd`/`enum_window`, DPI awareness, `set_read_timeout`; macOS activation policy and window level (AppKit), config dir `~/.config/komorebi`. Window and viewport setup belongs behind `BarPlatform` hooks (`init_process`, `viewport_builder`, `after_creation`), not written: it cannot be exercised here. |
| `os.rs` | 83 / 60 | 71 (14) | the platform implementations themselves; stays per platform (`cfg(target_os)` modules after the merge). |
| `widgets/widget.rs`, `widgets/mod.rs` | 41 / 49, 22 / 19 | 20, 5 | `WidgetConfigExt` (macOS builds a no-op `Unsupported` for the three Windows-only widgets) and the module lists. |

Windows-only and not shared: `widgets/keyboard.rs` (167), `media.rs` (322), `systray.rs` (1,205);
their config types are in the core. The macOS copies of keyboard and media were dead and are deleted.

### Known macOS/Windows differences that were kept (D-coord-1)

- `network.rs`: see above. The macOS bar ignores `default_interface_options`.
- Windows-only config keys are accepted on macOS and skipped with a logged warning (BEHAVIOUR on
  macOS, as for `StaticConfig`): `Keyboard`, `Media` and `Systray` widgets
  (`widgets::widget::warn_unsupported`), the `mouse` options and the network
  `default_interface_options`. Before, the first group failed to deserialize and rejected the whole
  config, the other two were ignored silently.
- macOS commands that spawn programs that do not exist there are preserved, not fixed:
  `SystemTool::PowerSettings` (battery click) spawns `cmd.exe /C start ms-settings:...`,
  `SystemTool::Url` (update click) spawns `explorer.exe`, and `UserCommand::launch_if_ready` of the
  applications widget spawns `cmd /C <command>` in both bars. `update.rs` still has a second GitHub
  API URL for the Windows repository. The macOS spawns live in
  `komorebi-for-mac/komorebi-bar/src/os.rs`; the storage click is `SystemTool::StorageLocation` and
  each shell spawns what it spawned before.
- `KomorebiLayout` is `#[serde(untagged)]` in both bars now (the macOS source converged; the macOS
  schema view restores the macOS text). Its `Deserialize` is hand-written and nothing in the bar
  serializes it, so this changes only the derived schema and `Serialize`. The coordinator said to
  leave it alone; it is a separate revision (`mac-bar: converge komorebi.rs config text and
  komorebi_layout.rs`) and can be dropped, at the price of keeping `KomorebiLayout` and its
  widget in the shells.

## The merged package layout (for the coordinator's workspace merge)

Not done and not prepared in the tree (no workspace or lockfile structure was changed). When the
root workspace and `komorebi-for-mac` are merged the bar should look like this:

- Package `komorebi-bar` (one, in `komorebi-bar/`) plus `crates/komorebi-bar-core` as today.
  Version: the workspace version. The macOS tree's `komorebi-bar` 0.1.1 and the `v0.1.0` text in
  its schema description are a view concern (`schema/changes.rs`), not a package one.
- `src/main.rs` (shared: the argument parsing, config read, `--schema`, the event loop shell)
  and `src/bar.rs` split into `src/bar/mod.rs` (shared `Komobar`) with
  `#[cfg(target_os = "windows")] mod windows;` and `#[cfg(target_os = "macos")] mod macos;`
  for the window handle, DPI, activation policy, window level, position and the PowerShell
  session. `src/os.rs` becomes `src/os/{windows,macos}.rs` (the `BarPlatform` implementations,
  selected by `cfg(target_os)` and installed first thing in `main`). Windows-only widgets
  (`keyboard`, `media`, `systray`) are `#[cfg(target_os = "windows")]` modules; `komorebi.rs` and
  `network.rs` stay two files (`komorebi/{windows,macos}.rs` behind one `widgets::komorebi`
  facade) until the snapshot split.
- `Cargo.toml`: `windows`, `windows-core`, `windows-icons*`, `systray-util`, `font-loader` under
  `[target.'cfg(windows)'.dependencies]`; `objc2`, `objc2-app-kit`, `raw-window-handle`, `icns`
  under `[target.'cfg(target_os = "macos")'.dependencies]`; `netdev` has two versions (0.46,
  0.40): pick one when `network.rs` converges, until then a target-specific rename
  (`netdev-win`, `netdev-mac`). `komorebi-client`: one crate (the two differ in 150 lines).
  Features: `schemars` as now (it forwards to the core). No new feature per platform: the
  platform is `cfg(target_os)`.
- Schema: the checked-in files stay (`schema.bar.json`, `komorebi-for-mac/schema.bar.json`, or one
  directory for both after the merge). `--schema` prints the view of the running platform
  (`SchemaPlatform` from `cfg(target_os)`); a `--schema-for <platform>` flag would let one host
  regenerate both, as the Windows union is the Windows text. Windows' three
  platform-dependent descriptions (protocol `PathBuf`, themes) need the Windows target to
  regenerate, so the Windows file is only verifiable on Windows or with the harness.
- Lock files: the macOS `Cargo.lock` folds into the root one; the lines this stack added to both
  (`image`, `komorebi-config`, `komorebi-layouts`, `komorebi-protocol`, `komorebi-themes`,
  `tracing`, `lazy_static`, `color-eyre` for bar-core) are the same crates in both, so the merge
  adds no new versions beyond `netdev` and `starship-battery`/`sysinfo` (already unified here).
- Nix: `komorebi-for-mac/nix/komorebi-bar-options.nix` is generated from the macOS file and does
  not change; the two flakes' `extractBinary "komorebi-bar"` become one derivation per system.

## The plan, in order

1. Done: schema views and oracle; storage, time, applications, komorebi layout widget, icon
   cache, all widget config types, `WidgetConfig`, `config.rs` moved; hooks for platform
   spawns, the shell session and the komorebi client.
2. Not done, needs a screen or a Mac: `bar.rs`/`main.rs` window code behind `BarPlatform`
   (`init_process`, `viewport_builder`, `after_creation`, position/DPI/activation), then
   `Komobar<P>` and the shared `app` body into the core.
3. Not done, needs the snapshot split: `komorebi.rs`. It needs `State`-to-snapshot conversion
   per platform and one renderer; the frame/margin difference must be decided by looking at both.
4. Decision for `network.rs`: converge on the Windows widget (macOS gains friendly-name,
   IPv4/IPv6 and the `netdev` bump) or keep two.
5. Workspace merge (above), then the two shells collapse into one package.
6. The oracle then compares generated text with the files for both platforms.

## What to check by hand

Windows (coordinator, screenshot e2e): the bar starts; the storage, time, applications (icons
from executables), komorebi layout (click opens the options, choosing one changes the layout,
Monocle/Floating toggles, paused) and komorebi workspaces widgets render; a click on storage
opens Explorer on the drive; a config with `mouse` options and a `Systray`/`Keyboard`/`Media`
widget still works (this is the Windows path through the moved `config.rs`, `WidgetConfig` and
the `exec_shell_command` hook: a `Command` mouse message must still reach the PowerShell session);
`komorebi-bar --schema` equals `schema.bar.json`.

macOS (also in the section for `docs/dev/macos-handcheck.md`): the bar starts with your real
config; the time widget renders (format variants, `changing_icon`); clicking a storage widget
opens the mounted volume in Finder (`open <path>`); the komorebi layout widget click works as
before; a config that contains a `Keyboard`, `Media` or `Systray` widget, a `mouse` block, or
`default_interface_options` under `Network` starts the bar and logs one warning per item (before:
the widget case failed to load the config); `komorebi-bar --schema` equals
`komorebi-for-mac/schema.bar.json`; the battery and update clicks still fail the way they did
(preserved, one-function fix in `os.rs`).
