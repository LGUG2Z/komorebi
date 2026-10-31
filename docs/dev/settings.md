# Settings: replacing the process-wide statics

Status: design (plan step 10, phase 1). Measured on `wip-main` 633e7453 (the Windows tree
`komorebi/src`, KDE `komorebi-for-kde/komorebi/src`, macOS `komorebi-for-mac/komorebi/src`). Nothing
here was run: counts come from `grep` and reading, and macOS cannot run on this host. The shared type
is landed as the crate `crates/komorebi-settings` (phase 2) and **no daemon uses it yet**.

Abbreviations: `W/`, `K/`, `M/` are the three `src` trees. `apply_globals` is
`StaticConfig::apply_globals` (`W/static_config.rs:396`, `K/static_config.rs:414`,
`M/static_config.rs:392`).

## 1. What the statics are

Declarations of `static`, `static ref` and `lazy_static` (thread locals and per-call counters left out):
**Windows 126, KDE 53, macOS 74.** Most are not settings. Four kinds:

| kind | examples | goes into `Settings`? |
|---|---|---|
| **config**: written by `komorebi.json` load and/or a `SocketMessage`, read by other threads | `BORDER_*`, `ANIMATION_*`, paddings, rule lists, stackbar | yes |
| **session state**: changes while running, is not configuration | `SESSION_FLOATING_APPLICATIONS`, `UNMANAGED_WINDOW_IDS`, `HIDDEN_HWNDS`, `WORKSPACE_MATCHING_RULES`, `INITIAL_CONFIGURATION_LOADED`, `CUSTOM_FFM` (command line) | no; stays where it is (core step 9) |
| **plumbing**: channels, `OnceLock<Sender>`, caches of OS handles, per-window tables | `CHANNEL`, `BORDER_STATE`, `MONITOR_CACHE`, `HWNDS_CACHE`, `PARKED_WINDOWS`, `live_resize.rs` flags | no |
| **environment**: computed once | `HOME_DIR`, `DATA_DIR`, `AHK_EXE`, `WINDOWS_11` | no |

Config-class statics by my count: **Windows 61, KDE 35, macOS 36** (these include the session-state
rule lists that sit next to them; see 2.3). What `apply_globals` itself writes (distinct statics,
counted on the function body): **Windows 51, KDE 29, macOS 28.** The plan's "19-27 written per
platform" counted scalar settings only; the 11 rule lists, the 7 per-animation/global animation
statics and the stackbar block make the totals larger.

## 2. The inventory (config class)

"Writers": `load` is `apply_globals` (runs on start-up through `preload`, and again on every
`ReloadConfiguration` through `reload`), `cmd` is a `SocketMessage` arm of `process_command.rs`,
`theme` is `theme_manager`, `thr` is another thread. "Readers" lists only threads that are not the
window-manager thread; "wm" means code that runs under the window-manager lock. Match is the name
on the other platforms (`=` same name and type, `~` same name, different default or type, `-`
absent).

### 2.1 Scalars and small values

| static | type | writers | readers | W / K / M | shared? | note |
|---|---|---|---|---|---|---|
| `BORDER_ENABLED` | AtomicBool | load, cmd `Border` | border thread (all 3), wm | = = = | shared | K reads it to decide whether borders take room in the layout (`BORDER_DRAWN` is its derived twin, K only) |
| `BORDER_WIDTH` | AtomicI32 | load, cmd | border thread, wm (offsets), stackbar (W) | ~ default 8 / 10 / 10 | shared | |
| `BORDER_OFFSET` | AtomicI32 | load, cmd | border thread, wm | ~ default -1 / 0 / 0 | shared | |
| `BORDER_RADIUS` | AtomicI32 | load | K effect, M `ns_window` | - K M | K+M | Windows draws square or via DWM (`STYLE`) |
| `BORDER_OFFSET_ADJUSTMENT` | `static i32` = 7 | nobody | M border | - - M | platform constant | stays a constant |
| `FOCUSED UNFOCUSED UNFOCUSED_LOCKED MONOCLE STACK FLOATING` | 6 x AtomicU32 | load, cmd `BorderColour`, theme | border thread | = = = | shared | `UNFOCUSED_LOCKED` is written by theme only (load never writes it); one theme change writes 6 border and (W) 3 stackbar values one by one |
| `STYLE` | AtomicCell<BorderStyle> | load, cmd | border thread, wm | W only | W platform | load resets it to default when the file has none |
| `IMPLEMENTATION` | AtomicCell<BorderImplementation> | load, cmd | border thread | W only | W platform | |
| `ANIMATION_ENABLED_GLOBAL` | Arc<AtomicBool> | load, cmd, **scoped override**, shutdown | W animation engine, M animator and lane threads, wm | = = = | shared | see 4 (override) |
| `ANIMATION_STYLE_GLOBAL` | Arc<Mutex<AnimationStyle>> | load, cmd | same | = = = | shared | |
| `ANIMATION_DURATION_GLOBAL` | Arc<AtomicU64> | load, cmd | same | = = = | shared | |
| `ANIMATION_{ENABLED,STYLE,DURATION}_PER_ANIMATION` | Arc<Mutex<HashMap<AnimationPrefix, _>>> x3 | load, cmd (2 sites for style), shutdown (enabled) | same | = = = | shared | `load` clears the map when a global value is given, replaces it when a prefix map is given |
| `ANIMATION_FPS` | AtomicU64 | load, cmd | W engine, M animator (`frame_period`) | = = = | shared | load writes the default when `animation` is present but `fps` is not |
| `GHOST_MOVEMENT_ENABLED` | AtomicBool | load | W ghost dispatcher, live resize | W only | W platform | |
| `TRANSPARENCY_ALPHA` | AtomicU8 | load, cmd | W/K transparency manager | = = - | shared (W, K) | default 200 |
| `TRANSPARENCY_ENABLED` | AtomicBool | load (`transparency`), cmd (set, toggle) | transparency manager, W process_event | = = - | shared (W, K) | a command toggles what the file set; a reload overwrites it only when the key is present |
| `DEFAULT_WORKSPACE_PADDING`, `DEFAULT_CONTAINER_PADDING` | AtomicI32 | load | wm (every layout), W stackbar | ~ default 10 / 5 / 5 | shared | load keeps the old value when the key is absent |
| `DEFAULT_RESIZE_DELTA`, `DEFAULT_MOUSE_FOLLOWS_FOCUS` | `static i32 / bool` | nobody | wm | = = = | shared constants | become part of the platform defaults |
| `FLOATING_WINDOW_TOGGLE_ASPECT_RATIO` | Arc<Mutex<AspectRatio>> | load (2 sites) | wm | = = = | shared | load writes a default when absent |
| `LAYOUT_DEFAULTS` | Arc<Mutex<HashMap<DefaultLayout, LayoutDefaultEntry>>> | load (replace or clear) | wm | = = = | shared | |
| `DISPLAY_INDEX_PREFERENCES` | Arc<RwLock<HashMap<usize, String>>> | load, cmd | monitor reconciliator thread, wm | = = = | shared | |
| `MONITOR_INDEX_PREFERENCES` | Arc<Mutex<HashMap<usize, Rect>>> | load, cmd | W windows_api, reconciliator | W only | W platform | commented out of macOS and KDE load |
| `DUPLICATE_MONITOR_SERIAL_IDS` | Arc<RwLock<Vec<String>>> | reconciliator, windows_api | windows_api | W only | **not config** | derived from the monitor list |
| `HIDING_BEHAVIOUR` | Arc<Mutex<HidingBehaviour>> | load, cmd, (state loader) | **reaper thread** (every 20 ms), wm | W only | W platform | reaper reads it without the wm lock |
| `WINDOW_HANDLING_BEHAVIOUR` | AtomicCell<WindowHandlingBehaviour> | load, **scoped override** in `border.rs:831-851` | W windows_api (every position call), ghost dispatcher, scrolling | W only | W platform | see 4 |
| `MINIMUM_WIDTH`, `MINIMUM_HEIGHT` | AtomicI32 x2 | load | wm | W only | W platform | commented out of the macOS load |
| `SLOW_APPLICATION_COMPENSATION_TIME` | AtomicU64 | nobody after init | W window | W only | W platform | effectively constant 20 |
| `REMOVE_TITLEBARS` | AtomicBool | cmd | wm, workspace | W only | W platform | runtime toggle, not in `apply_globals` |
| `STACKBAR_*` (9: font size/family, focused/unfocused text, tab background, height, width, label, mode) | AtomicI32/U32/Cell, `Mutex<Option<String>>` | load, cmd, theme (3 colours) | stackbar threads, wm | W only | W platform | load sets font size and family unconditionally (0 / None) when `stackbar.tabs` is present |
| `STACKBAR_TEMPORARILY_DISABLED` | AtomicBool | ghost dispatcher | stackbar manager | W only | session state | |
| `CURRENT_THEME` | AtomicCell<Option<Theme>> | theme thread | theme thread only | = = = | not config | read nowhere else |
| `TABBED_APPLICATIONS`, `TITLELESS_APPLICATIONS`, `DO_NOT_OBSERVE_APPLICATIONS` | Arc<Mutex<Vec<String>>> | load (first two) | M window, process_event | M only | M platform | |

### 2.2 Rule lists (the lock-ordered set)

| static | writers | readers | W / K / M |
|---|---|---|---|
| `FLOATING_APPLICATIONS`, `IGNORE_IDENTIFIERS`, `MANAGE_IDENTIFIERS` | load (all three held at once) | wm; W also `winevent` and `window_manager_event` threads through `window_is_eligible` | = = = |
| `TRANSPARENCY_BLACKLIST` | load | transparency manager | = = - (K: pub) |
| `LAYERED_WHITELIST`, `TRAY_AND_MULTI_WINDOW_IDENTIFIERS`, `OBJECT_NAME_CHANGE_ON_LAUNCH`, `OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST`, `SLOW_APPLICATION_IDENTIFIERS`, `NO_TITLEBAR` | load | wm, event threads | W only |
| `REGEX_IDENTIFIERS` | load (clears), every rule check (compiles on demand) | every rule check | = = = |
| `PERMAIGNORE_CLASSES` (W also `WSL2_UI_PROCESSES`) | nobody after init | window eligibility | = = = (W: +1) |
| `SESSION_FLOATING_APPLICATIONS` | cmd (add/remove at run time) | window eligibility | = = = |
| `WORKSPACE_MATCHING_RULES` | cmd, **reload (clears, then refills after `apply_globals`)** | wm, reconciliator, M process_event | = = = |

### 2.3 Lock-order relations

The documented order (comment above the statics in `komorebi/src/lib.rs` on `wip-u-kfw`; it is the
only one: KDE and macOS declare no order and take the same lists in whatever order the code does):

```
FLOATING_APPLICATIONS, SESSION_FLOATING_APPLICATIONS, TRANSPARENCY_BLACKLIST, IGNORE_IDENTIFIERS,
MANAGE_IDENTIFIERS, LAYERED_WHITELIST, TRAY_AND_MULTI_WINDOW_IDENTIFIERS, OBJECT_NAME_CHANGE_ON_LAUNCH,
OBJECT_NAME_CHANGE_TITLE_IGNORE_LIST, SLOW_APPLICATION_IDENTIFIERS, NO_TITLEBAR,
WORKSPACE_MATCHING_RULES, REGEX_IDENTIFIERS
```

`REGEX_IDENTIFIERS` is last (every rule check holds a list and then the regexes); `PERMAIGNORE_CLASSES`
and `WSL2_UI_PROCESSES` are leaves. The window-manager lock is above all of them: `apply_globals` runs
with it held (the `reload` signature takes `&mut WindowManager`), but the event threads of Windows
(`winevent_listener` to `window_manager_event`) and the transparency manager also lock single lists
without it.

Two findings:

1. `apply_globals` on Windows takes **eleven** list locks in a fixed order and keeps them for the whole
   function (`W/static_config.rs:562-572`), so the rules change atomically with respect to each other,
   but not with respect to the scalars (written before and after).
2. `reload` clears `WORKSPACE_MATCHING_RULES` and refills it per monitor afterwards (`M/static_config.rs`
   after `apply_globals`, same on W and K): for the duration of the loop a reader that does not hold the
   wm lock sees an **empty** list. This is a live partial-update window today.

With `Settings` the immutable lists are fields of one `Arc` snapshot: readers take no lock, there is no
order to follow, and the lock order shrinks to: wm lock, then the two session mutexes
(`SESSION_FLOATING_APPLICATIONS`, the regex cache), never nested the other way round.

## 3. The design

### 3.1 Crates

New crate `crates/komorebi-settings` (std plus `parking_lot`; types from `komorebi-config`,
`komorebi-protocol` and `komorebi-layouts`; no daemon dependency, so it tests natively):

```rust
pub struct Settings<P> {
    pub shared: Shared,   // the same struct on all three platforms
    pub platform: P,      // Windows: border style, stackbar, hiding/handling behaviour, ...
}                         // KDE: nothing yet (the compositor owns look); macOS: tabbed/titleless apps

pub struct Shared {
    pub border: Border,                // enabled, width, offset, six colours
    pub animation: Animation,          // global + per-prefix enabled/style/duration, fps; motion_for(prefix)
    pub defaults: Defaults,            // container/workspace padding, resize delta, mouse-follows-focus
    pub transparency: Transparency,    // enabled, alpha (Windows and KDE; macOS keeps the default)
    pub floating_window_aspect_ratio: AspectRatio,
    pub layout_defaults: HashMap<DefaultLayout, LayoutDefaultEntry>,
    pub display_index_preferences: HashMap<usize, String>,
}

pub struct Handle<P> { /* RwLock<Arc<Settings<P>>> + a writer mutex */ }
impl<P: Clone> Handle<P> {
    pub fn snapshot(&self) -> Arc<Settings<P>>;                  // readers
    pub fn replace(&self, s: Settings<P>);                       // whole new value
    pub fn update<R>(&self, f: impl FnOnce(&mut Settings<P>) -> R) -> R;   // copy, edit, swap; writers serialised
    pub fn scoped<T>(&self, field: fn(&mut Settings<P>) -> &mut T, value: T) -> Scoped<'_, P, T>;
}
```

The **platform part** is a plain type parameter `P` (not a trait): each daemon names its struct
(`WindowsSettings`, `MacosSettings`, `KdeSettings`) and the handle is `static SETTINGS: LazyLock<Handle<P>>`
behind a `settings()` function of the daemon crate. `P` needs only `Clone + Debug`. A trait would add
nothing, because the shared code never calls into `P`: only the platform's own subsystems read it. When
the daemons become one crate this is `#[cfg(target_os)] type Platform = ...`.

The lists of rules and the stackbar block move into the platform parts first (they are Windows only or
differ); the rule lists join `Shared` in step G7 below once the lock order is retired.

### 3.2 How readers get a consistent snapshot

* A reader calls `settings().snapshot()` **once per unit of work** (one render pass of a border, one
  `motion_for` call, one layout pass, one window eligibility check) and uses that `Arc` for the whole
  unit. Cost: one uncontended `RwLock` read and an `Arc` clone; no atomics are needed on the hot
  paths I found (border thread loops once per notification, animator once per frame, rule checks once per
  event). If a measurement shows one needs it, add an `AtomicBool` *mirror* of that single field written by
  the same `update`; do not read the mirror and the snapshot for the same decision.
* `ArcSwap` was the first idea. It is **not in any of the three lockfiles** (checked), so it would add a
  dependency to three lockfiles and the flake hashes for no gain at this read rate. `Handle` hides the
  representation: swapping `RwLock<Arc<_>>` for `ArcSwap<_>` later changes one file.
* Writers go through `update`, which holds a mutex while it clones, edits and publishes, so two writers
  (a `SocketMessage` and a config reload, a theme change and a command) never lose each other's field. The
  statics lose such updates today only when both write the same field, but a theme change writes nine
  separate atomics and a command can interleave.
* A config load builds the new value and publishes it **once**; a theme change is one `update`.

### 3.3 Semantics that must survive: reload merges, it does not replace

`apply_globals` is a field-by-field merge: `if let Some(x) = self.border_width { store }`. A key absent
from a reloaded `komorebi.json` **keeps the previous value** (and a command-set value); a few fields are
instead reset (listed in section 4, "reset on reload"). A "build a fresh Settings from the file and swap"
would reset every absent key to its default. So the migration keeps the merge: `Settings::apply(delta)`
is written as the same sequence of `if let Some` over a copy of the **current** snapshot, published once.
Replacing instead of merging is a separate, optional BEHAVIOUR change, last in the list.

### 3.4 The scoped override

Two places write a setting, do work and put it back: Windows `border.rs:831-851`
(`WINDOW_HANDLING_BEHAVIOUR`: load, store `handling`, ..., store `previous`) and KDE
`process_command.rs:607-609` (`ANIMATION_ENABLED_GLOBAL.swap(false)`, `add_window_to_container`,
`store(restore)`). Both clobber a concurrent change (a reload or a command in between is overwritten with the old value). macOS
already has the right pattern: `animation::suspend()` returns a guard that counts and `motion_for` returns
`None` while it is held (`M/animation/mod.rs:36-71`). `Handle::scoped` gives the same for any field:
it sets the value and, on drop, puts the previous value back **only if the field still holds the
override value**, so a change made meanwhile wins. For animations KDE and Windows should adopt
macOS's `suspend` counter instead (a field of `Shared::animation`, not a scoped value).

## 4. Statics whose behaviour would change

Everything here is a **BEHAVIOUR** change if it is not made to keep today's semantics. Flagged items
say what the migration does about each.

**Partial updates visible mid-reload (today).** `apply_globals` publishes about 28 to 51 values
one at a time while the border, animator, reaper and transparency threads keep reading. A border
thread can draw once with the new width and the old colours; `motion_for` (macOS) reads enabled,
duration and style as three separate lock/atomic reads and can mix two configurations. With `Handle` all
readers see the old or the new `Settings`, never a mix: **BEHAVIOUR (intended, can only remove
tears)**, on every static of the table. The e2e that would show it is `config_reload`.

**Reset on reload (must be kept as written):** `LAYOUT_DEFAULTS` (cleared when absent),
`FLOATING_WINDOW_TOGGLE_ASPECT_RATIO` (default when absent), `ANIMATION_FPS` (default when `animation` is present
without `fps`), `GHOST_MOVEMENT_ENABLED` (W), `STYLE` and `IMPLEMENTATION` (W: default when absent),
`STACKBAR_FONT_SIZE` / `STACKBAR_FONT_FAMILY` (W: 0 / None when `stackbar.tabs` is present),
`ANIMATION_*_PER_ANIMATION` (cleared when the global form is used) and the rule lists (`apply_globals`
clears each only when the new file gives that list; check per list in G7).

**Keeps the old value when absent (must be kept):** borders (width, offset, radius, enabled, colours),
paddings, transparency alpha, hiding and handling behaviour, minimum size, stackbar scalars, display index
preferences.

**Scoped override (3.4):** KDE `ANIMATION_ENABLED_GLOBAL.swap/store` pair and Windows `WINDOW_HANDLING_BEHAVIOUR`
pair become guards. A concurrent reload no longer gets clobbered: BEHAVIOUR.

**Shutdown writes** (`main.rs:367-368`, `window_manager.rs:1445-1446` on W; the Stop handler on K and M): they
disable animation permanently before restoring windows. These are plain `update`s, not scoped.

**`WORKSPACE_MATCHING_RULES` empty window** (2.3): stays in the core until step 9 (session state); not part
of `Settings`, so not changed here.

**Defaults differ per platform** (border width 8/10/10, offset -1/0/0, padding 10/5/5, KDE border radius):
`Shared::defaults_for(PlatformDefaults::{WINDOWS, KDE, MACOS})`. Changing any default is user visible and is
not part of this step.

**Threads read the statics without the wm lock:** border thread (all three), W reaper
(`HIDING_BEHAVIOUR`), W winevent and window_manager_event threads (rule lists), W transparency manager,
macOS animator thread and the per-pid lane threads, macOS border thread. These are the readers whose behaviour a snapshot
changes (from "latest value on each read" to "value of the snapshot taken at the start of the unit"); the
gap is at most one unit of work.

## 5. Migration order (least to most risky)

Each group is one revision per platform tree (or per platform file set), moving the writers and **all**
readers of that group together, never dual-writing a static and a field. P = pure refactor (same
observable behaviour); B = BEHAVIOUR (marked in the revision body, not landed until its gate ran).

| step | group | kind | gate |
|---|---|---|---|
| S0 | the crate and its tests (landed, phase 2) | P | native `cargo test -p komorebi-settings` |
| S1 | `DEFAULT_RESIZE_DELTA`, `DEFAULT_MOUSE_FOLLOWS_FOCUS`, `BORDER_OFFSET_ADJUSTMENT`: constants into `PlatformDefaults` | P | KDE `cargo test -p komorebi --lib`, Windows clippy, macOS fake-CC check |
| S2 | install `settings()` in each daemon with the platform defaults (no reader yet) | P | same |
| S3 | paddings, aspect ratio, layout defaults, display index preferences (wm-lock readers and the reconciliator) | P for the readers; the load writes the new place once: B (tear) | KDE lib tests; Windows `config_reload`, `layouts`, `workspaces` e2e; macOS: reload with changed padding |
| S4 | transparency enabled and alpha (W, K) | P | Windows `config_reload` plus the transparency scenario if it exists; KDE lib tests |
| S5 | animation (global, per-prefix, fps): `motion_for`, engine, animator; KDE/Windows adopt `suspend()` guards | **B** (tear removed, override semantics) | KDE VM suites that stack windows (`stacks`); Windows animation e2e; **macOS hand-check**: animation on, switch workspace, change `animation.duration` by `komorebic` mid-animation, stack a window |
| S6 | border scalars and colours, theme colours as one `update` | **B** (the 6 colour atomics change together) | Windows border e2e (screenshot) and `config_reload`; KDE VM `borders`/look suites; **macOS hand-check**: `border-colour`, `border-width`, theme change, reload with changed border |
| S7 | Windows platform part: `HIDING_BEHAVIOUR`, `WINDOW_HANDLING_BEHAVIOUR` (with `scoped`), min size, stackbar block, border style/implementation, ghost movement | **B** (reaper reads a snapshot; override guard) | Windows: `commands`, `config_reload`, `lifecycle`, `lifecycle_hidden`, stackbars, border e2e |
| S8 | rule lists into `Shared` (immutable `Arc<RuleSet>`), session floating and regex cache stay mutexes; retire the lock-order comment | **B** (hot path of every window event; deadlock class removed) | Windows `config_reload` (lock order), `commands`, `focus`, `lifecycle`; KDE lib tests plus VM suites; macOS: manage/ignore rules by hand |
| S9 | `apply_globals` as one `update` (single publish), and one decision on merge versus replace | **B** | Windows `config_reload`; macOS reload by hand |

Risk grows with the number of threads that read without the wm lock and with how hot the read is:
S3 has none, S5 and S6 have the macOS threads the plan names, S7 has the reaper, S8 is every event.
Do S8 only after the Windows lock-order e2e from the `wip-u-kfw` stack has run once on the current
tree.

## 6. What is not in `Settings`

`SESSION_FLOATING_APPLICATIONS`, `UNMANAGED_WINDOW_IDS`, `HIDDEN_HWNDS`/`HIDDEN_WINDOWS`,
`WORKSPACE_MATCHING_RULES`, `REMOVE_TITLEBARS`, `CUSTOM_FFM`,
`INITIAL_CONFIGURATION_LOADED`, subscriptions, `*_CHANNEL`s, caches, `BORDER_STATE`: these are session or
plumbing state and belong to the model (step 9) or the subsystem. `Settings` is only what the user
configures.

## 7. What landed (phase 2) and findings from writing it

`crates/komorebi-settings` (native tests, 18 passed): `Settings<P>`, `Handle<P>` (`snapshot`, `replace`,
`update`, `scoped`), `Shared` (border, animation with `motion_for`, paddings and constants, transparency,
aspect ratio, layout defaults, display index preferences), `PlatformDefaults::{WINDOWS, KDE, MACOS}` and
`SharedDelta::apply`, which is `apply_globals` for the shared keys written as a function of the current
value. Not yet in the crate: rule lists, theme application as one update, anything platform specific.

Findings that only showed up while encoding the semantics:

* `FLOATING_WINDOW_TOGGLE_ASPECT_RATIO` starts at **Widescreen** on all three platforms, but a load
  whose file has no `floating_window_aspect_ratio` writes **Standard** (`unwrap_or`). So the first
  reload of a file without the key changes the ratio. `SharedDelta::apply` keeps this; fixing it is a
  separate user-visible change.
* `TRANSPARENCY_ENABLED` is written by the load (`transparency`) and by the toggle command, so it is a
  setting, not session state (corrected from the first draft of this document's table).
* `komorebi-config` warns about an unused import (`crate::defaults`, `static_config.rs:39`) when it is
  built with `default-features = false`; the settings crate builds it that way. Not touched.
* Windows `reload` runs `apply_globals` twice per reload: in `preload` of the new window manager
  (while the old one is paused) and again in `reload`; both writes become visible to the threads that
  read without the wm lock. One `update` per load keeps the two visible publishes.

## 8. S1 and S2 as landed for KDE and macOS, and the Windows patch (not applied)

KDE and macOS (`wip-u-settings`): S1 (the `DEFAULT_*` statics take their values from
`komorebi_settings::shared`, paddings from `PlatformDefaults::{KDE, MACOS}`; the statics keep their
names and types) and S2 (`src/settings.rs` with `KdeSettings` / `MacosSettings` and `settings()`;
a test pins that the starting settings equal the starting values of the border and default
statics). The new crate is a path dependency of both workspaces (manifest, package manifest,
`Cargo.lock` +1 package). S3 was **not** started: it moves the writers (`apply_globals`) and the
readers in `monitor.rs`, `workspace.rs`, `window.rs`, `process_command.rs`, which the KDE and macOS
stacks are editing, and the load would publish once instead of storing key by key, which is the tear
removal marked BEHAVIOUR in section 5. It needs the rebase onto the merged stacks and its gate.

Windows S1/S2, to apply when told (all in the files kfw is changing, so not done):

1. `komorebi/Cargo.toml`: `komorebi-settings = { path = "../crates/komorebi-settings" }` (the crate is
   already a root workspace member; `Cargo.lock` gets the dependency edge, no new package).
2. `komorebi/src/lib.rs`: `DEFAULT_WORKSPACE_PADDING` / `DEFAULT_CONTAINER_PADDING` start from
   `PlatformDefaults::WINDOWS.workspace_padding` / `.container_padding` (10, 10);
   `DEFAULT_RESIZE_DELTA` and `DEFAULT_MOUSE_FOLLOWS_FOCUS` are `komorebi_settings::shared::...`
   (50, true); add `pub mod settings;`.
3. New `komorebi/src/settings.rs`, a copy of the KDE one with `WindowsSettings` (empty) and
   `PlatformDefaults::WINDOWS`; its pin test compares the paddings, resize delta, mouse-follows-focus,
   `BORDER_ENABLED`, `BORDER_WIDTH` (8), `BORDER_OFFSET` (-1) and the six colours, and leaves out
   `border.radius` (Windows has no `BORDER_RADIUS`).
4. Gate: Windows clippy (`-D warnings`) and `cargo test -p komorebi --lib`; `tools/e2e-needed` will
   say YES (the crate is in the closure of `komorebi.exe`) so one e2e run is owed, but behaviour is
   unchanged.
