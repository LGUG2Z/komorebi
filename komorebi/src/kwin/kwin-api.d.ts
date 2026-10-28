// The part of KWin's scripting API that bridge.js uses, declared for `tsc` (tsconfig.json). KWin
// publishes no declarations; these are written from its scripting documentation
// (https://develop.kde.org/docs/plasma/kwin/api/) and from what a script is given in KWin 6.8.
// Something the bridge starts to use is declared here first.
//
// They are narrower than KWin's API on purpose: a property the bridge has no business setting is
// read-only, and `callDBus` takes one string argument, because a JavaScript number goes over D-Bus
// as a 32-bit integer and a call that does not match the method's signature fails without a word.

/** What the daemon prepends to the script (`kwin::load_script`): its D-Bus names and how long it
 * holds a `Poll` call. */
declare const CFG: {
    readonly service: string;
    readonly path: string;
    readonly iface: string;
    readonly poll_path: string;
    readonly poll_iface: string;
    readonly hold_ms: number;
};

/** A Qt signal. A script connects a function to it, which is called with the signal's arguments. */
interface Signal<Args extends unknown[] = []> {
    connect(slot: (...args: Args) => void): void;
}

interface QPointF {
    readonly x: number;
    readonly y: number;
}

interface QSizeF {
    readonly width: number;
    readonly height: number;
}

interface QRectF {
    readonly x: number;
    readonly y: number;
    readonly width: number;
    readonly height: number;
}

/** A `QUuid`, which is not a string: `String()` gives its text, `{...}` with the braces. */
interface QUuid {
    readonly __quuid: never;
}

/** A virtual desktop (`KWin::VirtualDesktop`). `id` is a UUID, as a string. */
interface VirtualDesktop {
    readonly id: string;
    readonly name: string;
}

/** An output (`KWin::Output`). */
interface Output {
    readonly name: string;
    readonly manufacturer: string;
    readonly model: string;
    readonly serialNumber: string;
    readonly geometry: QRectF;
    readonly devicePixelRatio: number;
}

/** A window (`KWin::Window`). */
interface KWinWindow {
    readonly internalId: QUuid;
    readonly pid: number;
    readonly resourceClass: string;
    readonly resourceName: string;
    readonly desktopFileName: string;
    readonly caption: string;
    readonly normalWindow: boolean;
    readonly skipTaskbar: boolean;
    readonly skipPager: boolean;
    readonly skipSwitcher: boolean;
    /** A panel: a window the compositor keeps other windows out of the way of. */
    readonly dock: boolean;
    minimized: boolean;
    /** How opaque the window is drawn, 0 to 1: 1 is not see-through at all. */
    opacity: number;
    readonly fullScreen: boolean;
    /** `KWin::MaximizeMode`: 0 restored, 1 vertically, 2 horizontally, 3 both ways. */
    readonly maximizeMode: number;
    /** Set to a plain `{x, y, width, height}`: a script has no `Qt` global to make a `QRectF`. */
    frameGeometry: QRectF;
    readonly minSize: QSizeF;
    /** True while the user moves the window with the pointer. */
    readonly move: boolean;
    /** True while the user resizes the window with the pointer. */
    readonly resize: boolean;
    /** The desktops the window is on; empty when it is on all of them. Setting it moves the
     * window. */
    desktops: VirtualDesktop[];
    readonly frameGeometryChanged: Signal<[oldGeometry: QRectF]>;
    readonly minimizedChanged: Signal;
    readonly captionChanged: Signal;
    readonly desktopsChanged: Signal;
    /** The user took hold of the window with the pointer, to move it (`move`) or resize it
     * (`resize`). */
    readonly interactiveMoveResizeStarted: Signal;
    /** A step of the drag, with the frame the user is asking for, which the window's frame follows
     * once the client has drawn at that size. */
    readonly interactiveMoveResizeStepped: Signal<[geometry: QRectF]>;
    /** The user let go of the window. */
    readonly interactiveMoveResizeFinished: Signal;
    closeWindow(): void;
    /**
     * Maximizes the window vertically and horizontally, or restores it from either. Maximizing
     * is to the work area, panels left alone, and restoring gives back the frame it had.
     */
    setMaximize(vertically: boolean, horizontally: boolean): void;
}

/** The global `workspace` (`KWin::ScriptingWorkspace`). */
interface Workspace {
    readonly screens: Output[];
    /** Setting it activates the window. */
    activeWindow: KWinWindow | null;
    readonly cursorPos: QPointF;
    /** Every virtual desktop, in order. */
    readonly desktops: VirtualDesktop[];
    windowList(): KWinWindow[];
    clientArea(option: number, output: Output, desktop: VirtualDesktop): QRectF;
    /** The desktop an output shows. With per-output virtual desktops each output has its own. */
    currentDesktopForScreen(output: Output): VirtualDesktop;
    /** Shows a desktop on one output; with per-output virtual desktops the others keep theirs. */
    setCurrentDesktopForScreen(desktop: VirtualDesktop, output: Output): void;
    raiseWindow(window: KWinWindow): void;
    readonly windowAdded: Signal<[window: KWinWindow]>;
    readonly windowRemoved: Signal<[window: KWinWindow]>;
    /** `null` when no window has the focus. */
    readonly windowActivated: Signal<[window: KWinWindow | null]>;
    /** An output was added or removed, or has another geometry or scale. It does not say which,
     * and the bridge reads every output. */
    readonly screensChanged: Signal;
    /** Some output shows another desktop. It does not say which, and the bridge does not need
     * what KWin passes with it: it reads the desktop of every output. */
    readonly currentDesktopChanged: Signal;
    /** A desktop was created or removed. */
    readonly desktopsChanged: Signal;
}

declare const workspace: Workspace;

/** KWin's enums. */
declare const KWin: {
    /** The `KWin::ClientAreaOption` of the area windows are placed in. */
    readonly PlacementArea: number;
};

/** Calls a D-Bus method. `callback` is called with the reply when the call succeeds, and not at
 * all when it fails. */
declare function callDBus(
    service: string,
    path: string,
    iface: string,
    method: string,
    arg: string,
    callback?: (reply: string) => void,
): void;

/** Writes to KWin's log, under the `kwin_scripting` category. */
declare function print(...values: unknown[]): void;

declare class QTimer {
    interval: number;
    /** Whether the timer fires once per `start`, rather than every `interval`. */
    singleShot: boolean;
    /** True from `start` until a single-shot timer fires or `stop` is called. */
    readonly active: boolean;
    readonly timeout: Signal;
    start(): void;
    stop(): void;
}
