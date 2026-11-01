# Known issues that need a decision

Findings of the consolidation reviews and tests that are NOT fixed because whether they are bugs, or how to fix them, is a decision for the owner. Each has the evidence, why it may or may not be a bug, the platforms, and a proposed fix. Paths are relative to the repository root at the time of writing and may have moved.

## 1. RESOLVED (wip-u-ptr): KDE FocusWorkspaceNumber, FocusWorkspaceNumbers and FocusLastWorkspace skipped the pointer-monitor preamble

- **Resolution**: the owner approved Windows behaviour for all platforms. The preamble now runs for all three daemons in the shared `komorebi_core::command` functions (`focus_workspace_number`, `focus_workspace_numbers`, `focus_last_workspace`, and the others). KDE is covered by `komorebi-for-kde/komorebi/tests/pointer_monitor.rs`, core by `crates/komorebi-core/tests/commands.rs`. Owed: KDE VM commands and workspace-switch, macOS hand check H1 (docs/dev/macos-handcheck.md). The text below is the original finding.

- **Evidence**: komorebi-for-kde/komorebi/src/process_command.rs lines 1245-1277: the arms for the three messages call `focus_workspace` on the focused monitor directly. The Windows daemon (komorebi/src/process_command.rs:972-987 for `FocusWorkspaceNumber`, and lines about 2185 and 2215 for the helpers behind `FocusLastWorkspace` and `FocusWorkspaceNumbers`) first checks `monitor_idx_from_current_pos()`: when the pointer is on another monitor whose focused workspace is empty, that monitor becomes the focused one. KDE has the same preamble only in `cycle_focus_workspace` (line 897-909).
- **Why it may be a bug**: on a secondary monitor with an empty workspace and the pointer on it, `focus-workspace 2` switches the workspace of the monitor that was focused before, not the one the user is pointing at, unlike on Windows and unlike KDE's own `cycle-focus-workspace`. **Why not**: KWin may not report an empty monitor as pointed at the same way; the KDE tests do not cover it, so it may have been left out on purpose.
- **Platforms**: KDE only (the Windows and macOS daemons are not known to be affected; not checked on macOS).
- **Proposed fix**: add the preamble to the three arms, with a KDE unit test that the monitor under the pointer becomes the focused one; needs the owner to say which behaviour is intended.

## 2. Monitor::focus_workspace overwrites an existing workspace name

- **Evidence**: crates/komorebi-core/src/monitor.rs:159-165, `Monitor::focus_workspace`: on every focus it sets the workspace name to the platform's known name for that index, replacing a name the user or a rule gave the workspace. Pinned as-is by crates/komorebi-core/tests/monitor_workspaces.rs.
- **Why it may be a bug**: a workspace renamed at runtime (`workspace-name`) loses its name the next time it is focused. **Why not**: the original Windows and KDE code did exactly this (the name table is the source of truth there); it only matters where names can differ from the table.
- **Platforms**: all three (shared code, inherited unchanged from the Windows tree).
- **Proposed fix**: only fill the name when the workspace has none; a behaviour change on all three daemons, so it needs the Windows e2e (`workspace`/`commands` scenarios) before landing.

## 3. Workspace::remove_container focuses the container before the focused one, not before the removed one

- **Evidence**: crates/komorebi-core/src/workspace.rs:1011-1016, `remove_container`, calls `focus_previous_container` (lines 796-799), which focuses `focused_container_idx() - 1` (saturating) after the removal, whatever index was removed. Pinned by crates/komorebi-core/tests/workspace_containers.rs (around line 240).
- **Why it may be a bug**: closing a container that is not the focused one (a background window closes) moves the focus to a container next to the focused one rather than keeping the focused container focused. **Why not**: with the usual flow the removed container is the focused one, where both rules agree, and the original Windows code behaves identically.
- **Platforms**: all three (shared code).
- **Proposed fix**: when the removed index is below the focused one, decrement the focus by one; when it is above, leave it; when it is the focused one, focus the previous container. A behaviour change on all three, needing the Windows e2e (close scenarios) first.

## 4. Subscriber pushes have no write timeout on macOS

- **Evidence**: the macOS daemon's subscriber push (no write timeout in the subscriber send; see the macOS review notes in docs/dev/macos-handcheck.md): a stopped bar that does not read its socket wedges the daemon once the socket buffer is full.
- **Why it may be a bug**: a daemon must not hang on a client. **Why not fixed**: the Windows review showed that a write timeout can truncate a notification mid-message, so the naive fix is worse.
- **Platforms**: macOS (and the same shape on KDE, not verified).
- **Proposed fix**: write the notification from a per-subscriber thread or channel with a drop-the-subscriber policy; needs a design decision.
