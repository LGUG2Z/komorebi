# The platform seam: evidence

Status: evidence document (step 7a of the consolidation). No `Backend` trait is proposed here as code, and none may be written before this document has been reviewed. Every statement about the code carries a citation; where I could not confirm something it says "not verified" and the item is collected in section 8.

## 1. Purpose and how to read it

A shared `Backend` seam has to sit between code that is to be shared (windows, containers, workspaces, monitors, commands, event handling) and three platform layers that today do the same jobs in three different ways. A trait written from memory of one platform would bake that platform's assumptions into the other two. This document records what each platform actually does, so that the trait can be derived from facts.

- **Facts** are the tables and the numbered findings, each with a `file:line` citation read on the tips below. Nothing in a table is a proposal.
- **Proposals** are confined to section 7 (consequences). Each consequence is a statement about what the seam must satisfy, with the facts that support it, and is marked CONFIRMED, CORRECTED or TARGET (not true of any current code, but something the seam would have to make true).
- "Not verified" means I read the code and could not settle the point by reading (usually: Apple or Microsoft runtime behaviour, or behaviour that needs a Mac). It does not mean "false".

Tips read: integration line `wip-main` at `aa52fdc4d9e5` (`build(nix): relock the nested flakes after the step 1 and crates changes`). The three trees live in one checkout:

| Tree | Prefix used below | Path |
|---|---|---|
| Windows (repo root) | `W/` | `komorebi/src/` |
| KDE | `K/` | `komorebi-for-kde/komorebi/src/` |
| macOS | `M/` | `komorebi-for-mac/komorebi/src/` |

Shared crates are cited with their full path (`komorebi-layouts/src/rect.rs`). Only reading and `grep` were used; nothing was compiled or run, and no macOS or Windows runtime behaviour was observed.

## Corrections to earlier claims

Things in the plan (`generic-percolating-matsumoto.md`, Step 7 and "Risks") or the progress log that this reading contradicts or refines.

1. **KDE `Backend` method count.** The trait declares **31** methods: 19 required (`K/backend/mod.rs:242-258`, `325-389`, `408-432`) and 12 provided (`:280`, `:311`, `:473`, `:504`, `:529`, `:553`, `:586`, `:597`, `:607`, `:612`, `:617`, `:622`). The plan's "28 (not 31)" matches only if the three derived helpers `is_window`, `window_rect`, `monitor_from_point` (`:612-628`, written in terms of `window` and `monitors`) are left out. Use "28 primitives plus 3 derived".
2. **macOS `monitors()` is not a cache** (already corrected by the kfm A0 report, confirmed). `MacosApi::latest_monitor_information` queries `NSScreen` live and does `MainThreadMarker::new().unwrap()` (`M/macos_api.rs:212-213`), so it panics off the main thread; every caller is already on main (section 3.3). "monitors() reads a cache refreshed by the display callback" is a TARGET (consequence 4).
3. **Consequence 2 is too strong as written.** "`set_rect` is never read back to confirm" is false of macOS: after a direct write the layout reads the frame back to learn whether the app refused the width (`M/window.rs:944-972`, `M/arrival.rs:1-12`), and the animator verifies each journey (`M/animation/animator.rs:902`). KDE returns an optimistic value: `set_rect` writes the requested rect into its own table at once (`K/kwin/state.rs:596-600`, called from `K/kwin/mod.rs:855`). The defensible contract is in section 7.
4. **The KDE `u32` id is not "the compositor's".** The doc comments say so (`K/backend/mod.rs:65`, `K/window.rs:78-81`), but the id is a counter that the daemon assigns to each KWin UUID string (`K/kwin/state.rs:153-154`, `603-611`). KDE already has a registry (uuid <-> id) behind its backend, and `KWinBackend::command` resolves every command through it (`K/kwin/mod.rs:797-806`).
5. **The macOS deadlock has more entry points than the plan lists.** The plan names `destroy_all_borders` from the event thread (`M/process_event.rs:128, 139`). A blocking main-queue dispatch under the window-manager lock also happens from the event thread at `M/process_event.rs:791` (`activate_finder`, `M/macos_api.rs:492`), and from command threads (`M/process_command.rs:294, 303, 1776, 1802, 1806`) and workspace switching (`M/window_manager.rs:1385`, `2286`). Section 3.3.
6. **Windows HWND width**: the code never narrows an HWND, so it assumes nothing about 32-bit significance (section 4). The Microsoft-documented fact that handles are 32-bit significant is not verified here.
7. **Counts** (re-measured; method in section 2.3). KDE trait: 31 (not 28, see 1). Windows `WindowsApi`: 71 distinct functions used outside `windows_api.rs` (matches), 280 call sites in `W/` plus 3 in `komorebi-bar/src/bar.rs` = 283 (earlier survey 279). The generic / Windows-only split depends entirely on the rule used; with the rule stated in 2.3 it is 41 / 30 functions and 219 / 61 sites (earlier survey 42 / 29 and 209 / 70). macOS: 14 `MacosApi` functions, 5 of 8 `CoreGraphicsApi` functions, 13 `AccessibilityApi` functions are used outside their API files; the earlier "52 raw AX sites, 33 in window.rs" does not reproduce for the total: **67** `AccessibilityApi::` sites in 12 files, 33 of them in `M/window.rs` (that part agrees).
8. **The plan's list of macOS main-thread-only work is incomplete in one respect.** The AX observer and event-tap callbacks run on main and do synchronous AX IPC to other applications there (`M/window.rs:288-350`, `M/input_event_listener.rs:96-100`): "main thread" is not just "cheap UI work".
9. **macOS has no focus-follows-mouse implementation**, only config/state fields (`grep` finds `FocusFollowsMouse` in `M/static_config.rs`, `M/state.rs`, `M/core/mod.rs` and nowhere else); KDE and Windows both have one. The plan lists focus-follows-mouse among platform subsystems; for macOS it is absent, not platform-specific.
