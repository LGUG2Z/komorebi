// Tests of bridge.js, run by `node --test` (node's own test runner: no package.json and no
// dependencies). Each test loads the script as the daemon has KWin load it, with `var CFG = {...};`
// in front, into a `node:vm` context of its own whose globals are a fake of the part of KWin's API
// that kwin-api.d.ts declares: a `workspace` whose signals are tiny emitters, windows that are
// plain objects, a `callDBus` that records the script's calls, and a `QTimer` that a test fires.
// The fake does what KWin does as far as the bridge can tell (setting a property raises its
// signal, KWin activates a new window before it announces it); whether KWin really behaves so is
// what the VM tests check.
//
// The first test plays a session and compares what the bridge sends with fixtures/events.json,
// which the tests of `kwin::state` feed to the table; the commands it answers the bridge's `Poll`
// with are fixtures/commands.json, which the tests of `kwin` expect the backend to send. A change
// to what one side sends or reads fails a test on the other side.

"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const SOURCE = fs.readFileSync(path.join(__dirname, "bridge.js"), "utf8");

function fixture(name) {
    return JSON.parse(fs.readFileSync(path.join(__dirname, "fixtures", name), "utf8"));
}

// what `kwin::load_script` prepends
const CFG = {
    service: "org.komorebi.Bridge",
    path: "/org/komorebi/Bridge",
    iface: "org.komorebi.Bridge",
    poll_path: "/org/komorebi/Bridge/Poll",
    poll_iface: "org.komorebi.Bridge.Poll",
    hold_ms: 10000,
};

class Signal {
    constructor() {
        this.slots = [];
    }

    connect(slot) {
        this.slots.push(slot);
    }

    emit(...args) {
        for (const slot of this.slots) {
            slot(...args);
        }
    }
}

function geometry(x, y, width, height) {
    return { x, y, width, height };
}

// A window. `id` becomes its `internalId`, which is a QUuid in KWin: an object, not a string.
// Setting `minimized`, `frameGeometry` or `desktops` raises the signal KWin raises, and
// `closeWindow` closes it once it is in a KWin. `setMaximize` sets `maximizeMode` the way KWin's
// does and records what it was asked in `maximizations`. `place` is where KWin puts the window
// when it is asked for a frame: where it was asked, unless a test says otherwise.
function window({
    id,
    minimized = false,
    frameGeometry = geometry(0, 0, 100, 100),
    desktops = [],
    place = (g) => g,
    ...rest
}) {
    const w = {
        internalId: { toString: () => id },
        pid: 0,
        resourceClass: "",
        resourceName: "",
        desktopFileName: "",
        caption: "",
        normalWindow: true,
        skipTaskbar: false,
        skipPager: false,
        skipSwitcher: false,
        fullScreen: false,
        maximizeMode: 0,
        minSize: { width: 0, height: 0 },
        maximizations: [],
        setMaximize: (vertically, horizontally) => {
            w.maximizations.push([vertically, horizontally]);
            w.maximizeMode = (vertically ? 1 : 0) | (horizontally ? 2 : 0);
        },
        move: false,
        resize: false,
        frameGeometryChanged: new Signal(),
        minimizedChanged: new Signal(),
        captionChanged: new Signal(),
        desktopsChanged: new Signal(),
        closeWindow: () => assert.fail("a window that is in no KWin was closed"),
        ...rest,
    };
    let state = { minimized, frameGeometry, desktops };
    Object.defineProperties(w, {
        desktops: {
            get: () => state.desktops,
            set: (ds) => {
                state.desktops = ds;
                w.desktopsChanged.emit();
            },
        },
        minimized: {
            get: () => state.minimized,
            set: (v) => {
                state.minimized = v;
                w.minimizedChanged.emit();
            },
        },
        frameGeometry: {
            get: () => state.frameGeometry,
            set: (g) => {
                const old = state.frameGeometry;
                state.frameGeometry = place(geometry(g.x, g.y, g.width, g.height));
                w.frameGeometryChanged.emit(old);
            },
        },
    });
    return w;
}

// one output, with a panel along its bottom edge
const DP1 = {
    name: "DP-1",
    manufacturer: "ACME",
    model: "M1",
    serialNumber: "S1",
    geometry: geometry(0, 0, 1920, 1080),
    devicePixelRatio: 1,
    // not KWin's: what the fake's `clientArea` answers for it
    placementArea: geometry(0, 0, 1920, 1050),
};

// the virtual desktops of a KWin that nobody has given any others
function twoDesktops() {
    return [
        { id: "desktop-1", name: "Desktop 1" },
        { id: "desktop-2", name: "Desktop 2" },
    ];
}

// A KWin with `windows` open, `active` focused and the pointer at `cursor`, running the bridge.
// Every output shows the first of `desktops`.
function kwin({
    windows = [],
    screens = [DP1],
    desktops = twoDesktops(),
    active = null,
    cursor = { x: 0, y: 0 },
} = {}) {
    const calls = [];
    const printed = [];
    const raised = [];
    const timers = [];
    const clock = { now: 1000 };
    const open = [];
    let activeWindow = active;
    const current = Object.fromEntries(screens.map((s) => [s.name, desktops[0]]));

    const workspace = {
        screens,
        desktops,
        cursorPos: cursor,
        windowList: () => open.slice(),
        clientArea: (option, output, desktop) => {
            assert.equal(option, 7, "the placement area");
            assert.equal(desktop, current[output.name], "the desktop the output shows");
            return output.placementArea;
        },
        currentDesktopForScreen: (output) => current[output.name],
        // per-output virtual desktops: the other outputs keep theirs
        setCurrentDesktopForScreen: (desktop, output) => {
            current[output.name] = desktop;
            workspace.currentDesktopChanged.emit();
        },
        raiseWindow: (w) => raised.push(w),
        windowAdded: new Signal(),
        windowRemoved: new Signal(),
        windowActivated: new Signal(),
        currentDesktopChanged: new Signal(),
        desktopsChanged: new Signal(),
    };
    Object.defineProperty(workspace, "activeWindow", {
        get: () => activeWindow,
        set: (w) => {
            activeWindow = w;
            workspace.windowActivated.emit(w);
        },
    });

    class QTimer {
        constructor() {
            this.interval = 0;
            this.timeout = new Signal();
            this.running = false;
            timers.push(this);
        }

        start() {
            this.running = true;
        }
    }

    // a window KWin has: it can close
    function adopt(w) {
        open.push(w);
        w.closeWindow = () => k.close(w);
    }
    windows.forEach(adopt);

    const context = vm.createContext({
        workspace,
        KWin: { PlacementArea: 7 },
        callDBus: (service, path, iface, method, arg, callback) => {
            calls.push({ service, path, iface, method, arg, callback });
        },
        print: (...values) => printed.push(values.join(" ")),
        QTimer,
        Date: { now: () => clock.now },
    });
    vm.runInContext(`var CFG = ${JSON.stringify(CFG)};\n${SOURCE}`, context, {
        filename: "bridge.js",
    });

    const k = {
        workspace,
        calls,
        printed,
        raised,
        timers,
        clock,
        // what the bridge has sent with `Event`, oldest first
        events: () => calls.filter((c) => c.method === "Event").map((c) => JSON.parse(c.arg)),
        polls: () => calls.filter((c) => c.method === "Poll"),
        // the daemon answers the bridge's last `Poll` with `text`
        reply: (text) => calls.filter((c) => c.method === "Poll").at(-1).callback(text),
        // a window opens: KWin activates it before it announces it, unless it is not to be focused
        open: (w, { focus = true } = {}) => {
            adopt(w);
            if (focus) {
                workspace.activeWindow = w;
            }
            workspace.windowAdded.emit(w);
        },
        close: (w) => {
            open.splice(open.indexOf(w), 1);
            if (activeWindow === w) {
                activeWindow = null;
            }
            workspace.windowRemoved.emit(w);
        },
        // the daemon answers the bridge's last `Poll` with these batches
        answer: (batches) => k.reply(JSON.stringify(batches)),
        // somebody makes another desktop (System Settings, the pager)
        createDesktop: (desktop) => {
            desktops.push(desktop);
            workspace.desktopsChanged.emit();
        },
    };
    return k;
}

test("what the bridge sends for a session is what the daemon's tests read", () => {
    const [events, commands] = [fixture("events.json"), fixture("commands.json")];
    const editor = window({
        id: "{0e5a1c3b-0000-4000-8000-00000000000a}",
        pid: 4242,
        resourceClass: "org.kde.kate",
        resourceName: "kate",
        desktopFileName: "org.kde.kate",
        caption: "notes.txt — Kate",
        frameGeometry: geometry(0, 0, 1920, 1050),
        minSize: { width: 300, height: 200 },
        desktops: [{ id: "desktop-1" }],
    });
    const k = kwin({ windows: [editor], active: editor, cursor: { x: 480.6, y: 525.2 } });

    const terminal = window({
        id: "{7f3d9e21-0000-4000-8000-00000000000b}",
        pid: 4343,
        resourceClass: "foot",
        resourceName: "foot",
        desktopFileName: "foot",
        caption: "a terminal",
        frameGeometry: geometry(480, 270, 960, 540),
        minSize: { width: 21, height: 34.5 },
        desktops: [{ id: "desktop-1" }],
    });
    k.open(terminal);
    const dialog = window({
        id: "{c4b8a2f0-0000-4000-8000-00000000000c}",
        pid: 4242,
        resourceClass: "org.kde.kate",
        resourceName: "kate",
        desktopFileName: "org.kde.kate",
        caption: "Save As — Kate",
        normalWindow: false,
        frameGeometry: geometry(660, 290, 600, 500),
        desktops: [{ id: "desktop-1" }],
    });
    k.open(dialog, { focus: false });
    terminal.minimized = true;
    terminal.minimized = false;
    // the client took the frame it was given to whole cells of its font
    terminal.frameGeometry = geometry(480.4, 270, 956, 532);

    const [cursorQuery, windowCommands, workspaceCommands] = commands;
    assert.equal(k.events().at(-1).seq, cursorQuery.after_seq);
    k.workspace.cursorPos = { x: 1200, y: 700.5 };
    k.answer([cursorQuery.batch]);

    k.close(terminal);

    // the editor is tiled, focused, raised, minimized and brought back, maximized and restored,
    // the dialog closed, and the terminal, which has closed, is asked for a frame
    assert.equal(k.events().at(-1).seq, windowCommands.after_seq);
    k.answer([windowCommands.batch]);

    // the editor is sent to the second workspace, and the output is switched to it
    assert.equal(k.events().at(-1).seq, workspaceCommands.after_seq);
    k.answer([workspaceCommands.batch]);
    const [first, second] = k.workspace.desktops;
    // (an array the script made, whose prototype is not this file's)
    assert.deepStrictEqual([...editor.desktops], [second]);
    assert.equal(k.workspace.currentDesktopForScreen(DP1), second);

    // then somebody else makes a desktop, moves the editor back to the first, and shows that
    k.createDesktop({ id: "desktop-3", name: "mail" });
    editor.desktops = [first];
    k.workspace.setCurrentDesktopForScreen(first, DP1);

    assert.deepStrictEqual(k.events(), events);
    assert.deepStrictEqual(editor.frameGeometry, geometry(0, 0, 960, 1050));
    assert.equal(k.workspace.activeWindow, editor);
    assert.deepStrictEqual(k.raised, [editor]);
    assert.equal(editor.minimized, false);
    assert.deepStrictEqual(editor.maximizations, [
        [true, true],
        [false, false],
    ]);
    assert.equal(editor.maximizeMode, 0);
    assert.deepStrictEqual(k.workspace.windowList(), [editor]);
});

test("the desktop each output shows is sent when it changes, and once", () => {
    const k = kwin();
    const [first, second] = k.workspace.desktops;

    k.workspace.setCurrentDesktopForScreen(second, DP1);
    k.timers[0].timeout.emit();
    k.workspace.setCurrentDesktopForScreen(first, DP1);

    assert.deepStrictEqual(k.events().slice(1), [
        { type: "current_desktops", current: [{ output: "DP-1", id: "desktop-2" }], seq: 2 },
        { type: "current_desktops", current: [{ output: "DP-1", id: "desktop-1" }], seq: 3 },
    ]);
});

test("a desktop made while the bridge runs is sent as the new list of desktops", () => {
    const k = kwin();

    k.createDesktop({ id: "desktop-3", name: "mail" });

    assert.deepStrictEqual(k.events().at(-1), {
        type: "desktop_list",
        desktops: [
            { id: "desktop-1", name: "Desktop 1" },
            { id: "desktop-2", name: "Desktop 2" },
            { id: "desktop-3", name: "mail" },
        ],
        seq: 2,
    });
});

test("a desktop a command names that KWin does not have is given back as failed", () => {
    const w = window({ id: "{w}", desktops: [{ id: "desktop-1" }] });
    const k = kwin({ windows: [w] });
    const commands = [
        { op: "desktop", id: "{w}", v: "gone" },
        { op: "output_desktop", output: "DP-1", v: "gone" },
        { op: "output_desktop", output: "HDMI-9", v: "desktop-2" },
    ];

    k.answer([{ batch: 6, cmds: commands }]);

    assert.deepStrictEqual(w.desktops, [{ id: "desktop-1" }]);
    assert.equal(k.workspace.currentDesktopForScreen(DP1).id, "desktop-1");
    assert.deepStrictEqual(k.events().at(-1), { type: "failed", batch: 6, cmds: commands, seq: 2 });
});

test("a window KWin activates before announcing it is announced first, and once", () => {
    const k = kwin();
    const w = window({ id: "{w}" });

    k.open(w);

    assert.deepStrictEqual(
        k.events().map((e) => [e.type, e.id]),
        [
            ["snapshot", undefined],
            ["added", "{w}"],
            ["activated", "{w}"],
        ],
    );
});

test("no window having the focus is an activation with no id", () => {
    const w = window({ id: "{w}" });
    const k = kwin({ windows: [w], active: w });

    k.workspace.activeWindow = null;

    assert.deepStrictEqual(k.events().at(-1), { type: "activated", id: null, seq: 2 });
});

test("dialogs, the lock screen and windows nobody can switch to are not application windows", () => {
    const windows = [
        window({ id: "{app}" }),
        window({ id: "{dialog}", normalWindow: false }),
        window({ id: "{lock}", resourceClass: "kwin_wayland" }),
        window({ id: "{hidden}", skipTaskbar: true, skipPager: true, skipSwitcher: true }),
        window({ id: "{no-taskbar}", skipTaskbar: true }),
    ];
    const k = kwin({ windows });

    const [snapshot] = k.events();

    assert.deepStrictEqual(
        snapshot.windows.map((w) => [w.id, w.normal]),
        [
            ["{app}", true],
            ["{dialog}", false],
            ["{lock}", false],
            ["{hidden}", false],
            ["{no-taskbar}", true],
        ],
    );
});

test("a window maximized one way only is not maximized", () => {
    const windows = [0, 1, 2, 3].map((mode) => window({ id: `{${mode}}`, maximizeMode: mode }));
    const k = kwin({ windows });

    const [snapshot] = k.events();

    assert.deepStrictEqual(
        snapshot.windows.map((w) => w.maximized),
        [false, false, false, true],
    );
});

test("a title change is reported with the title the window has now", () => {
    const w = window({ id: "{beta}", caption: "beta" });
    const k = kwin({ windows: [w] });

    w.caption = "beta-renamed";
    w.captionChanged.emit();

    assert.deepStrictEqual(k.events().at(-1), {
        type: "caption",
        seq: 2,
        id: "{beta}",
        caption: "beta-renamed",
    });
});

test("every call goes to the daemon's names, with one JSON string", () => {
    const k = kwin();
    k.answer([]);

    assert.deepStrictEqual(
        k.calls.map((c) => [c.service, c.path, c.iface, c.method, typeof c.arg]),
        [
            [CFG.service, CFG.path, CFG.iface, "Event", "string"],
            [CFG.service, CFG.poll_path, CFG.poll_iface, "Poll", "string"],
            [CFG.service, CFG.poll_path, CFG.poll_iface, "Poll", "string"],
        ],
    );
    assert.deepStrictEqual(JSON.parse(k.polls()[0].arg), { hold_ms: CFG.hold_ms });
});

test("the bridge always has a Poll open: one when it starts, and another after each answer", () => {
    const k = kwin();
    assert.equal(k.polls().length, 1);

    k.answer([]);
    k.answer([{ batch: 1, cmds: [{ op: "cursor", token: 1 }] }]);

    assert.equal(k.polls().length, 3);
});

test("the watchdog calls Poll again when the last call is older than the hold and 3 seconds", () => {
    const k = kwin();
    const [watchdog] = k.timers;
    assert.equal(watchdog.interval, 1000);
    assert.ok(watchdog.running);

    k.clock.now += CFG.hold_ms + 3000;
    watchdog.timeout.emit();
    assert.equal(k.polls().length, 1, "not older yet");

    k.clock.now += 1;
    watchdog.timeout.emit();
    assert.equal(k.polls().length, 2);
});

test("the answer to a Poll that the watchdog replaced is dropped", () => {
    const k = kwin();
    const [stale] = k.polls();
    k.clock.now += CFG.hold_ms + 3001;
    k.timers[0].timeout.emit();

    stale.callback(JSON.stringify([{ batch: 1, cmds: [{ op: "cursor", token: 1 }] }]));

    assert.equal(k.events().length, 1, "only the snapshot");
    assert.equal(k.polls().length, 2, "and no third Poll");
});

test("an answer that is not JSON is printed, and the bridge polls again", () => {
    const k = kwin();

    k.reply("not json");

    assert.match(k.printed[0], /^komorebi bridge: bad poll reply: SyntaxError/);
    assert.equal(k.polls().length, 2);
});

test("a command the bridge does not know is given back as failed", () => {
    const k = kwin();

    k.answer([{ batch: 4, cmds: [{ op: "cursor", token: 9 }, { op: "teleport" }] }]);

    assert.deepStrictEqual(k.events().slice(1), [
        { type: "cursor", token: 9, x: 0, y: 0, seq: 2 },
        { type: "failed", batch: 4, cmds: [{ op: "teleport" }], seq: 3 },
    ]);
});

test("a frame is set as a plain object, and the frame KWin gives comes back as an event", () => {
    const w = window({ id: "{w}", frameGeometry: geometry(0, 0, 100, 100) });
    const k = kwin({ windows: [w] });

    k.answer([{ batch: 1, cmds: [{ op: "geo", id: "{w}", x: 10, y: 20, w: 300, h: 400 }] }]);

    assert.deepStrictEqual(w.frameGeometry, geometry(10, 20, 300, 400));
    assert.deepStrictEqual(k.events().slice(1), [
        { type: "geometry", id: "{w}", geo: { x: 10, y: 20, w: 300, h: 400 }, seq: 2 },
    ]);
});

test("a window the bridge has not seen is found in KWin's list", () => {
    const k = kwin();
    const unseen = window({ id: "{unseen}" });
    // in KWin's list, without a windowAdded
    k.workspace.windowList = () => [unseen];

    k.answer([{ batch: 1, cmds: [{ op: "min", id: "{unseen}", v: true }] }]);

    assert.equal(unseen.minimized, true);
    assert.equal(k.events().length, 1, "and nothing failed");
});

test("a command for a window KWin no longer has is given back, and the rest are carried out", () => {
    const w = window({ id: "{w}" });
    const k = kwin({ windows: [w] });
    const gone = { op: "activate", id: "{gone}" };

    k.answer([{ batch: 3, cmds: [gone, { op: "raise", id: "{w}" }] }]);

    assert.deepStrictEqual(k.raised, [w]);
    assert.deepStrictEqual(k.events().at(-1), { type: "failed", batch: 3, cmds: [gone], seq: 2 });
});

test("a command KWin throws on is printed and given back as failed", () => {
    const w = window({ id: "{w}" });
    const k = kwin({ windows: [w] });
    w.closeWindow = () => {
        throw new Error("the window is being destroyed");
    };
    const close = { op: "close", id: "{w}" };

    k.answer([{ batch: 5, cmds: [close] }]);

    assert.match(k.printed[0], /^komorebi bridge: Error: the window is being destroyed in /);
    assert.deepStrictEqual(k.events().at(-1), { type: "failed", batch: 5, cmds: [close], seq: 2 });
});

// KWin keeps 100 pixels of a window that it resizes on an output
function keepOnScreen(g) {
    return geometry(Math.max(g.x, 100 - g.width), g.y, g.width, g.height);
}

test("a window KWin pulls back from where it was asked to be is asked for the position again", () => {
    let pulled = false;
    const w = window({
        id: "{w}",
        // the first time only, as when the client commits a new size
        place: (g) => (pulled ? g : ((pulled = true), keepOnScreen(g))),
    });
    const k = kwin({ windows: [w] });

    k.answer([{ batch: 1, cmds: [{ op: "geo", id: "{w}", x: -900, y: 0, w: 800, h: 600 }] }]);

    assert.deepStrictEqual(w.frameGeometry, geometry(-900, 0, 800, 600));
    assert.deepStrictEqual(
        k.events().slice(1).map((e) => e.geo),
        [
            { x: -700, y: 0, w: 800, h: 600 },
            { x: -900, y: 0, w: 800, h: 600 },
        ],
    );
});

test("a window that cannot go where it was asked is asked again three times at most", () => {
    let frames = 0;
    const w = window({ id: "{w}", place: (g) => (frames++, keepOnScreen(g)) });
    const k = kwin({ windows: [w] });

    k.answer([{ batch: 1, cmds: [{ op: "geo", id: "{w}", x: -900, y: 0, w: 800, h: 600 }] }]);

    assert.equal(frames, 4, "the command and three tries");
    assert.deepStrictEqual(w.frameGeometry, geometry(-700, 0, 800, 600));
});

test("a window is asked for its position again for a second after the command, and no longer", () => {
    const w = window({ id: "{w}" });
    const k = kwin({ windows: [w] });
    k.answer([{ batch: 1, cmds: [{ op: "geo", id: "{w}", x: -900, y: 0, w: 800, h: 600 }] }]);

    k.clock.now += 1001;
    w.frameGeometry = geometry(-700, 0, 800, 600);

    assert.deepStrictEqual(w.frameGeometry, geometry(-700, 0, 800, 600));
});

test("a window the user is moving or resizing is not asked for its position again", () => {
    const w = window({ id: "{w}", move: true });
    const k = kwin({ windows: [w] });
    k.answer([{ batch: 1, cmds: [{ op: "geo", id: "{w}", x: -900, y: 0, w: 800, h: 600 }] }]);

    w.frameGeometry = geometry(-700, 0, 800, 600);

    assert.deepStrictEqual(w.frameGeometry, geometry(-700, 0, 800, 600));
});

test("a window maximized right after it was given a frame is not put back at that frame", () => {
    const w = window({ id: "{w}" });
    const maximize = w.setMaximize;
    // KWin gives a maximized window the work area, which moves its frame
    w.setMaximize = (vertically, horizontally) => {
        maximize(vertically, horizontally);
        w.frameGeometry = geometry(0, 0, 1920, 1050);
    };
    const k = kwin({ windows: [w] });
    k.answer([{ batch: 1, cmds: [{ op: "geo", id: "{w}", x: 480, y: 270, w: 960, h: 540 }] }]);

    k.answer([{ batch: 2, cmds: [{ op: "maximize", id: "{w}", v: true }] }]);

    assert.deepStrictEqual(w.frameGeometry, geometry(0, 0, 1920, 1050));
});
