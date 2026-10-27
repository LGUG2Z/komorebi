// @ts-check
// komorebi's KWin bridge.
//
// A dumb adapter: it reports what KWin tells it and does what komorebi asks, and holds no window
// manager logic. The daemon prepends `var CFG = {...};` (the D-Bus names and the poll hold time).
//
// Everything crosses D-Bus as a JSON string. `callDBus` types a JS number as int32 and a mismatch
// with the declared signature is a silent error, and its callback only fires on success, so the
// inbound command loop below is guarded by a watchdog (see the end of the file).
//
// KWin runs this file as it is: there is no build step. `tsc -p tsconfig.json` checks it as strict
// TypeScript through its JSDoc comments, against the part of KWin's API declared in
// kwin-api.d.ts, and `node --test bridge.test.js` runs it against a fake KWin (`just check-js`
// does both). The messages typed below are what `kwin::state` reads and `kwin::KWinBackend`
// sends; the tests of both sides read the same messages from fixtures/.

// ---- the messages --------------------------------------------------------------------------

/** @typedef {{ x: number, y: number }} Point */
/** @typedef {{ x: number, y: number, w: number, h: number }} Rect */

/**
 * A window as KWin describes it. `id` is KWin's UUID for the window, `normal` says whether it is
 * a window of an application (`isApplicationWindow`), and `desktops` are the ids of its virtual
 * desktops.
 *
 * @typedef {{
 *     id: string, pid: number, class: string, name: string, desktopFile: string, caption: string,
 *     normal: boolean, minimized: boolean, fullscreen: boolean, maximized: boolean, geo: Rect,
 *     minSize: { w: number, h: number }, desktops: string[]
 * }} WindowInfo
 */

/**
 * An output. `area` is the part of it windows may cover (not the panels) on the virtual desktop
 * it shows.
 *
 * @typedef {{
 *     name: string, manufacturer: string, model: string, serial: string, geo: Rect,
 *     scale: number, area: Rect
 * }} Screen
 */

/**
 * What the bridge sends, as the one argument of `Event`. `emit` adds `seq` to each, which counts
 * them from 1. `snapshot` is everything on the desktop, sent once, when the script starts; an
 * `activated` with no `id` means no window has the focus; `cursor` answers a cursor query, and
 * `failed` gives back the commands of a batch that could not be carried out.
 *
 * @typedef {{
 *     type: "snapshot", windows: WindowInfo[], screens: Screen[], active: string | null,
 *     cursor: Point
 * }} Snapshot
 * @typedef {WindowInfo & { type: "added" }} Added
 * @typedef {{ type: "removed", id: string }} Removed
 * @typedef {{ type: "activated", id: string | null }} Activated
 * @typedef {{ type: "minimized", id: string, v: boolean }} Minimized
 * @typedef {{ type: "geometry", id: string, geo: Rect }} Geometry
 * @typedef {{ type: "cursor", token: number, x: number, y: number }} CursorAnswer
 * @typedef {{ type: "failed", batch: number, cmds: Command[] }} Failed
 * @typedef {Snapshot | Added | Removed | Activated | Minimized | Geometry | CursorAnswer | Failed} BridgeEvent
 */

/**
 * What the daemon sends: the answer to `Poll` is a list of batches, each a list of commands. A
 * window command names the window by its `id`: `geo` sets its frame, `min` minimizes it (`v`) or
 * brings it back, and `activate`, `raise` and `close` do what they say. A cursor query is
 * numbered by `token` so that its answer can be matched.
 *
 * @typedef {{ op: "cursor", token: number }} CursorQuery
 * @typedef {{ op: "geo", id: string, x: number, y: number, w: number, h: number }} SetFrame
 * @typedef {{ op: "min", id: string, v: boolean }} SetMinimized
 * @typedef {{ op: "activate", id: string }} Activate
 * @typedef {{ op: "raise", id: string }} Raise
 * @typedef {{ op: "close", id: string }} Close
 * @typedef {SetFrame | SetMinimized | Activate | Raise | Close} WindowCommand
 * @typedef {CursorQuery | WindowCommand} Command
 * @typedef {{ batch: number, cmds: Command[] }} Batch
 */

// ---- reporting -------------------------------------------------------------------------------

var SVC = CFG.service, PATH = CFG.path, IFACE = CFG.iface;
var POLL_PATH = CFG.poll_path, POLL_IFACE = CFG.poll_iface;
var seq = 0;
/** @type {Record<string, KWinWindow>} */
var known = {};

// Where the daemon last asked a window to be, for a moment after it asked. KWin honors a plain
// move to anywhere, but when a window is *resized* it keeps 100px of it on an output, so a window
// that komorebi puts off screen (the scrolling layout does) and resizes in the same breath, or
// whose client rounds the size (a terminal snapping to its cells), is pulled back onto the screen
// once the client has committed the new size. By then asking again for the position is a plain
// move. (The first geometry change, to the requested rect, comes before the client has answered,
// so a match does not end the watch; only the clock does.)
/** @type {Record<string, { x: number, y: number, tries: number, until: number }>} */
var want = {};
var WANT_RETRIES = 3;
var WANT_MS = 1000;

/**
 * @template {BridgeEvent["type"]} T
 * @param {T} type
 * @param {Omit<Extract<BridgeEvent, { type: T }>, "type">} body
 */
function emit(type, body) {
    /** @type {Record<string, unknown>} */
    var ev = body || {};
    ev.type = type;
    ev.seq = ++seq;
    callDBus(SVC, PATH, IFACE, "Event", JSON.stringify(ev));
}

/**
 * @param {QRectF} g
 * @returns {Rect}
 */
function rect(g) {
    return { x: Math.round(g.x), y: Math.round(g.y), w: Math.round(g.width), h: Math.round(g.height) };
}

/** @param {KWinWindow} w */
function wid(w) {
    return String(w.internalId);
}

/** @param {KWinWindow} w */
function desktopIds(w) {
    var ds = [];
    for (var i = 0; i < w.desktops.length; i++) {
        ds.push(String(w.desktops[i].id));
    }
    return ds;
}

// A window of an application, as opposed to a dialog, a panel, a popup, or something the compositor
// shows for itself: the lock screen is a normal window by KWin's reckoning, but it is on every
// desktop, in no task list, no pager and no switcher, and belongs to KWin. komorebi tiling it would
// move everything else about while the screen is locked.
/** @param {KWinWindow} w */
function isApplicationWindow(w) {
    if (!w.normalWindow) {
        return false;
    }
    if (w.skipTaskbar && w.skipPager && w.skipSwitcher) {
        return false;
    }
    return String(w.resourceClass) !== "kwin_wayland";
}

// maximized both ways (KWin's MaximizeFull); a window maximized in one direction only is not "maximized"
/** @param {KWinWindow} w */
function isMaximized(w) {
    return w.maximizeMode === 3;
}

/**
 * @param {KWinWindow} w
 * @returns {WindowInfo}
 */
function describe(w) {
    return {
        id: wid(w),
        pid: w.pid,
        class: String(w.resourceClass),
        name: String(w.resourceName),
        desktopFile: String(w.desktopFileName),
        caption: String(w.caption),
        normal: isApplicationWindow(w),
        minimized: !!w.minimized,
        fullscreen: !!w.fullScreen,
        maximized: isMaximized(w),
        geo: rect(w.frameGeometry),
        minSize: { w: Math.round(w.minSize.width), h: Math.round(w.minSize.height) },
        desktops: desktopIds(w)
    };
}

/** @param {unknown} v */
function str(v) {
    return v === undefined || v === null ? "" : String(v);
}

/** @returns {Screen[]} */
function screens() {
    var out = [];
    for (var i = 0; i < workspace.screens.length; i++) {
        var s = workspace.screens[i];
        out.push({
            name: String(s.name),
            manufacturer: str(s.manufacturer),
            model: str(s.model),
            serial: str(s.serialNumber),
            geo: rect(s.geometry),
            scale: s.devicePixelRatio,
            area: rect(workspace.clientArea(KWin.PlacementArea, s, workspace.currentDesktopForScreen(s)))
        });
    }
    return out;
}

/** @returns {Point} */
function cursor() {
    var p = workspace.cursorPos;
    return { x: Math.round(p.x), y: Math.round(p.y) };
}

/**
 * @param {string} id
 * @returns {KWinWindow | null}
 */
function findWindow(id) {
    if (known[id]) {
        return known[id];
    }
    var l = workspace.windowList();
    for (var i = 0; i < l.length; i++) {
        if (wid(l[i]) === id) {
            return l[i];
        }
    }
    return null;
}

// A window has moved or been resized; if it is not where komorebi asked, ask again for the
// position alone (see `want`). A few tries at most, so a window that cannot go where it was asked
// (a fullscreen one, say) is left alone rather than chased.
/**
 * @param {KWinWindow} w
 * @param {string} id
 */
function settle(w, id) {
    var t = want[id];
    if (!t || w.move || w.resize) {
        return;
    }
    if (Date.now() > t.until || t.tries >= WANT_RETRIES) {
        delete want[id];
        return;
    }
    var g = w.frameGeometry;
    if (Math.round(g.x) !== t.x || Math.round(g.y) !== t.y) {
        t.tries++;
        w.frameGeometry = { x: t.x, y: t.y, width: g.width, height: g.height };
    }
}

// ---- hooks ---------------------------------------------------------------------------------

/**
 * @param {KWinWindow} w
 * @param {boolean} announce
 */
function track(w, announce) {
    var id = wid(w);
    if (known[id]) {
        return;
    }
    known[id] = w;
    if (announce) {
        emit("added", describe(w));
    }
    w.frameGeometryChanged.connect(function (old) {
        emit("geometry", { id: id, geo: rect(w.frameGeometry) });
        settle(w, id);
    });
    w.minimizedChanged.connect(function () {
        emit("minimized", { id: id, v: !!w.minimized });
    });
}

workspace.windowAdded.connect(function (w) {
    track(w, true);
});
workspace.windowRemoved.connect(function (w) {
    var id = wid(w);
    delete known[id];
    delete want[id];
    emit("removed", { id: id });
});
/** @type {string | null} */
var lastActive = null;

/** @param {KWinWindow | null} w */
function announceActive(w) {
    // kwin emits windowActivated for a new window *before* windowAdded; keep the invariant that no
    // event mentions a window before its `added`
    if (w) {
        track(w, true);
    }
    lastActive = w ? wid(w) : null;
    emit("activated", { id: lastActive });
}

workspace.windowActivated.connect(announceActive);

var existing = workspace.windowList();
for (var i = 0; i < existing.length; i++) {
    track(existing[i], false);
}

lastActive = workspace.activeWindow ? wid(workspace.activeWindow) : null;

emit("snapshot", {
    windows: workspace.windowList().map(describe),
    screens: screens(),
    active: workspace.activeWindow ? wid(workspace.activeWindow) : null,
    cursor: cursor()
});

// ---- commands from the daemon ----------------------------------------------------------------

/**
 * @param {Command} c
 * @returns {boolean} false if the command could not be carried out
 */
function applyOne(c) {
    if (c.op === "cursor") {
        // a query: the answer comes back as an event
        var p = cursor();
        emit("cursor", { token: c.token, x: p.x, y: p.y });
        return true;
    }

    var w = findWindow(c.id);
    if (!w) {
        return false;
    }
    switch (c.op) {
        case "geo":
            // a plain object converts to QRectF; there is no `Qt` global in kwin scripts
            want[c.id] = { x: c.x, y: c.y, tries: 0, until: Date.now() + WANT_MS };
            w.frameGeometry = { x: c.x, y: c.y, width: c.w, height: c.h };
            break;
        case "min":
            w.minimized = !!c.v;
            break;
        case "activate":
            workspace.activeWindow = w;
            break;
        case "raise":
            workspace.raiseWindow(w);
            break;
        case "close":
            w.closeWindow();
            break;
        default:
            return false;
    }
    return true;
}

/** @param {Batch} batch */
function applyBatch(batch) {
    var failed = [];
    for (var i = 0; i < batch.cmds.length; i++) {
        try {
            if (!applyOne(batch.cmds[i])) {
                failed.push(batch.cmds[i]);
            }
        } catch (e) {
            print("komorebi bridge: " + e + " in " + JSON.stringify(batch.cmds[i]));
            failed.push(batch.cmds[i]);
        }
    }
    if (failed.length > 0) {
        emit("failed", { batch: batch.batch, cmds: failed });
    }
}

// The daemon holds a Poll call open and answers it when it has commands, or after `hold_ms` with an
// empty batch. `callDBus` calls the callback only on success, so if the call fails (the daemon is
// gone, or it restarted) nothing would ever re-issue it; the watchdog does. Generations drop the
// reply of a call that has been superseded.
var gen = 0;
var lastIssue = 0;

function pull() {
    var g = ++gen;
    lastIssue = Date.now();
    callDBus(SVC, POLL_PATH, POLL_IFACE, "Poll", JSON.stringify({ hold_ms: CFG.hold_ms }), function (reply) {
        if (g !== gen) {
            return;
        }
        try {
            /** @type {Batch[]} */
            var batches = JSON.parse(reply);
            for (var i = 0; i < batches.length; i++) {
                applyBatch(batches[i]);
            }
        } catch (e) {
            print("komorebi bridge: bad poll reply: " + e);
        }
        pull();
    });
}

pull();

var watchdog = new QTimer();
watchdog.interval = 1000;
watchdog.timeout.connect(function () {
    if (Date.now() - lastIssue > CFG.hold_ms + 3000) {
        pull();
    }
});
watchdog.start();
