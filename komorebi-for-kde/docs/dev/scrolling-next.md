# The rework of the Scrolling layout on upstream's `next`

Upstream (LGUG2Z/komorebi) reworked the Scrolling layout on its `next` branch: a platform-free
model of a strip of columns (`komorebi-scrolling`), the glue that every komorebi shares
(`komorebi_layouts::scrolling`), and, on top of them, columns that hold several windows and the
commands that go with them. This page says, for each of the commits of `master...next` that touch
the layout, what it changes, whether komorebi for KDE (kfde) takes it, and how. The review is of
`next` at `6d862a114` (the revision kfde's `Cargo.toml` pins now).

Two things shape what applies. The model and the platform-free glue live in crates that kfde takes
from upstream by git revision, as it already took `komorebi-layouts`, so a commit that only
touches them arrives with the bump of the revision. And what the glue needs of a window (where it
is, whether it is hidden, how wide it really is) is a call to the backend in kfde, not to Win32.

## Commits of the layout

| Commit | What it changes | kfde |
|---|---|---|
| `9ccf2efce` model of a scrollable strip | New crate `komorebi-scrolling`: columns of tiles, widths (proportion or fixed, limits), heights, view offset, centring (`Never`, `Always`, `OnOverflow`), pan, swipe tracker. No Windows code. Tests in the crate. | **Ported by dependency.** `komorebi-scrolling` is a git dependency at the same revision as the layouts. Its tests are upstream's; kfde tests the glue. |
| `abd4fde3e` lay out with the model | The workspace keeps a `ScrollingSpace` keyed by container id; `update()` places containers from it instead of recomputing equal columns from the last layout. | **Ported** (`komorebi/src/scrolling.rs`, `Workspace::update`). |
| `cb127204c` platform-free glue | `komorebi_layouts::scrolling` (options, `place`, `resize`, hand-over between workspaces), `ScrollingCentering` replaces the `bool` of `center_focused_column`; Workspace methods for focus, order, consume, expel, stack. | **Ported:** the layouts half by the bump, the Workspace half in `komorebi/src/scrolling.rs`. |
| `249709e60` commands and config | Socket messages and `komorebic` commands `consume`, `expel`, `toggle-column-stack`, `scrolling-layout-full-width`, `center-focus`, `expand`; focus, move, resize, retile and close go through the strip. The message says a top level `scrolling` section too. | **Ported**, except the `scrolling` section: neither kfw nor kfde has one (only komorebi for Mac has; COMPARE.md says so). |
| `9ff278eb5` schemas | `center_focused_column` is a boolean or a name; docs for the new commands; `schema.bar.json`. | **Ported:** `schema.json`, `nix/komorebi-options.nix`, `docs/cli/`. `schema.bar.json` is kfw's (kfde has no bar). |
| `2dff53095` room for windows that will not get narrow | After windows are placed, a window wider than its column widens the column (never narrower again, never wider than the screen), for an overhang of 16 px or more. | **Ported, with a different trigger.** kfde cannot ask a window how wide it is right after moving it: the frame comes back from KWin a moment later. The widths are read at the first layout after the frames have settled (`WIDTH_SETTLE`), as upstream does with animations on. Covered by unit tests in `scrolling.rs`; no VM check, because it needs a client that will not get narrower and foot has no minimum width. |
| `e75f3db47` rest windows at the edge | A window out of view rests at the edge it left by with 5 px on screen and keeps its size. | **Ported** (`parked`). On KDE this also keeps the window from being resized while it moves out of view, which is what KWin pulls back (PLATFORM.md). |
| `ab1896330` docs | `docs/usage/scrolling-layout.md`, `mkdocs.yml`. | **Ported**, rewritten for KDE (no animation, no borders at the edges, no touchpad). |
| `1531a892a` more than one monitor | Removes the single-monitor restriction; a window out of view past an edge with a monitor next to it is hidden, not parked; a partly visible one is cut to its work area; a window that comes back is shown first, only the one at the front of a stack. | **Ported** (`rest`, `other_monitors`, `should_restore_when_placed`, from `backend().monitors()` and `Window::hide`). kfde never had the restriction. This answers the open question in PLATFORM.md about a strip that spills onto the next monitor. |
| `abb230b45` spread a stack out | The window that stays in the container is restored; focus stays on the column while the containers are inserted. | **Ported.** |
| `ca22b0f81` refocus after Hide | The `Hide` event arm refocuses like `Destroy`. | **Not applicable.** kfde has no `Hide` event: KWin reports a closed window as a destroyed one, which already refocuses. |
| `f2b72a029`, `f22560464` left edge | Resizing a column by its left edge moves that edge, however far. | **By the bump** (`komorebi_layouts::scrolling::resize`). kfde's drag of an edge goes through the same function, and `LiveResize` keeps the strip (below). |
| `1f658fd90` `pan` | `komorebic pan left\|right` moves the view a column. | **Ported.** |
| `09b99700f` touchpad | `pan --stream` reads whkd's gesture stream; `PanGesture`, swipe tracking, pictures over the windows. | **Not applicable.** whkd and the raw touchpad input are Windows only. The swipe tracker is in the model crate if a KDE input path wants it later. |
| `674ae7937`, `5098ff7b7` resize follows the drag | The neighbours follow a dragged edge; the strip is kept with the rest of what the drag started from. | **The strip part ported:** `LiveResize` keeps and restores `Workspace::scrolling`, or every step of a drag would compound on the widths of the one before. The pictures over windows are DWM. |
| `ce13267e1`, `62628f381`, `2a447208f`, `58693ecd3`, `52ad6be78` | Where a drag is measured from; pictures and ghosts for windows being resized or animated. | **Not applicable.** Ghost windows and DWM. kfde measures a drag from `Backend::drag_origin` already. |
| `e5512ca5d` and the other `animation` and `border` commits | Ghosts, workspace slide, border in ghosts, batches of animations. | **Not applicable** (DWM, ghost windows, borders). The Windows session's. |
| `3b424dd70`, `837ca257d`, `5c8e4770d`, `9b9cad997` | Fixes in config, monitors, rules. | **Not scrolling.** Already in kfde or in the hands of the other sessions. |

## What I could not judge

- Whether the 5 px of a parked window pass KWin's rule of keeping 100 px of a resized window on an
  output. The bridge's `settle` asks again for a position up to three times, and a parked window
  is not resized to be parked, but only the VM says.
- Whether `Window::hide` of a column that scrolls out of view next to another monitor is fast
  enough to follow a scroll without a flash. It is what stacks do.
- The 16 px tolerance of `2dff53095` against the clients of `client-geometry` (`foot` rounds to
  cells, which is below it).

## What is tested, and where

- `komorebi/src/scrolling.rs`: the glue against a workspace with no desktop (widths, consume, expel,
  moving a column, stacking and spreading out with the windows that are hidden, expanding, panning,
  where a window rests, which monitors are the others, what a window that will not get narrow
  says). The model itself has its tests in upstream's `komorebi-scrolling`.
- `komorebi/tests/scrolling.rs` and `layout_options.rs`: the commands through
  `WindowManager::process_command` against the fake desktop, a strip next to another monitor, and a
  drag of the left edge of a column.
- `komorebic/tests/linux_cli.rs`: each new subcommand sends its message.
- `nix/tests/vm/lib/komorebi_vm/core/scenarios/scrolling.py` (`columns`, shared with komorebi for
  Windows), run by `vm-scrolling` and `core-e2e`: where the daemon keeps a window at the edge and the
  commands of the columns, in a real session. Run on KDE: the `scrolling` suite passes
  (32 checks, with `two_monitors`). KWin leaves a window resting at the left edge with its right edge a few pixels inside the
  work area, or a cell short of them, because foot rounds its frame to whole cells. The shared
  `scrolling` scenario passes in `core-e2e` too.
