#!/usr/bin/env python3
"""Generate the tables of docs/linux/dev/COMPARE.md from the source trees.

NOTE (wip-cand4): STALE. The command definitions moved to crates/komorebic-core, the daemons' message
types to crates/komorebi-protocol and the configuration types to crates/komorebi-config, so the
readers below (`Tree.commands`, `socket_messages`, `struct_keys`, ...) find nothing and the script
exits with "cannot find ^enum SubCommand". It runs only against the pre-consolidation checkouts
named in COMPARE.md. Port it to the crates before using it again, or retire it with COMPARE.md.

NOTE (layout): STALE in a second way, and NOT repaired on purpose. This file now lives in
scripts/linux/ of the consolidated repository, and the three trees it compares are komorebi-windows/,
komorebi-linux/ and komorebi-macos/ of that one repository (the old komorebi-for-kde/ and
komorebi-for-mac/ directories, the `komorebi/src` paths below, and the MONOREPO / HERE.parent
logic describe the layout before the consolidation). `HERE` below is scripts/linux/, not a tree.

komorebi for Windows (kfw), komorebi for macOS (kfm) and komorebi for KDE Plasma (kfde, this
repository) are read as they are on disk. Nothing in the tables is typed from memory, except the
small hand-written tables in the NOTES section of this file, which hold what a source file cannot
say: why a Windows command has no counterpart on KDE, and which KWin mechanism stands in for a
Win32 call. The notes are checked against the sources: a note that names a command, key, method or
event that does not exist is an error, and so is a command, key, method or event that needs a note
and has none.

This is a maintainer tool, and continuous integration cannot run it: its inputs include checkouts
of kfw and kfm, which are other repositories. Run it after a change to a command, a message, a key
of komorebi.json, an event, a method of `Backend` or one of the lists `komorebic check` uses, and
commit the result with the change.

Usage:

    python3 -I scripts/linux/compare-gen.py [--kfw DIR] [--kfm DIR] [--kfde DIR]
                                      [--update COMPARE.md | --check COMPARE.md] [--block NAME]

Without --update or --check the blocks are printed to stdout, each between the markers

    <!-- gen:NAME -->
    ...
    <!-- /gen:NAME -->

that COMPARE.md carries. --update rewrites the text between the markers of the file in place and
leaves everything outside them (the prose) alone. --check exits 1 when a block of the file is not
what would be generated now, and names the blocks; the block `sources` (the revisions read) is
skipped, because it changes whenever a source tree moves. --block prints one block.

Defaults: kfde the checkout this script is in; kfw and kfm are the repository around it and its
komorebi-for-mac/ when it is komorebi-for-kde/ of the komorebi repository, else ~/komorebi and
~/komorebi-for-mac.

The script is read-only: it reads files, and runs `git log -1` and `git status` in the checkouts of
kfw and kfm; it never asks for a remote.
"""

import argparse
import re
import subprocess
import sys
from collections import OrderedDict
from pathlib import Path

HOME = Path.home()
HERE = Path(__file__).resolve().parent.parent
# in the komorebi repository this tree is komorebi-for-kde/, next to komorebi-for-mac/, and kfw is the
# repository itself; on its own it sits beside ~/komorebi and ~/komorebi-for-mac
MONOREPO = (HERE.parent / "komorebi-for-mac").is_dir() and (HERE.parent / "komorebi" / "Cargo.toml").is_file()
DEFAULT_KFW = HERE.parent if MONOREPO else HOME / "komorebi"
DEFAULT_KFM = HERE.parent / "komorebi-for-mac" if MONOREPO else HOME / "komorebi-for-mac"
ERRORS = []


def problem(msg):
    if msg not in ERRORS:
        ERRORS.append(msg)


# --------------------------------------------------------------------------------------------
# Reading Rust
# --------------------------------------------------------------------------------------------


def block(src, header):
    """The text between the braces of the item whose header matches the regex `header`."""
    m = re.search(header, src, re.M)
    if not m:
        raise SystemExit(f"cannot find {header!r}")
    i = src.index("{", m.start())
    depth = 0
    for j in range(i, len(src)):
        if src[j] == "{":
            depth += 1
        elif src[j] == "}":
            depth -= 1
            if depth == 0:
                return src[i + 1 : j]
    raise SystemExit(f"unbalanced braces after {header!r}")


def kebab(name):
    """clap's kebab-case of a variant name."""
    s = re.sub(r"(?<=[a-z0-9])([A-Z])", r"-\1", name)
    s = re.sub(r"(?<=[A-Z])([A-Z][a-z])", r"-\1", s)
    return s.lower()


class Item:
    """An enum variant or struct field, with the doc and attributes above it."""

    def __init__(self, name, doc, attrs, commented=False):
        self.name = name
        self.doc = list(doc)
        self.attrs = list(attrs)
        self.commented = commented
        self.json = name
        self.json_aliases = []

    @property
    def summary(self):
        return " ".join(self.doc).strip()

    @property
    def first_sentence(self):
        text = self.summary
        m = re.match(r"(.+?[.!?])(\s|$)", text)
        return (m.group(1) if m else text).strip()

    @property
    def aliases(self):
        out = []
        for a in self.attrs:
            out += re.findall(r'alias\s*=\s*"([^"]+)"', a)
        return out

    @property
    def hidden(self):
        return any("hide = true" in a for a in self.attrs)


def variants(body):
    """The variants of an enum body: the live ones, and the ones commented out with `//`."""
    pad = "    "
    live, dead = [], []
    doc, attrs, cdoc, cattrs = [], [], [], []
    for line in body.split("\n"):
        if line.startswith(pad + "///"):
            doc.append(line[7:].strip())
            continue
        if line.startswith(pad + "#["):
            attrs.append(line.strip())
            continue
        m = re.match(pad + r"([A-Z]\w*)\s*(?:[(,{=]|$)", line)
        if m:
            live.append(Item(m.group(1), doc, attrs))
            doc, attrs, cdoc, cattrs = [], [], [], []
            continue
        c = re.match(pad + r"//\s*///\s?(.*)", line)
        if c:
            cdoc.append(c.group(1).strip())
            continue
        c = re.match(pad + r"//\s*#\[(.*)", line)
        if c:
            cattrs.append("#[" + c.group(1).strip())
            continue
        m = re.match(pad + r"//\s*([A-Z]\w*)\s*(?:[(,{=]|$)", line)
        if m:
            dead.append(Item(m.group(1), cdoc, cattrs, commented=True))
            cdoc, cattrs = [], []
            continue
        if line.strip() and not line.strip().startswith("//"):
            doc, attrs = [], []
    return live, dead


def fields(body):
    """The `pub name: Type` fields of a struct body, live and commented out, with serde names."""
    out, dead = [], []
    doc, attrs, cdoc = [], [], []
    for line in body.split("\n"):
        if line.startswith("    ///"):
            doc.append(line[7:].strip())
            continue
        if line.startswith("    #["):
            attrs.append(line.strip())
            continue
        m = re.match(r"    pub (\w+): ", line)
        if m:
            it = Item(m.group(1), doc, attrs)
            for a in attrs:
                r = re.search(r'rename\s*=\s*"([^"]+)"', a)
                if r:
                    it.json = r.group(1)
                it.json_aliases += re.findall(r'alias\s*=\s*"([^"]+)"', a)
            out.append(it)
            doc, attrs, cdoc = [], [], []
            continue
        m = re.match(r"    //\s*pub (\w+): ", line)
        if m:
            dead.append(Item(m.group(1), cdoc, [], commented=True))
            cdoc = []
            continue
        c = re.match(r"    //\s*///\s?(.*)", line)
        if c:
            cdoc.append(c.group(1).strip())
            continue
        if line.strip() and not line.strip().startswith("//"):
            doc, attrs = [], []
    return out, dead


def clap_flags(body):
    """The long flags of a clap argument struct body (`config: Option<PathBuf>` is `--config`).

    Arguments that follow a `--` (`#[clap(last = true)]`) are not flags."""
    names = []
    attrs = []
    for line in body.split("\n"):
        if re.match(r"    #\[", line):
            attrs.append(line)
            continue
        m = re.match(r"    (\w+): ", line)
        if m:
            if not any("last = true" in a for a in attrs):
                names.append("--" + m.group(1).replace("_", "-"))
            attrs = []
        elif not line.strip().startswith("//"):
            attrs = []
    return names


def string_list(src, const):
    m = re.search(rf"const {const}: &\[&str\] = &\[(.*?)\];", src, re.S)
    if not m:
        raise SystemExit(f"cannot find const {const}")
    return re.findall(r'"([^"]+)"', m.group(1))


# --------------------------------------------------------------------------------------------
# The trees
# --------------------------------------------------------------------------------------------


class Tree:
    def __init__(self, label, root):
        self.label = label
        self.root = Path(root).expanduser()
        if not self.root.is_dir():
            raise SystemExit(f"{label}: {self.root} is not a directory")
        self._cache = {}

    def src(self, rel):
        if rel not in self._cache:
            self._cache[rel] = (self.root / rel).read_text()
        return self._cache[rel]

    def all_rust(self):
        if "all_rust" not in self._cache:
            self._cache["all_rust"] = "\n".join(
                p.read_text() for p in sorted((self.root / "komorebi" / "src").rglob("*.rs"))
            )
        return self._cache["all_rust"]

    def has_fn(self, name):
        return re.search(rf"\bfn {re.escape(name)}\b", self.all_rust()) is not None

    def commands(self):
        body = block(self.src("komorebic/src/main.rs"), r"^enum SubCommand\b")
        live, dead = variants(body)
        cmds = OrderedDict((kebab(v.name), v) for v in live)
        gone = OrderedDict((kebab(v.name), v) for v in dead if kebab(v.name) not in cmds)
        return cmds, gone

    def clap_struct(self, name):
        try:
            return clap_flags(block(self.src("komorebic/src/main.rs"), rf"^struct {name}\b"))
        except SystemExit:
            return None

    def socket_messages(self):
        body = block(self.src("komorebi/src/core/mod.rs"), r"^pub enum SocketMessage\b")
        live, dead = variants(body)
        return OrderedDict((v.name, v) for v in live), OrderedDict((v.name, v) for v in dead)

    def struct_keys(self, struct, rel="komorebi/src/static_config.rs"):
        body = block(self.src(rel), rf"^pub struct {struct}\b")
        live, dead = fields(body)
        return OrderedDict((f.json, f) for f in live), OrderedDict((f.json, f) for f in dead)

    def events(self):
        body = block(self.src("komorebi/src/window_manager_event.rs"), r"^pub enum WindowManagerEvent\b")
        live, _ = variants(body)
        return OrderedDict((v.name, v) for v in live)

    def enum_count(self, rel, name):
        body = block(self.src(rel), rf"^pub enum {name}\b")
        live, _ = variants(body)
        return len(live)

    def cargo_dep(self, name):
        """(kind, value) of a dependency, from the root or the komorebi manifest: ("git", rev),
        or ("path", the directory, relative to the tree)."""
        for rel in ("Cargo.toml", "komorebi/Cargo.toml"):
            text = self.src(rel)
            m = re.search(rf"^{re.escape(name)}\s*=\s*\{{(.*?)\}}", text, re.M | re.S)
            if not m:
                continue
            spec = m.group(1)
            g = re.search(r'git\s*=\s*"[^"]+"', spec)
            r = re.search(r'rev\s*=\s*"([0-9a-f]+)"', spec)
            if g and r:
                return ("git", r.group(1))
            p = re.search(r'path\s*=\s*"([^"]+)"', spec)
            if p:
                base = Path(rel).parent
                return ("path", str((base / p.group(1))))
        return None

    def git(self, *args):
        try:
            return subprocess.run(
                ["git", "-C", str(self.root), *args], capture_output=True, text=True, timeout=30
            ).stdout.strip()
        except Exception:
            return ""

    def revision(self):
        head = self.git("log", "-1", "--format=%h, %ad", "--date=short")
        if not head:
            return "not a git checkout"
        dirty = self.git("status", "--porcelain", "--untracked-files=no")
        return head + (", plus uncommitted changes" if dirty else "")


def layouts_dir(tree):
    """The directory of komorebi-layouts a tree builds, or None when it cannot be found."""
    dep = tree.cargo_dep("komorebi-layouts")
    if dep is None:
        return None
    kind, value = dep
    if kind == "path":
        return (tree.root / value / "src"), f"`komorebi-layouts` in the tree"
    for checkout in (HOME / ".cargo" / "git" / "checkouts").glob("komorebi-*"):
        for rev in checkout.iterdir():
            if value.startswith(rev.name) or rev.name.startswith(value[:7]):
                d = rev / "komorebi-layouts" / "src"
                if d.is_dir():
                    return d, f"`komorebi-layouts` at git rev `{value[:7]}`"
    return None


# --------------------------------------------------------------------------------------------
# NOTES: what the sources cannot say. Each entry is checked against them.
# --------------------------------------------------------------------------------------------

# Commands (kebab-case) of kfw or kfm that kfde does not have, with the group they belong to and
# the reason. A reason says what the command is for, which the sources of kfw and kfm show; where
# DESIGN.md records why kfde does something else, the reason says what kfde does.
NOT_ON_KDE = {
    # borders: the compositor draws them
    "border-style": ("borders", "How a Win32 border window is drawn. The compositor draws the border."),
    "border-implementation": ("borders", "How a Win32 border window is drawn. The compositor draws the border."),
    "invisible-borders": ("borders", "The size of the invisible frame Windows adds around a window."),
    "identify-border-overflow-application": ("borders", "A border window that overflows is a Win32 problem. The compositor draws the border."),
    # stacks, titles, hiding
    "stackbar-mode": ("stackbar", "komorebi draws the tab bar of a stack on Windows. KDE has no counterpart; a stack is cycled with `cycle-stack` and `focus-stack-window`."),
    "window-hiding-behaviour": ("hiding", "One native mechanism hides windows, so there is nothing to choose."),
    "remove-title-bar": ("title bars", "A window without a title bar has no shadow in KWin, and so no border."),
    "toggle-title-bars": ("title bars", "A window without a title bar has no shadow in KWin, and so no border."),
    # Win32 quirks
    "identify-layered-application": ("Win32 quirks", "A Win32 window style. There is no counterpart in a Wayland compositor."),
    "identify-object-name-change-application": ("Win32 quirks", "A Win32 event some applications send on launch."),
    "identify-tray-application": ("Win32 quirks", "A Win32 notion: an application that closes to the system tray."),
    "force-focus": ("Win32 quirks", "Works around the rules Windows has for taking the foreground."),
    "alt-focus-hack": ("Win32 quirks", "Works around the rules Windows has for taking the foreground."),
    "restore-windows": ("hiding", "Debugging command for windows komorebi hid itself. On KDE `stop` and the next start send parked windows home."),
    # AutoHotkey, PowerShell, whkd, the bar, helper programs
    "ahk-app-specific-configuration": ("AutoHotkey and PowerShell", "Writes application rules for `komorebi.ahk`."),
    "pwsh-app-specific-configuration": ("AutoHotkey and PowerShell", "Writes application rules for a PowerShell script."),
    "complete-configuration": ("AutoHotkey and PowerShell", "Part of the legacy `komorebi.ahk` and `komorebi.ps1` configurations."),
    "reload-configuration": ("AutoHotkey and PowerShell", "Part of the legacy `komorebi.ahk` and `komorebi.ps1` configurations."),
    "watch-configuration": ("AutoHotkey and PowerShell", "Part of the legacy `komorebi.ahk` and `komorebi.ps1` configurations. `komorebi.json` is watched always."),
    "convert-app-specific-configuration": ("upstream applications file", "Maintains the YAML file of the Windows applications list."),
    "format-app-specific-configuration": ("upstream applications file", "Maintains the YAML file of the Windows applications list."),
    "fetch-app-specific-configuration": ("upstream applications file", "Not supported yet: there is no applications file for KDE upstream, so there is nothing to fetch."),
    "gui": ("Windows tooling", "Launches `komorebi-gui`, which is not ported."),
    "toggle-shortcuts": ("Windows tooling", "Toggles `komorebi-shortcuts`, which is not ported."),
    "bar-configuration": ("Windows tooling", "`komorebi-bar` is not ported: the Plasma panel is used, and the work area accounts for it."),
    "whkdrc": ("Windows tooling", "Shows the path of the whkd file. The daemon reads key binding files itself."),
    "kill": ("Windows tooling", "Stops the helper programs `komorebic start` launches on Windows (whkd, AutoHotkey, the bar, masir). It launches none on KDE."),
    # licensing
    "license": ("license", "No license check was ported."),
    "splash": ("license", "No splash screen was ported."),
    # custom layouts
    "load-custom-layout": ("custom layouts", "Not supported yet."),
    "workspace-custom-layout": ("custom layouts", "Not supported yet."),
    "workspace-custom-layout-rule": ("custom layouts", "Not supported yet."),
    "named-workspace-custom-layout": ("custom layouts", "Not supported yet."),
    "named-workspace-custom-layout-rule": ("custom layouts", "Not supported yet."),
    # monitors, pipes
    "monitor-index-preference": ("monitors", "Identifies a monitor by its size. kfde has `display-index-preference`, which names the display, as kfm does."),
    "subscribe-pipe": ("named pipes", "A named pipe is a Windows facility. Subscribers use a Unix domain socket."),
    "unsubscribe-pipe": ("named pipes", "A named pipe is a Windows facility. Subscribers use a Unix domain socket."),
    # kfm only
    "center-focus": ("kfm scrolling strip", "Belongs to the scrolling strip of kfm (`komorebi-scrolling`). The Scrolling layout of kfde is the one of `komorebi-layouts`."),
    "consume": ("kfm scrolling strip", "Belongs to the scrolling strip of kfm (`komorebi-scrolling`)."),
    "expel": ("kfm scrolling strip", "Belongs to the scrolling strip of kfm (`komorebi-scrolling`)."),
    "scrolling-layout-full-width": ("kfm scrolling strip", "Belongs to the scrolling strip of kfm (`komorebi-scrolling`)."),
    "toggle-column-stack": ("kfm scrolling strip", "Belongs to the scrolling strip of kfm (`komorebi-scrolling`)."),
    "permissions": ("macOS", "Shows the System Settings dialogs macOS asks for. KDE asks for none."),
}

# Config keys that kfw (or kfm) has and kfde does not read, with the reason. A key is checked in
# both directions: it must be in a struct of kfw or kfm and not in kfde.
NOT_READ = {
    "bar_configurations": "`komorebi-bar` is not ported.",
    "border_implementation": "How a Win32 border window is drawn. The compositor draws the border.",
    "border_overflow_applications": "How a Win32 border window is drawn. The compositor draws the border.",
    "border_style": "How a Win32 border window is drawn. The compositor draws the border.",
    "border_z_order": "How a Win32 border window is drawn. The compositor draws the border.",
    "invisible_borders": "The size of the invisible frame Windows adds around a window.",
    "layered_applications": "A Win32 window style. There is no counterpart in a Wayland compositor.",
    "minimum_window_height": "Marked DISCOURAGED in komorebi for Windows. Commented out in kfde.",
    "minimum_window_width": "Marked DISCOURAGED in komorebi for Windows. Commented out in kfde.",
    "monitor_index_preferences": "Identifies a monitor by its size. kfde reads `display_index_preferences`, which names the display, as kfm does.",
    "object_name_change_applications": "A Win32 event some applications send on launch.",
    "object_name_change_title_ignore_list": "A Win32 event some applications send on launch.",
    "remove_titlebar_applications": "A window without a title bar has no shadow in KWin, and so no border.",
    "slow_application_compensation_time": "A Win32 workaround for applications that are slow to move.",
    "slow_application_identifiers": "A Win32 workaround for applications that are slow to move.",
    "stackbar": "KDE has no counterpart of the tab bar komorebi draws on Windows.",
    "tray_and_multi_window_applications": "A Win32 notion: an application that closes to the system tray.",
    "window_handling_behaviour": "A Win32 quirk of how windows are moved.",
    "window_hiding_behaviour": "One native mechanism hides windows, so there is nothing to choose.",
    "custom_layout": "Custom layouts are not supported yet.",
    "custom_layout_rules": "Custom layouts are not supported yet.",
    "scrolling": "The `ScrollingConfig` of the strip in kfm (`komorebi-scrolling`). kfde has the Scrolling layout of `komorebi-layouts`, tuned with `layout_options`.",
}

# kfde's WindowManagerEvent variants against the others'. A cell is the variants (of that tree)
# that play the same part; None is "no counterpart". `source` is the KWin signal. Every name is
# checked against the tree it is said to be in.
EVENTS = [
    # (kfde, source, kfw, kfm)
    ("Show", "`workspace.windowAdded`", ["Show", "Uncloak"], ["Show"]),
    ("Destroy", "`workspace.windowRemoved`", ["Destroy"], ["Destroy"]),
    ("FocusChange", "`workspace.windowActivated`", ["FocusChange", "Raise"], ["FocusChange"]),
    ("Minimize", "`window.minimizedChanged`, to true", ["Minimize"], ["Minimize"]),
    ("Restore", "`window.minimizedChanged`, to false", None, ["Restore"]),
    ("TitleUpdate", "`window.captionChanged`", ["TitleUpdate"], None),
    ("Manage", "komorebi itself (`manage`, a rule)", ["Manage"], ["Manage"]),
    ("Unmanage", "komorebi itself (`unmanage`, a rule)", ["Unmanage"], ["Unmanage"]),
    ("MoveStart", "`window.interactiveMoveResizeStarted`, for a move", ["MoveResizeStart"], ["MoveStart"]),
    ("MoveEnd", "`window.interactiveMoveResizeFinished`, after a move", ["MoveResizeEnd"], ["MoveEnd"]),
    ("ResizeStart", "`window.interactiveMoveResizeStarted`, for a resize", ["MoveResizeStart"], ["ResizeStart"]),
    ("ResizeEnd", "`window.interactiveMoveResizeFinished`, after a resize", ["MoveResizeEnd"], ["ResizeEnd"]),
    ("ResizeStep", "`window.interactiveMoveResizeStepped`", None, None),
    ("WorkspaceChange", "`workspace.currentDesktopChanged`", None, ["SpaceChange"]),
    ("WorkspaceMove", "`window.desktopsChanged`", None, None),
    ("OutputChange", "`window.outputChanged`", None, None),
    ("MaximizeChange", "`window.maximizedChanged`", None, None),
    ("FullscreenChange", "`window.fullScreenChanged`", None, None),
]

# Variants of the other trees that kfde has no event for, and why.
EVENTS_UNMAPPED = {
    "kfw": {
        "Hide": "Win32 hides and cloaks windows; kfde has no counterpart because KWin shows and hides windows by desktop, and says so with `WorkspaceChange`.",
        "Cloak": "Win32 hides and cloaks windows; kfde has no counterpart because KWin shows and hides windows by desktop, and says so with `WorkspaceChange`.",
        "MouseCapture": "Windows tells komorebi that a mouse button went down on a window. KWin reports the start of a move or resize instead.",
    },
    "kfm": {
        "ScreenLock": "KDE locks and unlocks the screen with `org.freedesktop.ScreenSaver`; komorebi hears it in `kwin/power.rs` and reconciles, with no event of this kind.",
        "ScreenUnlock": "KDE locks and unlocks the screen with `org.freedesktop.ScreenSaver`; komorebi hears it in `kwin/power.rs` and reconciles, with no event of this kind.",
    },
}

# The Backend trait of kfde against the calls the other two make. The KWin cell names the
# mechanism and is checked: every backtick-quoted token must occur in the sources of the KWin
# backend. The kfw and kfm cells name functions, which are checked to exist in their trees.
BACKEND = OrderedDict(
    [
        ("windows", ("`workspace.windowList()` once, then the events of the bridge", ["enum_windows"], ["window_elements"])),
        ("window", ("the table the bridge keeps, fed by events", ["window_rect", "exe_path"], ["window_by_title"])),
        ("foreground_window", ("`workspace.activeWindow`", ["foreground_window"], ["foreground_window"])),
        ("monitors", ("`workspace.screens` and `clientArea(KWin.PlacementArea, ...)`, which leaves out the panel", ["load_monitor_information"], ["load_monitor_information"])),
        ("cursor_pos", ("`workspace.cursorPos`", ["cursor_pos"], ["cursor_pos"])),
        ("drag_origin", ("the frame the bridge saw at `interactiveMoveResizeStarted`", None, None)),
        ("drag_end", ("the frame the bridge saw at the last `interactiveMoveResizeStepped`", None, None)),
        ("set_rect", ("`frameGeometry`, and a second request for the position once the size settles", ["position_window"], ["set_position"])),
        ("focus", ("`workspace.activeWindow = w`", ["raise_and_focus_window"], ["focus"])),
        ("raise", ("`workspace.raiseWindow(w)`", ["raise_window"], ["raise"])),
        ("hide", ("moves the window to the parking desktop (`desktops`)", ["hide_window"], ["hide"])),
        ("restore", ("moves the window back from the parking desktop (`desktops`)", ["restore_window"], ["restore"])),
        ("minimize", ("`minimized = true`", ["minimize_window"], ["minimize"])),
        ("unminimize", ("`minimized = false`", ["restore_window"], ["unminimize"])),
        ("close", ("`closeWindow()`", ["close_window"], ["close"])),
        ("center_cursor_in", ("the \"Move Mouse to Focus\" action (`MoveMouseToFocus`)", ["center_cursor_in_rect"], ["center_cursor_in_rect"])),
        ("ensure_workspaces", ("`createDesktop` on `org.kde.KWin.VirtualDesktopManager`", None, None)),
        ("switch_workspace", ("`setCurrentDesktopForScreen`", None, None)),
        ("set_window_workspace", ("`desktops` of the window", None, None)),
        ("window_workspace", ("`desktops` of the window", None, None)),
        ("current_workspaces", ("`currentDesktopForScreen`", None, None)),
        ("apply_look", ("writes the effects' groups of `kwinrc`, `loadEffect` and `reconfigureEffect`", None, None)),
        ("set_border_colours", ("`org.komorebi.BorderColours1`, which only komorebi's build of the border effect has", None, None)),
        ("maximize", ("`setMaximize`", ["maximize_window"], None)),
        ("set_opacity", ("`opacity`", ["set_transparent"], None)),
        ("set_focus_follows_mouse", ("the `FocusPolicy` of `kwinrc`, then `reconfigure`", ["enable_focus_follows_mouse"], None)),
        ("set_wallpaper", ("`org.kde.PlasmaShell` `setWallpaper`", ["set_wallpaper"], ["set_wallpaper"])),
    ]
)
# Provided methods of the trait that every backend gets, and so need no row.
BACKEND_PROVIDED = {"shutdown", "is_window", "window_rect", "monitor_from_point"}

# What is native, in prose-sized cells. The sources of every cell are checked by `needs`: pairs of
# (tree, file, pattern) that must match for the row to be true.
NATIVE = [
    {
        "what": "Workspaces",
        "kfw": "komorebi's own workspaces; the windows of the other workspaces are hidden",
        "kfm": "komorebi's own workspaces; the windows of the other workspaces are hidden",
        "kfde": "KWin's virtual desktops, one set per output (Plasma 6.8): workspace `i` is desktop `i` on every monitor",
        "needs": [("kfde", "komorebi/src/kwin/mod.rs", r"PerOutputVirtualDesktops"), ("kfw", "komorebi/src/workspace.rs", r"fn new_idx_for_direction|pub struct Workspace")],
    },
    {
        "what": "Hiding windows (a stack, a monocle)",
        "kfw": "`window_hiding_behaviour`: cloak, hide or minimize",
        "kfm": "moves the window to a corner of the screen",
        "kfde": "a virtual desktop nobody looks at (`komorebi-park`)",
        "needs": [("kfde", "komorebi/src/kwin/state.rs", r"komorebi-park"), ("kfw", "komorebi/src/core/mod.rs", r"pub enum HidingBehaviour"), ("kfm", "komorebi/src/window.rs", r"fn hide_on")],
    },
    {
        "what": "Borders",
        "kfw": "a topmost tool window per border, drawn by komorebi with Direct2D",
        "kfm": "an `NSWindow` per border, drawn by komorebi",
        "kfde": "the KDE Rounded Corners effect, with komorebi's patch for one color per kind of window",
        "needs": [("kfde", "komorebi/src/kwin/effects.rs", r"kwin4_effect_shapecorners"), ("kfw", "komorebi/src/border_manager/border.rs", r"Direct2D"), ("kfw", "komorebi/src/windows_api.rs", r"fn create_border_window"), ("kfm", "komorebi/src/border_manager/ns_window.rs", r"NSWindow|NsWindow")],
    },
    {
        "what": "Animation",
        "kfw": "komorebi moves the window step by step",
        "kfm": "komorebi moves the window step by step",
        "kfde": "the Geometry Change effect, with komorebi's patch for the easing curve",
        "needs": [("kfde", "komorebi/src/kwin/effects.rs", r"kwin4_effect_geometry_change"), ("kfw", "komorebi/src/animation/mod.rs", r"fps|FPS"), ("kfm", "komorebi/src/animation/mod.rs", r"fps|FPS")],
    },
    {
        "what": "Transparency of unfocused windows",
        "kfw": "`SetLayeredWindowAttributes`, called by komorebi",
        "kfm": "no key or command",
        "kfde": "KWin's window opacity, through the same patched effect as the borders",
        "needs": [("kfde", "komorebi/src/kwin/bridge.js", r"\.opacity"), ("kfw", "komorebi/src/windows_api.rs", r"fn set_transparent")],
    },
    {
        "what": "Focus follows mouse",
        "kfw": "the Windows setting, or komorebi's own implementation (`focus_follows_mouse`)",
        "kfm": "no key or command",
        "kfde": "KWin's focus policy; the user's policy is put back when it is turned off or komorebi exits",
        "needs": [("kfde", "komorebi/src/kwin/mod.rs", r"FocusPolicy"), ("kfw", "komorebi/src/windows_api.rs", r"fn enable_focus_follows_mouse")],
    },
    {
        "what": "Maximize",
        "kfw": "`toggle-maximize`: the Win32 maximize",
        "kfm": "no key or command",
        "kfde": "`toggle-maximize`: KWin's maximize, which respects the panel. A window the user maximizes is taken out of the layout until restored",
        "needs": [("kfde", "komorebi/src/kwin/bridge.js", r"setMaximize"), ("kfw", "komorebi/src/windows_api.rs", r"fn maximize_window")],
    },
    {
        "what": "Fullscreen windows",
        "kfw": "no handling of its own in the sources",
        "kfm": "no handling of its own, except that the reaper keeps a window that fails to move",
        "kfde": "left alone, and put back in its tile when it leaves fullscreen",
        "needs": [("kfde", "komorebi/src/window_manager_event.rs", r"FullscreenChange"), ("kfm", "komorebi/src/reaper.rs", r"AXFullScreen")],
        "absent": [("kfw", r"(?i)fullscreen|full_screen")],
    },
    {
        "what": "Wallpapers",
        "kfw": "the desktop wallpaper of each monitor",
        "kfm": "the desktop picture of each display",
        "kfde": "the Plasma shell, asked over D-Bus, one wallpaper per screen",
        "needs": [("kfde", "komorebi/src/kwin/wallpaper.rs", r"setWallpaper"), ("kfw", "komorebi/src/windows_api.rs", r"fn set_wallpaper"), ("kfm", "komorebi/src/macos_api.rs", r"fn set_wallpaper")],
    },
    {
        "what": "Hotkeys",
        "kfw": "a separate program: `whkd` (or AutoHotkey)",
        "kfm": "a separate program: `skhd`",
        "kfde": "KDE global shortcuts: `.desktop` files for plain bindings, and the daemon through KGlobalAccel for modes, chords and scopes",
        "needs": [("kfde", "komorebic/src/main.rs", r"ExportKdeShortcuts")],
    },
    {
        "what": "Monitors added, removed or changed",
        "kfw": "a reconciliator thread, fed by `WM_DISPLAYCHANGE` in a hidden window",
        "kfm": "a reconciliator thread, fed by the display reconfiguration callback",
        "kfde": "the same thread, fed by the script (when KWin's list of outputs changes) and by `kwin/power.rs`; `reconcile.rs` follows the windows the compositor moved",
        "needs": [("kfde", "komorebi/src/reconcile.rs", r"evacuate_monitor"), ("kfde", "komorebi/src/monitor_reconciliator.rs", r"std::thread::spawn"), ("kfw", "komorebi/src/monitor_reconciliator/hidden.rs", r"WM_DISPLAYCHANGE"), ("kfm", "komorebi/src/display_reconfiguration_listener.rs", r"CGDisplayRegisterReconfigurationCallback")],
    },
    {
        "what": "Sleep, wake and lock",
        "kfw": "power setting and session notifications (`register_power_setting_notification`, `wts_register_session_notification`)",
        "kfm": "`ScreenLock` and `ScreenUnlock` events",
        "kfde": "`PrepareForSleep` of logind and `ActiveChanged` of the KDE screen saver, on D-Bus",
        "needs": [("kfde", "komorebi/src/kwin/power.rs", r"PrepareForSleep"), ("kfw", "komorebi/src/windows_api.rs", r"fn wts_register_session_notification"), ("kfm", "komorebi/src/window_manager_event.rs", r"ScreenLock")],
    },
    {
        "what": "Reloading the configuration",
        "kfw": "`watch-configuration` is about `komorebi.ahk` and `komorebi.ps1`; `komorebi.json` is watched",
        "kfm": "`komorebi.json` is watched",
        "kfde": "`komorebi.json` is watched",
        "needs": [("kfde", "komorebi/src/static_config.rs", r"Hotwatch|hotwatch"), ("kfm", "komorebi/src/static_config.rs", r"Hotwatch|hotwatch|watch")],
    },
]

# Keys that kfde reads and that mean something else on KDE, or less. `struct` and `key` are checked
# against kfde; the text is from PLATFORM.md and from the code.
KEY_DIFFS = [
    ("StaticConfig", "border_width", "At most 20 px: the effect draws the ring in the window's shadow, and a window without a shadow has none. While borders are drawn, the layout leaves `border_width` plus `border_offset` on each side of a window, also past 20."),
    ("StaticConfig", "border_colours", "Six colors, one for each kind of window, when komorebi's build of the border effect is installed. With the stock effect only `single` and `unfocused` take effect, and komorebi says so once in its log."),
    ("StaticConfig", "animation", "`style` is the Qt easing curve of that name (`EaseInOutSine` is `InOutSine`) and a custom cubic Bezier curve is an ease out, when komorebi's build of the Geometry Change effect is installed; the stock effect always eases out exponentially. `fps` is read and ignored: the compositor animates at the refresh rate of the screen."),
    ("StaticConfig", "focus_follows_mouse", "Both implementations kfw names (`Windows` and `Komorebi`) ask KWin for the same thing: its focus policy, put back as the user had it when it is turned off or komorebi exits."),
    ("StaticConfig", "transparency", "KWin's window opacity, set for the unfocused windows of each monitor's shown workspace by kfw's rules (the focused window and a monocled one stay opaque)."),
    ("StaticConfig", "theme", "A theme by name or a palette of your own gives the six border colors, and the palette of a wallpaper takes over from it when its workspace is shown."),
    ("WorkspaceConfig", "wallpaper", "Set through the Plasma shell, which keeps one wallpaper for each screen and not for each desktop, so komorebi changes it as the workspace in front changes. The picture fills the screen."),
    ("MonitorConfig", "workspaces", "Workspace `i` is KWin's desktop `i` on every monitor, so every monitor has as many desktops as the largest workspace count, and where the names differ the desktop takes the first monitor's."),
    ("StaticConfig", "app_specific_configuration_path", "Read as in kfm and kfw. There is no applications file for KDE upstream, so komorebi has none to download."),
]

# Statements the prose of COMPARE.md makes about the generated tables. When one stops being true the
# prose has to be read again.
def _assumptions(f):
    kde_only_keys = []
    for st, _ in STRUCTS:
        s = struct_sets(f, st)
        kde_only_keys += set(s["kfde"][0]) - set(s["kfw"][0]) - set(s["kfm"][0])
    sm = {k: set(t.socket_messages()[0]) for k, t in f.trees.items()}
    d = f.names("kfde")
    renamed = [n for k in ("kfw", "kfm") for n in f.names(k) - d for v in f.cmds["kfde"][0].values() if n in v.aliases]
    start = f.kde.clap_struct("Start") or []
    refs = {}
    for k in ("kfm", "kfde"):
        refs[k] = f.trees[k].cargo_dep("komorebi-layouts")
    layouts = {k: layouts_dir(t) for k, t in f.trees.items()}
    def same_layouts(a, b):
        if not layouts[a] or not layouts[b]:
            return False
        da, db = layouts[a][0], layouts[b][0]
        return all((db / p.name).exists() and (db / p.name).read_text() == p.read_text() for p in da.glob("*.rs"))
    only_kde_messages = sm["kfde"] - sm["kfw"] - sm["kfm"]
    return [
        ("no key of komorebi.json is read by kfde alone", not kde_only_keys),
        ("the only `SocketMessage`s that neither kfw nor kfm has are `HotkeysState` and `ReloadHotkeys`", only_kde_messages == {"HotkeysState", "ReloadHotkeys"}),
        ("no command of kfw or kfm is kept in kfde under another name", not renamed),
        ("`komorebic start` of kfde has none of the flags that launch helper programs (`--whkd`, `--ahk`, `--bar`, `--masir`)", not ({"--whkd", "--ahk", "--bar", "--masir"} & set(start))),
        ("kfm has no `check` command", "check" not in f.names("kfm")),
        ("the crates `komorebi-layouts` of kfw and kfm have the same source files", same_layouts("kfw", "kfm")),
        ("kfde builds `komorebi-layouts` from the same git revision as kfm", refs["kfm"] is not None and refs["kfm"] == refs["kfde"]),
        ("the crates `komorebi-layouts` of kfw and kfde have the same source files", same_layouts("kfw", "kfde")),
        ("kfde has the commands `hotkeys` and `export-kde-shortcuts`", {"hotkeys", "export-kde-shortcuts"} <= d),
    ]


# --------------------------------------------------------------------------------------------
# Markdown helpers
# --------------------------------------------------------------------------------------------


def esc(text):
    return text.replace("|", "\\|").replace("\n", " ")


def table(header, rows, align=None):
    out = ["| " + " | ".join(header) + " |"]
    out.append("|" + "|".join("---" for _ in header) + "|")
    for r in rows:
        out.append("| " + " | ".join(esc(str(c)) for c in r) + " |")
    return "\n".join(out)


def code_list(names):
    return ", ".join(f"`{n}`" for n in names)


# --------------------------------------------------------------------------------------------
# The blocks
# --------------------------------------------------------------------------------------------


class Facts:
    def __init__(self, args):
        self.kfw = Tree("kfw", args.kfw)
        self.kfm = Tree("kfm", args.kfm)
        self.kde = Tree("kfde", args.kfde)
        self.trees = OrderedDict([("kfw", self.kfw), ("kfm", self.kfm), ("kfde", self.kde)])
        self.cmds = {k: t.commands() for k, t in self.trees.items()}

    def names(self, key):
        return set(self.cmds[key][0])


def tilde(path):
    p = str(path)
    h = str(HOME)
    return "~" + p[len(h):] if p.startswith(h) else p


def block_sources(f, args):
    rows = [
        ("kfw", "komorebi for Windows, the upstream", f"`{tilde(f.kfw.root)}`", f.kfw.revision()),
        ("kfm", "komorebi for macOS", f"`{tilde(f.kfm.root)}`", f.kfm.revision()),
        ("kfde", "this project", "this repository", "the commit that has this document"),
    ]
    return table(["Name", "What", "Read from", "Revision"], rows)


def block_commands_counts(f, args):
    w, m, d = f.names("kfw"), f.names("kfm"), f.names("kfde")
    renamed = 0
    for key in ("kfw", "kfm"):
        for n in f.names(key) - d:
            for v in f.cmds["kfde"][0].values():
                if n in v.aliases:
                    renamed += 1
    rows = [
        ("Commands `komorebic` has", len(w), len(m), len(d)),
        ("In all three", len(w & m & d), len(w & m & d), len(w & m & d)),
        ("Also in kfw, not in kfm", len((w & d) - m), "-", len((w & d) - m)),
        ("Also in kfm, not in kfw", "-", len((m & d) - w), len((m & d) - w)),
        ("Only in kfde", "-", "-", len(d - w - m)),
        ("In kfw or kfm, not in kfde", len(w - d), len(m - d), "-"),
        ("Same command under another name", "-", "-", renamed),
    ]
    return table(["", "kfw", "kfm", "kfde"], rows)


def block_commands_all3(f, args):
    names = sorted(f.names("kfw") & f.names("kfm") & f.names("kfde"))
    return "`" + "`, `".join(names) + "`"


def block_commands_adopted(f, args):
    names = sorted((f.names("kfw") & f.names("kfde")) - f.names("kfm"))
    rows = []
    for n in names:
        w = f.cmds["kfw"][0][n]
        rows.append((f"`{n}`", w.first_sentence or "(hidden)"))
    return table(["Command", "What the help of kfw says"], rows)


def block_commands_kfm(f, args):
    names = sorted((f.names("kfm") & f.names("kfde")) - f.names("kfw"))
    rows = []
    for n in names:
        rows.append((f"`{n}`", f.cmds["kfm"][0][n].first_sentence))
    return table(["Command", "What it does"], rows)


def block_commands_kde(f, args):
    names = sorted(f.names("kfde") - f.names("kfw") - f.names("kfm"))
    rows = [(f"`{n}`", f.cmds["kfde"][0][n].first_sentence) for n in names]
    return table(["Command", "What it does"], rows)


def block_commands_missing(f, args):
    w, m, d = f.names("kfw"), f.names("kfm"), f.names("kfde")
    missing = sorted((w | m) - d)
    rows = []
    for n in missing:
        where = ("kfw " if n in w else "") + ("kfm" if n in m else "")
        note = NOT_ON_KDE.get(n)
        if note is None:
            problem(f"commands: `{n}` is in {where.strip()} and not in kfde, and has no entry in NOT_ON_KDE")
            note = ("?", "no reason recorded")
        rows.append((note[0], f"`{n}`", where.strip().replace(" ", " and "), note[1]))
    stale = set(NOT_ON_KDE) - set(missing)
    for n in sorted(stale):
        problem(f"commands: NOT_ON_KDE names `{n}`, which is not missing from kfde (or no tree has it)")
    rows.sort(key=lambda r: (r[0], r[1]))
    return table(["Group", "Command", "In", "Why kfde has no counterpart"], rows)


def block_commands_flags(f, args):
    rows = []
    for struct, label in (("Start", "start"), ("Stop", "stop"), ("Check", "check")):
        flags = {k: t.clap_struct(struct) for k, t in f.trees.items()}
        allflags = sorted({x for v in flags.values() if v for x in v})
        for flag in allflags:
            row = [f"`{label}`", f"`{flag}`"]
            for k in ("kfw", "kfm", "kfde"):
                if flags[k] is None:
                    row.append("no command")
                else:
                    row.append("yes" if flag in flags[k] else "-")
            rows.append(row)
    return table(["Command", "Flag", "kfw", "kfm", "kfde"], rows)


def block_messages_counts(f, args):
    sm = {k: t.socket_messages() for k, t in f.trees.items()}
    w, m, d = (set(sm[k][0]) for k in ("kfw", "kfm", "kfde"))
    rows = [
        ("`SocketMessage` variants", len(w), len(m), len(d)),
        ("In all three", len(w & m & d), len(w & m & d), len(w & m & d)),
        ("In kfde and kfw, not kfm", len((w & d) - m), "-", len((w & d) - m)),
        ("In kfde and kfm, not kfw", "-", len((m & d) - w), len((m & d) - w)),
        ("Only in kfde", "-", "-", len(d - w - m)),
        ("In kfw or kfm, not in kfde", len(w - d), len(m - d), "-"),
    ]
    return table(["", "kfw", "kfm", "kfde"], rows)


def block_messages_only(f, args):
    sm = {k: t.socket_messages() for k, t in f.trees.items()}
    w, m, d = (set(sm[k][0]) for k in ("kfw", "kfm", "kfde"))
    lines = []
    for label, names in (
        ("Only in kfde", d - w - m),
        ("In kfde and kfw, not in kfm", (w & d) - m),
        ("In kfde and kfm, not in kfw", (m & d) - w),
    ):
        lines.append(f"- {label}: " + (code_list(sorted(names)) if names else "none") + ".")
    return "\n".join(lines)


def struct_sets(f, struct):
    out = {}
    for k, t in f.trees.items():
        live, dead = t.struct_keys(struct)
        out[k] = (live, dead)
    return out


STRUCTS = [("StaticConfig", "top level"), ("WorkspaceConfig", "workspace"), ("MonitorConfig", "monitor")]


def block_keys_counts(f, args):
    rows = []
    totals = [0, 0, 0]
    for st, label in STRUCTS:
        s = struct_sets(f, st)
        w, m, d = (set(s[k][0]) for k in ("kfw", "kfm", "kfde"))
        rows.append((f"`{st}` ({label})", len(w), len(m), len(d), len(w & m & d), len((w & d) - m), len((m & d) - w), len(d - w - m), len((w | m) - d)))
    t = [sum(r[i] for r in rows) for i in range(1, 9)]
    rows.append(("Total", *t))
    return table(["Struct", "kfw", "kfm", "kfde", "All three", "kfw and kfde", "kfm and kfde", "kfde only", "Not read by kfde"], rows)


def block_keys_adopted(f, args):
    rows = []
    for st, label in STRUCTS:
        s = struct_sets(f, st)
        w, m, d = (set(s[k][0]) for k in ("kfw", "kfm", "kfde"))
        for key in sorted((w & d) - m):
            rows.append((f"`{key}`", label, "kfw", s["kfde"][0][key].first_sentence))
        for key in sorted((m & d) - w):
            rows.append((f"`{key}`", label, "kfm", s["kfde"][0][key].first_sentence))
    return table(["Key", "Level", "Also in", "What it is"], rows)


def block_keys_kde_only(f, args):
    keys = []
    for st, label in STRUCTS:
        s = struct_sets(f, st)
        w, m, d = (set(s[k][0]) for k in ("kfw", "kfm", "kfde"))
        keys += [f"`{k}` ({label})" for k in sorted(d - w - m)]
    return ", ".join(keys) if keys else "none"


def check_lists(f):
    src = f.kde.src("komorebi/src/check.rs")
    return {
        "ignored_root": set(string_list(src, "IGNORED_ROOT")),
        "no_effect_root": set(string_list(src, "NO_EFFECT_ROOT")),
        "no_effect_monitor": set(string_list(src, "NO_EFFECT_MONITOR")),
        "unsupported_workspace": set(string_list(src, "UNSUPPORTED_WORKSPACE")),
    }


def block_keys_not_read(f, args):
    rows = []
    seen = set()
    for st, label in STRUCTS:
        s = struct_sets(f, st)
        w, m, d = (set(s[k][0]) for k in ("kfw", "kfm", "kfde"))
        for key in sorted((w | m) - d):
            seen.add(key)
            where = ("kfw" if key in w else "") + (" and " if key in w and key in m else "") + ("kfm" if key in m else "")
            reason = NOT_READ.get(key)
            if reason is None:
                problem(f"keys: `{key}` ({label}) is in {where} and not read by kfde, and has no entry in NOT_READ")
                reason = "no reason recorded"
            rows.append((f"`{key}`", label, where, reason))
    for key in sorted(set(NOT_READ) - seen):
        problem(f"keys: NOT_READ names `{key}`, which no tree has and kfde lacks")
    # cross-check against the lists `komorebic check` uses to tell a key of another platform from a typo
    lists = check_lists(f)
    ours = {r[0].strip("`") for r in rows}
    claimed = lists["ignored_root"] | lists["unsupported_workspace"]
    if ours - claimed - {"scrolling"}:
        problem(f"keys: not read, but `komorebic check` does not know them: {sorted(ours - claimed - {'scrolling'})}")
    if claimed - ours:
        problem(f"keys: `komorebic check` lists keys that this table does not: {sorted(claimed - ours)}")
    return table(["Key", "Level", "In", "Why kfde does not read it"], rows)


def block_keys_no_effect(f, args):
    lists = check_lists(f)
    rows = []
    for key in sorted(lists["no_effect_root"]):
        rows.append((f"`{key}`", "top level", "kfm", "Read, so that a file written for kfm loads, and does nothing on KDE."))
    for key in sorted(lists["no_effect_monitor"]):
        rows.append((f"`{key}`", "monitor", "kfm", "Read, so that a file written for kfm loads, and does nothing on KDE."))
    rows.append(("`animation.fps`", "nested", "kfw, kfm", "Read, and ignored: the compositor animates at the refresh rate of the screen."))
    return table(["Key", "Level", "Also in", "Effect on KDE"], rows)


def block_keys_differ(f, args):
    rows = []
    for st, key, text in KEY_DIFFS:
        live = f.kde.struct_keys(st)[0]
        if key not in live:
            problem(f"keys: KEY_DIFFS names `{key}` of `{st}`, which kfde does not read")
            continue
        label = dict(STRUCTS)[st]
        rows.append((f"`{key}`", label, text))
    return table(["Key", "Level", "What is different on KDE"], rows)


def block_keys_nested(f, args):
    rows = []
    for st in ("BorderColours", "ThemeOptions", "Wallpaper", "AnimationsConfig"):
        s = {k: set(t.struct_keys(st)[0]) for k, t in f.trees.items()}
        only_w = sorted(s["kfw"] - s["kfde"])
        only_m = sorted(s["kfm"] - s["kfde"])
        rows.append((f"`{st}`", len(s["kfw"]), len(s["kfm"]), len(s["kfde"]), code_list(only_w) if only_w else "-", code_list(only_m) if only_m else "-"))
    return table(["Struct", "kfw", "kfm", "kfde", "In kfw, not kfde", "In kfm, not kfde"], rows)


def block_keys_same(f, args):
    lines = []
    for st, label in STRUCTS:
        s = struct_sets(f, st)
        w, m, d = (set(s[k][0]) for k in ("kfw", "kfm", "kfde"))
        names = sorted(w & m & d)
        lines.append(f"- {label} (`{st}`): " + code_list(names) + ".")
    return "\n".join(lines)


def block_events(f, args):
    ev = {k: t.events() for k, t in f.trees.items()}
    rows = []
    covered = {"kfw": set(), "kfm": set(), "kfde": set()}
    for name, source, kfw, kfm in EVENTS:
        if name not in ev["kfde"]:
            problem(f"events: EVENTS names `{name}`, which kfde does not have")
        covered["kfde"].add(name)
        cells = []
        for key, names in (("kfw", kfw), ("kfm", kfm)):
            if names is None:
                cells.append("-")
                continue
            for n in names:
                if n not in ev[key]:
                    problem(f"events: EVENTS says `{n}` is a {key} event, and it is not")
                covered[key].add(n)
            cells.append(code_list(names))
        rows.append((f"`{name}`", source, cells[0], cells[1]))
    for name in ev["kfde"]:
        if name not in covered["kfde"]:
            problem(f"events: kfde has the event `{name}`, which EVENTS does not describe")
    out = [table(["kfde event", "Source on KDE", "kfw", "kfm"], rows), ""]
    lines = []
    for key in ("kfw", "kfm"):
        missing = [n for n in ev[key] if n not in covered[key]]
        for n in missing:
            note = EVENTS_UNMAPPED.get(key, {}).get(n)
            if note is None:
                problem(f"events: {key} has the event `{n}`, which has no counterpart in EVENTS and no note in EVENTS_UNMAPPED")
                note = "no reason recorded"
            lines.append((n, key, note))
        for n in EVENTS_UNMAPPED.get(key, {}):
            if n not in ev[key]:
                problem(f"events: EVENTS_UNMAPPED names `{n}`, which {key} does not have")
    out.append(table(["Event with no counterpart in kfde", "In", "Why"], [(f"`{n}`", k, note) for n, k, note in lines]))
    return "\n".join(out)


def block_event_sources(f, args):
    n_win = f.kfw.enum_count("komorebi/src/winevent.rs", "WinEvent")
    n_ax = f.kfm.enum_count("komorebi/src/accessibility/notification_constants.rs", "AccessibilityNotification")
    n_ns = f.kfm.enum_count("komorebi/src/app_kit_notification_constants.rs", "AppKitWorkspaceNotification")
    n_kwin = f.kde.enum_count("komorebi/src/window_manager_event.rs", "KWinNotification")
    rows = [
        ("kfw", "`WinEvent`, the events of `SetWinEventHook`", n_win),
        ("kfm", "`AccessibilityNotification` and `AppKitWorkspaceNotification`", f"{n_ax} and {n_ns}"),
        ("kfde", "`KWinNotification`, the signals of the KWin script", n_kwin),
    ]
    return table(["Tree", "What the OS layer reports", "Kinds"], rows)


def block_backend(f, args):
    body = block(f.kde.src("komorebi/src/backend/mod.rs"), r"pub trait Backend\b")
    methods = re.findall(r"^    fn (\w+)\(", body, re.M)
    rows = []
    kwin_src = "\n".join(f.kde.src(p) for p in ("komorebi/src/kwin/bridge.js", "komorebi/src/kwin/mod.rs", "komorebi/src/kwin/effects.rs", "komorebi/src/kwin/wallpaper.rs", "komorebi/src/kwin/kwinrc.rs"))
    for name in methods:
        if name in BACKEND_PROVIDED:
            continue
        if name not in BACKEND:
            problem(f"backend: the trait has `{name}`, which BACKEND does not describe")
            continue
        kwin, kfw, kfm = BACKEND[name]
        for token in re.findall(r"`([^`]+)`", kwin):
            word = re.split(r"[(\s=]", token)[0]
            if word not in kwin_src and token not in kwin_src:
                problem(f"backend: `{name}`: `{token}` is not in the sources of the KWin backend")
        for key, names, tree in (("kfw", kfw, f.kfw), ("kfm", kfm, f.kfm)):
            for n in names or []:
                if not tree.has_fn(n):
                    problem(f"backend: `{name}`: `{n}` is not a function of {key}")
        rows.append((f"`{name}`", kwin, code_list(kfw) if kfw else "none", code_list(kfm) if kfm else "none"))
    for name in BACKEND:
        if name not in methods:
            problem(f"backend: BACKEND names `{name}`, which the trait does not have")
    return table(["`Backend` method", "On KDE", "kfw function", "kfm function"], rows)


def block_backend_counts(f, args):
    body = block(f.kde.src("komorebi/src/backend/mod.rs"), r"pub trait Backend\b")
    methods = re.findall(r"^    fn (\w+)\(", body, re.M)
    required = [m for m in methods if m not in BACKEND_PROVIDED]
    n_fw = len(re.findall(r"^    pub (?:unsafe )?fn ", f.kfw.src("komorebi/src/windows_api.rs"), re.M))
    n_fm = len(re.findall(r"^    pub (?:unsafe )?fn ", f.kfm.src("komorebi/src/macos_api.rs"), re.M))
    js = len(f.kde.src("komorebi/src/kwin/bridge.js").splitlines())
    rows = [
        ("kfw", "`WindowsApi` in `windows_api.rs`", f"{n_fw} public functions"),
        ("kfm", "`MacosApi` in `macos_api.rs`, and the `accessibility`, `core_graphics` and `skylight` modules", f"{n_fm} public functions in `MacosApi`"),
        ("kfde", "`Backend`, with `FakeBackend` for tests and `KWinBackend` for KWin", f"{len(methods)} methods ({len(required)} are required, {len(methods) - len(required)} are provided), and a script of {js} lines that runs inside KWin"),
    ]
    return table(["Tree", "The operating system layer", "Size"], rows)


def block_layouts(f, args):
    found = {}
    for k, t in f.trees.items():
        got = layouts_dir(t)
        if got is None:
            problem(f"layouts: cannot find komorebi-layouts for {k}")
            continue
        d, label = got
        body = block((d / "default_layout.rs").read_text(), r"^pub enum DefaultLayout\b")
        live, _ = variants(body)
        found[k] = (label, OrderedDict((v.name, v) for v in live), d)
    if len(found) < 3:
        return "(komorebi-layouts not found)"
    rows = []
    for name in found["kfde"][1]:
        mark = lambda k: "yes" if name in found[k][1] else "-"
        rows.append((f"`{name}`", mark("kfw"), mark("kfm"), "yes"))
    for k in ("kfw", "kfm"):
        for name in found[k][1]:
            if name not in found["kfde"][1]:
                problem(f"layouts: {k} has the layout `{name}`, which kfde does not")
    out = [table(["`DefaultLayout`", "kfw", "kfm", "kfde"], rows), ""]
    out.append("The crate each tree builds: " + "; ".join(f"{k}: {found[k][0]}" for k in ("kfw", "kfm", "kfde")) + ".")
    ref = found["kfw"][2]
    names = sorted(p.name for p in ref.glob("*.rs"))
    diffs = []
    for k in ("kfm", "kfde"):
        differ = [n for n in names if not (found[k][2] / n).exists() or (found[k][2] / n).read_text() != (ref / n).read_text()]
        diffs.append(f"{k}: {len(differ)} of {len(names)} source files differ from the ones of kfw" + (f" ({code_list(differ)})" if differ else ""))
    out.append("")
    out.append("Compared file by file with the crate of kfw, " + "; ".join(diffs) + ".")
    return "\n".join(out)


def block_native(f, args):
    rows = []
    for row in NATIVE:
        for key, rel, pattern in row["needs"]:
            tree = f.trees[key]
            try:
                text = tree.src(rel)
            except FileNotFoundError:
                problem(f"native: `{row['what']}`: {key} has no file {rel}")
                continue
            if not re.search(pattern, text):
                problem(f"native: `{row['what']}`: {key} {rel} does not match /{pattern}/")
        for key, pattern in row.get("absent", []):
            if re.search(pattern, f.trees[key].all_rust()):
                problem(f"native: `{row['what']}`: {key} has code that matches /{pattern}/, which the row says it has not")
        rows.append((row["what"], row["kfw"], row["kfm"], row["kfde"]))
    return table(["What", "kfw", "kfm", "On KDE"], rows)


BLOCKS = OrderedDict(
    [
        ("sources", block_sources),
        ("commands-counts", block_commands_counts),
        ("commands-all3", block_commands_all3),
        ("commands-adopted", block_commands_adopted),
        ("commands-kfm", block_commands_kfm),
        ("commands-kde", block_commands_kde),
        ("commands-missing", block_commands_missing),
        ("commands-flags", block_commands_flags),
        ("messages-counts", block_messages_counts),
        ("messages-only", block_messages_only),
        ("keys-counts", block_keys_counts),
        ("keys-same", block_keys_same),
        ("keys-adopted", block_keys_adopted),
        ("keys-kde-only", block_keys_kde_only),
        ("keys-no-effect", block_keys_no_effect),
        ("keys-not-read", block_keys_not_read),
        ("keys-differ", block_keys_differ),
        ("keys-nested", block_keys_nested),
        ("events", block_events),
        ("event-sources", block_event_sources),
        ("backend-counts", block_backend_counts),
        ("backend", block_backend),
        ("layouts", block_layouts),
        ("native", block_native),
    ]
)
VOLATILE = {"sources"}


def slug(heading):
    h = re.sub(r"`", "", heading.strip().lower())
    return re.sub(r"[^\w\- ]", "", h).replace(" ", "-")


def check_links(path, docs_root):
    """Relative links of COMPARE.md: the file exists, and so does the heading of an anchor."""
    text = Path(path).read_text()
    base = Path(path).resolve().parent
    problems = []
    for m in re.finditer(r"\[[^\]]*\]\(([^)\s]+)\)", text):
        target = m.group(1)
        if re.match(r"[a-z]+:", target) or target.startswith("#"):
            continue
        file, _, anchor = target.partition("#")
        dest = (base / file).resolve()
        if not dest.exists():
            problems.append(f"link to {file}: no such file")
            continue
        if anchor and dest.suffix == ".md":
            heads = {slug(h) for h in re.findall(r"^#+\s+(.*)$", dest.read_text(), re.M)}
            if anchor not in heads:
                problems.append(f"link to {file}#{anchor}: no such heading")
    return problems


def render(name, text):
    return f"<!-- gen:{name} -->\n{text}\n<!-- /gen:{name} -->"


MARK = re.compile(r"<!-- gen:([\w-]+) -->\n(.*?)<!-- /gen:\1 -->", re.S)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--kfw", default=str(DEFAULT_KFW))
    ap.add_argument("--kfm", default=str(DEFAULT_KFM))
    ap.add_argument("--kfde", default=str(HERE))
    ap.add_argument("--update", metavar="FILE")
    ap.add_argument("--check", metavar="FILE")
    ap.add_argument("--block", help="print only this block")
    ap.add_argument("--docs-root", help="check the relative links of the file against the docs of this checkout")
    args = ap.parse_args()

    f = Facts(args)
    for text, holds in _assumptions(f):
        if not holds:
            problem(f"the prose assumes that {text}, and that is no longer true: read the prose again")
    generated = OrderedDict()
    for name, fn in BLOCKS.items():
        if args.block and name != args.block:
            continue
        generated[name] = fn(f, args)

    for e in ERRORS:
        print(f"error: {e}", file=sys.stderr)

    if args.update or args.check:
        path = Path(args.update or args.check)
        text = path.read_text()
        found = {m.group(1): m.group(2).rstrip("\n") for m in MARK.finditer(text)}
        for name in found:
            if name not in BLOCKS:
                print(f"error: {path} has a block `{name}` that this script does not generate", file=sys.stderr)
                ERRORS.append(name)
        for name in BLOCKS:
            if name not in found and not args.block:
                print(f"note: {path} has no block `{name}`", file=sys.stderr)
        stale = [n for n, body in found.items() if n in generated and n not in VOLATILE and body != generated[n]]
        if args.docs_root:
            for msg in check_links(path, args.docs_root):
                problem(f"{path}: {msg}")
            for e in ERRORS:
                if e.startswith(str(path)):
                    print(f"error: {e}", file=sys.stderr)
        if args.check:
            if stale:
                print("stale blocks: " + ", ".join(stale), file=sys.stderr)
            sys.exit(1 if stale or ERRORS else 0)
        new = MARK.sub(lambda m: render(m.group(1), generated.get(m.group(1), m.group(2).rstrip("\n"))), text)
        path.write_text(new)
        print(f"updated {len(found)} blocks in {path}" + (f", {len(stale)} had changed: {', '.join(stale)}" if stale else ""))
        sys.exit(1 if ERRORS else 0)

    for name, body in generated.items():
        print(render(name, body))
        print()
    sys.exit(1 if ERRORS else 0)


if __name__ == "__main__":
    try:
        main()
    except FileNotFoundError as e:
        print(f"error: {e.filename} is not there: --kfw, --kfm and --kfde are the roots of complete checkouts", file=sys.stderr)
        sys.exit(2)
