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

interface VirtualDesktop {
    readonly id: string;
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
    minimized: boolean;
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
    readonly desktops: VirtualDesktop[];
    readonly frameGeometryChanged: Signal<[oldGeometry: QRectF]>;
    readonly minimizedChanged: Signal;
    readonly captionChanged: Signal;
    closeWindow(): void;
}

/** The global `workspace` (`KWin::ScriptingWorkspace`). */
interface Workspace {
    readonly screens: Output[];
    /** Setting it activates the window. */
    activeWindow: KWinWindow | null;
    readonly cursorPos: QPointF;
    windowList(): KWinWindow[];
    clientArea(option: number, output: Output, desktop: VirtualDesktop): QRectF;
    currentDesktopForScreen(output: Output): VirtualDesktop;
    raiseWindow(window: KWinWindow): void;
    readonly windowAdded: Signal<[window: KWinWindow]>;
    readonly windowRemoved: Signal<[window: KWinWindow]>;
    /** `null` when no window has the focus. */
    readonly windowActivated: Signal<[window: KWinWindow | null]>;
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
    readonly timeout: Signal;
    start(): void;
}
