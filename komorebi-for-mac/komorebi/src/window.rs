use crate::AccessibilityObserver;
use crate::AccessibilityUiElement;
use crate::FLOATING_APPLICATIONS;
use crate::FLOATING_WINDOW_TOGGLE_ASPECT_RATIO;
use crate::IGNORE_IDENTIFIERS;
use crate::LibraryError;
use crate::MANAGE_IDENTIFIERS;
use crate::PERMAIGNORE_CLASSES;
use crate::REGEX_IDENTIFIERS;
use crate::TABBED_APPLICATIONS;
use crate::TITLELESS_APPLICATIONS;
use crate::WINDOW_RESTORE_POSITIONS;
use crate::accessibility::AccessibilityApi;
use crate::accessibility::action_constants::kAXPressAction;
use crate::accessibility::attribute_constants::kAXCloseButtonAttribute;
use crate::accessibility::attribute_constants::kAXFocusedAttribute;
use crate::accessibility::attribute_constants::kAXMainAttribute;
use crate::accessibility::attribute_constants::kAXMinimizedAttribute;
use crate::accessibility::attribute_constants::kAXParentAttribute;
use crate::accessibility::attribute_constants::kAXPositionAttribute;
use crate::accessibility::attribute_constants::kAXRoleAttribute;
use crate::accessibility::attribute_constants::kAXSizeAttribute;
use crate::accessibility::attribute_constants::kAXSubroleAttribute;
use crate::accessibility::attribute_constants::kAXTitleAttribute;
use crate::accessibility::error::AccessibilityCustomError;
use crate::accessibility::error::AccessibilityError;
use crate::accessibility::notification_constants::AccessibilityNotification;
use crate::accessibility::notification_constants::kAXTitleChangedNotification;
use crate::accessibility::notification_constants::kAXWindowDeminiaturizedNotification;
use crate::accessibility::notification_constants::kAXWindowMiniaturizedNotification;
use crate::accessibility::notification_constants::kAXWindowMovedNotification;
use crate::accessibility::notification_constants::kAXWindowResizedNotification;
use crate::animation;
use crate::animation::animator;
use crate::animation::animator::Journey;
use crate::animation::animator::WindowHandle;
use crate::animation::prefix::AnimationPrefix;
use crate::animation::step::MotionSpec;
use crate::animation::tracks::Verdict;
use crate::application::Application;
use crate::arrival;
use crate::cf_dictionary_value;
use crate::core::ApplicationIdentifier;
use crate::core::Rect;
use crate::core::WindowHidingPosition;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core_graphics::CoreGraphicsApi;
use crate::hidden_frame;
use crate::is_visible_on_another_display;
use crate::macos_api::MacosApi;
use crate::reaper;
use crate::reaper::ReaperNotification;
use crate::scrolling_edge_sliver;
use crate::skylight;
use crate::window_manager_event::SystemNotification;
use crate::window_manager_event::WindowManagerEvent;
use crate::window_manager_event_listener;
use color_eyre::eyre;
use objc2::__framework_prelude::Retained;
use objc2_app_kit::NSApplicationActivationOptions;
use objc2_app_kit::NSRunningApplication;
use objc2_application_services::AXObserver;
use objc2_application_services::AXUIElement;
use objc2_application_services::AXValueType;
use objc2_core_foundation::CFBoolean;
use objc2_core_foundation::CFDictionary;
use objc2_core_foundation::CFNumber;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFRunLoop;
use objc2_core_foundation::CFString;
use objc2_core_foundation::CGFloat;
use objc2_core_foundation::CGPoint;
use objc2_core_foundation::CGRect;
use objc2_core_foundation::CGSize;
use objc2_core_graphics::kCGWindowAlpha;
use objc2_core_graphics::kCGWindowBounds;
use objc2_core_graphics::kCGWindowName;
use objc2_core_graphics::kCGWindowOwnerName;
use objc2_core_graphics::kCGWindowOwnerPID;
use objc2_foundation::NSBundle;
use objc2_foundation::NSString;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use serde::ser::SerializeStruct;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ffi::c_void;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::str::FromStr;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use strum::Display;
use strum::EnumString;
use tracing::instrument;

/// Where a window that has scrolled out of view was parked
#[derive(Debug, Clone, Copy, PartialEq)]
struct Parked {
    /// The side of the display that it left by
    position: WindowHidingPosition,
    /// The display that it was parked on, which is the display of the monitor that owns the window,
    /// and which is not something to work out again from where the window is, since a window that
    /// is parked past the edge of a display is not on any display
    screen: CGRect,
}

lazy_static::lazy_static! {
    /// The windows that have scrolled out of view and been parked, and where they were parked
    ///
    /// A window in here is known to be out of the way without having to ask it, and is brought
    /// back in from the edge it left by rather than from the corner it was parked in.
    static ref PARKED_WINDOWS: parking_lot::Mutex<HashMap<u32, Parked>> =
        parking_lot::Mutex::new(HashMap::new());
}

/// How long, in seconds, to wait for a window to say how big it is after it was resized
const ARRIVAL_TIMEOUT: f32 = 1.0;

/// The window that somebody is dragging by an edge while the windows next to it follow
///
/// Nothing is dragged while the mouse button is up, so a button that went up without anything
/// saying so, which is what a tap that was turned off does, cannot leave the window of a drag that
/// is long over being treated as one that is going on.
#[derive(Debug, Default)]
struct Drag {
    /// The id of the window, or nought for none, which is no id that a window has
    window: AtomicU32,
}

impl Drag {
    fn begin(&self, window_id: u32) {
        self.window.store(window_id, Ordering::SeqCst);
    }

    fn end(&self) {
        self.window.store(0, Ordering::SeqCst);
    }

    /// The window that is being dragged, if there is one, given a way to find out whether the
    /// button is down, which is only asked if there is a drag to ask about: this is for every
    /// move and resize that any window says.
    ///
    /// With the button up there is nothing being dragged, and nothing is going to say so, so it is
    /// forgotten here.
    fn window(&self, mouse_is_down: impl FnOnce() -> bool) -> Option<u32> {
        let window_id = self.window.load(Ordering::SeqCst);

        if window_id == 0 {
            return None;
        }

        if !mouse_is_down() {
            let _ = self
                .window
                .compare_exchange(window_id, 0, Ordering::SeqCst, Ordering::SeqCst);

            return None;
        }

        Some(window_id)
    }
}

static DRAG: Drag = Drag {
    window: AtomicU32::new(0),
};

/// How windows are put where a layout says while the windows next to one that is dragged follow it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Snap {
    /// The way it always is
    Off,
    /// For a step of the drag of the window with this id
    Step(u32),
    /// For the end of a drag that was followed
    End,
}

lazy_static::lazy_static! {
    static ref SNAP: parking_lot::Mutex<Snap> = parking_lot::Mutex::new(Snap::Off);

    /// Where every window was last sent in the steps of the drag that is going on, so that a window
    /// that would be sent where it is already is left alone
    static ref SENT: parking_lot::Mutex<HashMap<u32, Rect>> =
        parking_lot::Mutex::new(HashMap::new());
}

/// Whether a window has scrolled out of view and been parked, which is somewhere that the layout
/// did not put it
pub(crate) fn is_parked(window_id: u32) -> bool {
    PARKED_WINDOWS.lock().contains_key(&window_id)
}

fn snap() -> Snap {
    *SNAP.lock()
}

/// Lays out what is laid out while it is alive straight into place and not by animation, as the
/// windows next to one that is dragged follow it, and are not to be seen catching up.
pub struct Snapping;

impl Snapping {
    /// For a step of the drag of the window `dragged`, which is left to the person dragging it, and
    /// which does not wait to be told what the windows turned out to be, nor send a window where it
    /// was sent last.
    pub fn step(dragged: u32) -> Self {
        *SNAP.lock() = Snap::Step(dragged);
        Self
    }

    /// For a step of a pan of the strip of the Scrolling layout, in which no window is being
    /// dragged, and so every one of them is laid out, straight into place, and left alone only
    /// if it is where it was sent last.
    pub fn pan() -> Self {
        // nought is the id of no window
        Self::step(0)
    }

    /// For the end of a drag that was followed: the windows are a few pixels from where the end of
    /// the drag puts them, which is not worth animating, and every one of them is sent there.
    pub fn end() -> Self {
        *SNAP.lock() = Snap::End;
        Self
    }
}

impl Drop for Snapping {
    fn drop(&mut self) {
        *SNAP.lock() = Snap::Off;
    }
}

/// Whether the windows are being laid out for a step of a drag, in which what a window turned out to
/// be is not asked, since a window that has not settled would say something that is not true.
pub fn is_stepping() -> bool {
    matches!(snap(), Snap::Step(_))
}

/// Somebody has taken hold of the edge of a window, and the windows next to it follow it from
/// here until [`live_resize_ends`].
pub fn live_resize_begins(window_id: u32) {
    SENT.lock().clear();
    DRAG.begin(window_id);
}

/// A run of steps that is not the drag of a window begins, as the fingers of a pan land: where
/// windows were sent by the last is not where they are by now.
pub fn steps_begin() {
    SENT.lock().clear();
}

/// The run of steps is over, and the layout that follows it sends every window where it goes.
pub fn steps_end() {
    SENT.lock().clear();
}

/// The drag is over.
pub fn live_resize_ends() {
    DRAG.end();
    SENT.lock().clear();
}

/// The window that somebody is dragging by an edge while the windows next to it follow, which is
/// none unless the button is down.
pub fn dragged_window() -> Option<u32> {
    DRAG.window(MacosApi::left_mouse_button_is_pressed)
}

const NOTIFICATIONS: &[&str] = &[
    kAXWindowMiniaturizedNotification,
    kAXWindowDeminiaturizedNotification,
    kAXWindowMovedNotification,
    kAXWindowResizedNotification,
    kAXTitleChangedNotification,
];

#[instrument(skip_all)]
unsafe extern "C-unwind" fn window_observer_callback(
    _observer: NonNull<AXObserver>,
    element: NonNull<AXUIElement>,
    notification: NonNull<CFString>,
    _context: *mut c_void,
) {
    unsafe {
        let window_id = AccessibilityApi::window_id(element.as_ref()).ok();
        let notification = AccessibilityNotification::from_str(&notification.as_ref().to_string());

        let mut process_id = 0;
        element.as_ref().pid(NonNull::from_mut(&mut process_id));

        // the moves and resizes the animator makes are reported back to us like any others, and
        // must not be mistaken for somebody moving a window
        if let Ok(
            moved_or_resized @ (AccessibilityNotification::AXWindowMoved
            | AccessibilityNotification::AXWindowResized),
        ) = notification
            && let Some(window_id) = window_id
        {
            // While an edge is dragged the windows next to it are laid out again and again, and
            // every one of them says so, with the mouse button down: it would be taken for somebody
            // dragging that window. What is said by the window that is being dragged is a step, and
            // nothing is asked of its app to say so, since the app is busy with the drag.
            if let Some(dragged) = dragged_window() {
                if window_id == dragged {
                    window_manager_event_listener::send_notification(
                        WindowManagerEvent::ResizeStep(
                            SystemNotification::Accessibility(moved_or_resized),
                            process_id,
                            window_id,
                        ),
                    );
                }

                return;
            }

            let mouse_is_down = MacosApi::left_mouse_button_is_pressed();

            let verdict =
                animator::notification_verdict(window_id, process_id, mouse_is_down, || {
                    MacosApi::window_rect(element.as_ref()).ok().map(Rect::from)
                });

            if verdict == Verdict::Suppress {
                return;
            }

            // an app that will not stop reporting must not be allowed to fill the event channel
            if !animator::admit_notification(
                window_id,
                process_id,
                moved_or_resized == AccessibilityNotification::AXWindowResized,
                mouse_is_down,
            ) {
                return;
            }
        }

        let name =
            AccessibilityApi::copy_attribute_value::<CFString>(element.as_ref(), kAXTitleAttribute)
                .map(|s| s.to_string());

        if let Some(name) = name
            && !name.is_empty()
            && let Ok(notification) = notification
            && let Some(event) = WindowManagerEvent::from_system_notification(
                SystemNotification::Accessibility(notification),
                process_id,
                window_id,
            )
        {
            tracing::debug!(
                "notification: {}, process: {process_id}, name: \"{name}\"",
                notification,
            );

            window_manager_event_listener::send_notification(event);
        }
    }
}

#[derive(Debug, Default)]
pub struct WindowInfo {
    name: Option<String>,
    pub owner_pid: i32,
    owner_name: String,
    alpha: f32,
    bounds: WindowBounds,
}

impl WindowInfo {
    pub fn new(entry: NonNull<CFDictionary>) -> Self {
        WindowInfo::from(unsafe { entry.as_ref() })
    }
}

#[derive(Debug, Default)]
#[allow(unused)]
pub struct ValidWindowInfo {
    pub name: String,
    pub owner_pid: i32,
    owner_name: String,
    alpha: f32,
    pub bounds: WindowBounds,
}

impl WindowInfo {
    pub fn validated(self) -> Option<ValidWindowInfo> {
        if let Some(name) = self.name
            && self.alpha != 0.0
            && self.bounds.y != 0.0
            && self.bounds.height != 0.0
            && !name.is_empty()
        {
            return Some(ValidWindowInfo {
                name,
                owner_pid: self.owner_pid,
                owner_name: self.owner_name,
                alpha: self.alpha,
                bounds: self.bounds,
            });
        }

        None
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Window {
    pub id: u32,
    #[serde(skip_deserializing)]
    pub element: AccessibilityUiElement,
    #[serde(skip_deserializing)]
    pub application: Application,
    #[serde(skip_deserializing)]
    observer: AccessibilityObserver,
    pub details: Option<WindowDetails>,
}

#[cfg(test)]
impl From<u32> for Window {
    fn from(id: u32) -> Self {
        Self {
            id,
            element: Default::default(),
            application: Default::default(),
            observer: Default::default(),
            details: None,
        }
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowDetails {
    pub title: String,
    pub exe: String,
    pub role: String,
    pub subrole: String,
    pub icon_path: PathBuf,
}

impl From<&Window> for WindowDetails {
    fn from(value: &Window) -> Self {
        Self {
            title: value.title().unwrap_or_default(),
            exe: value.exe().unwrap_or_default(),
            role: value.role().unwrap_or_default(),
            subrole: value.subrole().unwrap_or_default(),
            icon_path: value.icon_path().unwrap_or_default(),
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // this gets called when a cloned Window is dropped, so we need to make sure it only
        // invalidates the observer if the Window is no longer open
        if !self.is_valid() {
            tracing::info!(
                "invalidating window observer for {}",
                self.title()
                    .unwrap_or_else(|| String::from("<NO TITLE FOUND>"))
            );

            // Messages wouldn't close on click which is annoying, this handles that edge case
            // for now - hopefully it doesn't break anything else
            reaper::send_notification(ReaperNotification::InvalidWindow(self.id));

            // make sure the observer gets removed from any run loops
            AccessibilityApi::invalidate_observer(&self.observer);
        }
    }
}

impl Display for Window {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut display = format!(
            "(window_id: {}, process_id: {}",
            self.id, self.application.process_id
        );

        if let Some(title) = self.title() {
            write!(display, ", title: {title}")?;
        }

        if let Some(exe) = self.exe() {
            write!(display, ", exe: {exe}")?;
        }

        if let Some(role) = self.role() {
            write!(display, ", role: {role}")?;
        }

        if let Some(subrole) = self.subrole() {
            write!(display, ", subrole: {subrole}")?;
        }

        write!(display, ")")?;

        write!(f, "{display}")
    }
}

impl Serialize for Window {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Window", 6)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field(
            "rect",
            &Rect::from(MacosApi::window_rect(&self.element).unwrap_or_default()),
        )?;
        state.serialize_field("details", &WindowDetails::from(self))?;
        state.end()
    }
}

#[cfg(feature = "schemars")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
struct SerializedWindow {
    id: u32,
    title: String,
    exe: String,
    role: String,
    subrole: String,
    rect: Rect,
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Window {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Window")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        <SerializedWindow as schemars::JsonSchema>::json_schema(generator)
    }
}

impl Window {
    pub fn new(
        element: CFRetained<AXUIElement>,
        application: Application,
    ) -> Result<Self, AccessibilityError> {
        let observer = AccessibilityApi::create_observer(
            application.process_id,
            Some(window_observer_callback),
        )?;

        Ok(Self {
            id: AccessibilityApi::window_id(&element)?,
            element: AccessibilityUiElement(element),
            application,
            observer: AccessibilityObserver(Some(observer)),
            details: None,
        })
    }

    pub fn is_valid(&self) -> bool {
        AccessibilityApi::copy_attribute_names(&self.element).is_some()
    }

    pub fn is_focused(&self) -> bool {
        MacosApi::foreground_window_id().unwrap_or_default() == self.id
    }

    #[tracing::instrument(skip_all)]
    pub fn observe(
        &self,
        run_loop: &CFRunLoop,
        refcon: Option<*mut c_void>,
    ) -> Result<(), AccessibilityError> {
        tracing::info!("registering observer for {self}");

        AccessibilityApi::add_observer_to_run_loop(
            &self.observer,
            &self.element,
            NOTIFICATIONS,
            run_loop,
            refcon,
        )
    }

    /// Hide the window in the corner of the display that it is on
    ///
    /// Prefer [`Window::hide_on`] when the display is known, since a window that hangs over the
    /// edge of a display is not always on the display that the lookup by position finds.
    #[tracing::instrument(skip_all)]
    pub fn hide(
        &mut self,
        hiding_position: WindowHidingPosition,
    ) -> Result<(), AccessibilityError> {
        self.hide_on(hiding_position, None)
    }

    /// Hide the window in the corner of `screen`, the bounds of the display that it belongs to
    ///
    /// The display is the display of the monitor that has the window, and it has to come from
    /// there for a window that overhangs the left edge of its display, where it is not on any
    /// display, or one that overhangs the edge shared with another display, where it is on two. If
    /// it is not given it is looked up from where the window is.
    #[tracing::instrument(skip_all)]
    pub fn hide_on(
        &mut self,
        hiding_position: WindowHidingPosition,
        screen: Option<CGRect>,
    ) -> Result<(), AccessibilityError> {
        // a window that is on its way somewhere is restored to where it was going, not to wherever
        // it happened to be when it was hidden
        let rect = match animator::take_over(self.id) {
            Some(target) => CGRect::from(target),
            None => MacosApi::window_rect(&self.element)?,
        };

        let mut window_restore_positions = WINDOW_RESTORE_POSITIONS.lock();
        if let Entry::Vacant(entry) = window_restore_positions.entry(self.id) {
            entry.insert(rect);
        }
        drop(window_restore_positions);

        let Some(monitor_size) =
            screen.or_else(|| CoreGraphicsApi::display_bounds_for_window_rect(rect))
        else {
            // there is a restore point for a window that was not hidden, which is harmless, but
            // a window that is left where it is when it should not be is not
            tracing::warn!(
                "not hiding {}: there is no display to hide it on",
                self.title()
                    .unwrap_or_else(|| String::from("<NO TITLE FOUND>")),
            );

            return Ok(());
        };

        // I don't love this, but it's basically what Aerospace does in lieu of an actual "Hide" API
        let hidden_rect = hidden_frame(hiding_position, monitor_size, rect.size);

        tracing::debug!(
            "hiding {} and setting restore point to {},{}",
            self.title()
                .unwrap_or_else(|| String::from("<NO TITLE FOUND>")),
            rect.origin.x,
            rect.origin.y,
        );

        self.set_point(hidden_rect.origin, true)?;
        self.set_size(hidden_rect.size, true)?;

        Ok(())
    }

    /// Take a window that has scrolled out of view in the Scrolling layout out of the way
    ///
    /// The window comes to rest at the edge of the screen that it left by, with a sliver of it on
    /// screen. It keeps its size and its height up the screen, so bringing it back is a matter of
    /// moving it. With animations on it slides there.
    ///
    /// `screen` is the bounds of the display of the monitor that has the window. A window that is
    /// partly past the left edge of the screen is on no display, so this is not something to find
    /// out from where the window is.
    ///
    /// Where a neighbouring display would show the window at the edge, it is hidden in the corner
    /// of the screen instead, as with [`Window::hide`].
    #[tracing::instrument(skip_all)]
    pub fn hide_scrolling(
        &mut self,
        hiding_position: WindowHidingPosition,
        screen: Option<CGRect>,
    ) -> Result<(), AccessibilityError> {
        // nothing to do for a window that is already out of the way on this side, which keeps
        // every layout update from sending it on the same trip again
        let mut parked_at = PARKED_WINDOWS.lock().get(&self.id).copied();

        // a window that was stopped on its way out of view, by somebody grabbing it or by the app
        // not answering, is not out of the way at all
        if parked_at.is_some() && animator::take_unfinished(self.id) {
            PARKED_WINDOWS.lock().remove(&self.id);
            parked_at = None;
        }

        let parked_on = |screen| {
            Some(Parked {
                position: hiding_position,
                screen,
            })
        };

        if let Some(screen) = screen
            && parked_at == parked_on(screen)
        {
            return Ok(());
        }

        let rect = match animator::target(self.id) {
            Some(target) => CGRect::from(target),
            None => MacosApi::window_rect(&self.element)?,
        };

        let Some(screen) = screen.or_else(|| CoreGraphicsApi::display_bounds_for_window_rect(rect))
        else {
            tracing::warn!(
                "window {} cannot be parked: there is no display to park it on",
                self.id
            );

            return self.hide(hiding_position);
        };

        let parked = Parked {
            position: hiding_position,
            screen,
        };

        if parked_at == Some(parked) {
            return Ok(());
        }

        // a window that has to change sides would be seen sliding across the whole screen, so it
        // goes straight to the other edge
        let motion = match parked_at {
            Some(_) => None,
            // the windows next to an edge that is dragged follow it, and are not seen sliding
            None if snap() != Snap::Off => None,
            None => animation::motion_for(AnimationPrefix::Movement),
        };

        let edge = scrolling_edge_sliver(screen, rect, hiding_position);

        // resting at the edge next to a neighbouring display would be seen there
        if is_visible_on_another_display(edge, screen) {
            tracing::trace!(
                "window {} is parked in the corner of its display, since the edge is next to                  another display",
                self.id
            );

            self.hide_on(hiding_position, Some(screen))?;
            PARKED_WINDOWS.lock().insert(self.id, parked);

            return Ok(());
        }

        tracing::trace!(
            "window {} is parked at the {hiding_position} edge of its display, at {},{}",
            self.id,
            edge.origin.x,
            edge.origin.y,
        );

        if let Entry::Vacant(entry) = WINDOW_RESTORE_POSITIONS.lock().entry(self.id) {
            entry.insert(rect);
        }

        match motion {
            Some(motion) => {
                let handle = WindowHandle {
                    element: self.element.clone(),
                    app: self.application.element().clone(),
                };

                animator::retarget(
                    &Journey {
                        window_id: self.id,
                        pid: self.application.process_id,
                        handle,
                        target: Rect::from(edge),
                        motion,
                        // and it stays where it stops
                        after: None,
                    },
                    || Ok(Rect::from(rect)),
                )?;
            }
            None => self.set_point(edge.origin, true)?,
        }

        PARKED_WINDOWS.lock().insert(self.id, parked);

        Ok(())
    }

    pub fn minimize(&mut self) -> Result<(), AccessibilityError> {
        let cf_boolean = CFBoolean::new(true);
        let value = &**cf_boolean;
        AccessibilityApi::set_attribute_cf_value(&self.element, kAXMinimizedAttribute, value)
    }

    pub fn unminimize(&mut self) -> Result<(), AccessibilityError> {
        let cf_boolean = CFBoolean::new(false);
        let value = &**cf_boolean;
        AccessibilityApi::set_attribute_cf_value(&self.element, kAXMinimizedAttribute, value)
    }

    #[tracing::instrument(skip_all)]
    pub fn restore(&mut self) -> Result<(), AccessibilityError> {
        // waiting for what is on its way to the window is not something to do with a lock held
        animator::take_over(self.id);
        PARKED_WINDOWS.lock().remove(&self.id);

        let mut should_remove_restore_position = false;
        let mut window_restore_positions = WINDOW_RESTORE_POSITIONS.lock();
        if let Some(cg_rect) = window_restore_positions.get(&self.id) {
            tracing::debug!(
                "restoring {:?} to {cg_rect:?}",
                self.title()
                    .unwrap_or_else(|| String::from("<NO TITLE FOUND>"))
            );

            self.set_point(cg_rect.origin, true)?;
            self.set_size(cg_rect.size, true)?;
            should_remove_restore_position = true;
        }

        if should_remove_restore_position {
            window_restore_positions.remove(&self.id);
        }

        Ok(())
    }

    pub fn title(&self) -> Option<String> {
        AccessibilityApi::copy_attribute_value::<CFString>(&self.element, kAXTitleAttribute)
            .map(|s| s.to_string())
    }

    pub fn exe(&self) -> Option<String> {
        self.application.name()
    }

    pub fn bundle_identifier(&self) -> Option<String> {
        if let Ok(Some(identifier)) = self.running_application().map(|app| app.bundleIdentifier()) {
            Some(identifier.to_string())
        } else {
            None
        }
    }

    pub fn bundle_path(&self) -> Option<PathBuf> {
        if let Ok(Some(path)) = self
            .running_application()
            .map(|app| app.bundleURL())
            .map(|url| url.map(|url| url.to_file_path()))
        {
            path
        } else {
            None
        }
    }

    pub fn path(&self) -> Option<PathBuf> {
        if let Ok(Some(path)) = self
            .running_application()
            .map(|app| app.executableURL())
            .map(|ns_url| ns_url.map(|url| url.to_file_path()))
        {
            path
        } else {
            None
        }
    }

    pub fn icon_path(&self) -> Option<PathBuf> {
        if let Some(path) = self.bundle_path()
            && let Some(bundle) =
                NSBundle::bundleWithPath(&NSString::from_str(&path.to_string_lossy()))
            && let Some(icon_file) =
                bundle.objectForInfoDictionaryKey(&NSString::from_str("CFBundleIconFile"))
            && let Ok(icon_name) = icon_file.downcast::<NSString>()
        {
            let mut icon_path = format!("{}/Contents/Resources/{}", path.display(), icon_name);

            if !icon_path.ends_with(".icns") {
                icon_path.push_str(".icns");
            }

            let path = Path::new(&icon_path);

            if path.exists() {
                return Some(PathBuf::from(path));
            }
        }

        None
    }

    pub fn role(&self) -> Option<String> {
        AccessibilityApi::copy_attribute_value::<CFString>(&self.element, kAXRoleAttribute)
            .map(|s| s.to_string())
    }

    pub fn subrole(&self) -> Option<String> {
        AccessibilityApi::copy_attribute_value::<CFString>(&self.element, kAXSubroleAttribute)
            .map(|s| s.to_string())
    }

    fn running_application(&self) -> Result<Retained<NSRunningApplication>, AccessibilityError> {
        NSRunningApplication::runningApplicationWithProcessIdentifier(self.application.process_id)
            .ok_or(AccessibilityError::Custom(
                AccessibilityCustomError::NSRunningApplication(self.application.process_id),
            ))
    }

    pub fn set_position(&self, rect: &Rect) -> Result<(), AccessibilityError> {
        match snap() {
            Snap::Step(dragged) => return self.set_position_for_step(dragged, rect),
            Snap::End => {
                PARKED_WINDOWS.lock().remove(&self.id);
                return self.set_position_direct(rect);
            }
            Snap::Off => {}
        }

        match animation::motion_for(AnimationPrefix::Movement) {
            Some(motion) => {
                self.slide_in_from_edge(rect);
                self.set_position_animated(rect, motion)
            }
            None => {
                // nothing is going to slide it in, so nothing else is going to say that it is back
                PARKED_WINDOWS.lock().remove(&self.id);
                self.set_position_direct(rect)
            }
        }
    }

    /// Find out what the window is after it was written the size of `target` without animating,
    /// and say so if it did not take the width, so that the layout can make room for it.
    ///
    /// With animations on the animator finds out when the window arrives, and says so itself. A
    /// window that was not resized is not asked: that costs a round trip to the app, and what
    /// the window turned out to be is not going to be any different from last time.
    pub fn note_arrival(&self, target: &Rect, resized: bool) {
        // for a step of a drag it is not asked, however the window got there: a round trip to every
        // window that was resized, each of which can take as long as the timeout, is time that the
        // windows next to the edge spend not following it, and a window that has not settled would
        // say that it is wider than it will be
        let animated =
            snap() == Snap::Off && animation::motion_for(AnimationPrefix::Movement).is_some();

        if !resized || animated || is_stepping() {
            return;
        }

        // the window manager is waiting for this, and so is every window after this one
        let _ = AccessibilityApi::set_messaging_timeout(&self.element, ARRIVAL_TIMEOUT);

        match MacosApi::window_rect(&self.element) {
            Ok(actual) => arrival::report(self.id, *target, Rect::from(actual)),
            Err(error) => {
                tracing::debug!(
                    "could not read back the size of window {}: {error}",
                    self.id
                )
            }
        }
    }

    /// Bring a parked window back to the edge of the screen it left by, ready to slide in from there
    ///
    /// Without this it would fly in from the corner it was parked in. The display is the one that
    /// it was parked on, and not the one at its target, since a target that overhangs the left
    /// edge of the screen is on no display.
    fn slide_in_from_edge(&self, target: &Rect) {
        let Some(Parked {
            position: hiding_position,
            screen,
        }) = PARKED_WINDOWS.lock().remove(&self.id)
        else {
            return;
        };

        if animator::is_animating(self.id) {
            return;
        }

        let Ok(current) = MacosApi::window_rect(&self.element) else {
            return;
        };

        // the corner it was parked in is out of sight, so it comes back at the height it is going to
        let mut level = current;
        level.origin.y = target.top as CGFloat;

        let edge = scrolling_edge_sliver(screen, level, hiding_position);

        if is_visible_on_another_display(edge, screen) {
            return;
        }

        if let Err(error) = self.set_point(edge.origin, true) {
            tracing::debug!(
                "could not bring window {} back to the edge: {error}",
                self.id
            );
        }
    }

    /// Put the window where a step of the drag of the window `dragged` says that it goes
    ///
    /// The window that is being dragged is where the person dragging it has it. Any other goes
    /// straight there, without an animation to catch up with the next step, and not at all if it
    /// is where it was sent last.
    fn set_position_for_step(&self, dragged: u32, rect: &Rect) -> Result<(), AccessibilityError> {
        if self.id == dragged {
            tracing::debug!("window {} is left alone: it is being dragged", self.id);

            return Ok(());
        }

        if SENT.lock().insert(self.id, *rect) == Some(*rect) {
            tracing::debug!(
                "window {} is left alone: it is where it was sent last, {rect:?}",
                self.id
            );

            return Ok(());
        }

        tracing::debug!("window {} is sent to {rect:?}", self.id);

        // nothing is going to slide it in, so nothing else is going to say that it is back
        PARKED_WINDOWS.lock().remove(&self.id);
        let result = self.set_position_direct(rect);

        // a write that failed or timed out did not put the window anywhere, so it was not sent
        // there, and the next step that says the same sends it again
        if result.is_err() {
            let mut sent = SENT.lock();

            if sent.get(&self.id) == Some(rect) {
                sent.remove(&self.id);
            }
        }

        result
    }

    fn set_position_direct(&self, rect: &Rect) -> Result<(), AccessibilityError> {
        self.set_point(
            CGPoint::new(rect.left as CGFloat, rect.top as CGFloat),
            true,
        )?;
        self.set_size(
            CGSize::new(rect.right as CGFloat, rect.bottom as CGFloat),
            true,
        )
    }

    fn set_position_animated(
        &self,
        target_rect: &Rect,
        motion: MotionSpec,
    ) -> Result<(), AccessibilityError> {
        let handle = WindowHandle {
            element: self.element.clone(),
            app: self.application.element().clone(),
        };

        animator::retarget(
            &Journey {
                window_id: self.id,
                pid: self.application.process_id,
                handle,
                target: *target_rect,
                motion,
                after: None,
            },
            || MacosApi::window_rect(&self.element).map(Rect::from),
        )
    }

    pub fn focus(&self, mouse_follows_focus: bool) -> Result<(), LibraryError> {
        // Activating the application is turned away by the window server as an expired request once
        // the user has typed or clicked since komorebi started, and the application in front stays
        // there, so the window server is asked directly first. That is only for as long as it
        // works: when it does not, this is the way it always was.
        if !skylight::set_front_process(self.application.process_id, self.id) {
            match self.running_application() {
                Ok(running_application) => {
                    running_application
                        .activateWithOptions(NSApplicationActivationOptions::empty());
                }
                Err(error) => {
                    tracing::warn!(
                        "failed to get running application for {} ({:?}): {error}",
                        self.application.process_id,
                        self.application.name()
                    );
                }
            }
        }

        let cf_boolean = CFBoolean::new(true);
        let value = &**cf_boolean;

        // For tabbed applications, use the current main window element instead of
        // the stored element, which may point to a different tab
        let tabbed_applications = TABBED_APPLICATIONS.lock();
        let is_tabbed = tabbed_applications.contains(&self.application.name().unwrap_or_default());
        drop(tabbed_applications);

        let element_to_focus = if is_tabbed {
            // For tabbed apps, we still need to check if the stored element is valid
            // If valid, use it directly (allows focusing specific windows)
            // If invalid (tab was closed), fall back to main_window() for the active tab
            match AccessibilityApi::window_id(&self.element.0) {
                Ok(_) => self.element.clone(), // Element is valid, use it
                Err(_) => {
                    // Element is invalid (tab closed), get current main window
                    self.application
                        .main_window()
                        .map(AccessibilityUiElement)
                        .unwrap_or_else(|| self.element.clone())
                }
            }
        } else {
            self.element.clone()
        };

        AccessibilityApi::set_attribute_cf_value(&element_to_focus, kAXMainAttribute, value)?;

        if mouse_follows_focus {
            MacosApi::center_cursor_in_rect(&MacosApi::window_rect(&element_to_focus)?.into())?
        }

        Ok(())
    }

    pub fn raise(&self) -> Result<(), AccessibilityError> {
        let cf_boolean = CFBoolean::new(true);
        let value = &**cf_boolean;
        AccessibilityApi::set_attribute_cf_value(&self.element, kAXMainAttribute, value)?;
        AccessibilityApi::set_attribute_cf_value(&self.element, kAXFocusedAttribute, value)
    }

    pub fn set_point(&self, point: CGPoint, should_reap: bool) -> Result<(), AccessibilityError> {
        // anything that is animating this window has to stop before it is written to directly,
        // and wherever it ends up it is no longer where it was parked
        animator::take_over(self.id);
        PARKED_WINDOWS.lock().remove(&self.id);

        let result = AccessibilityApi::set_attribute_ax_value(
            &self.element,
            kAXPositionAttribute,
            AXValueType::CGPoint,
            point,
        );

        if should_reap {
            reaper::notify_on_error(self, result)
        } else {
            result
        }
    }

    pub fn set_size(&self, size: CGSize, should_reap: bool) -> Result<(), AccessibilityError> {
        animator::take_over(self.id);
        PARKED_WINDOWS.lock().remove(&self.id);

        let result = AccessibilityApi::set_attribute_ax_value(
            &self.element,
            kAXSizeAttribute,
            AXValueType::CGSize,
            size,
        );

        if should_reap {
            reaper::notify_on_error(self, result)
        } else {
            result
        }
    }

    pub fn center(&mut self, work_area: &Rect, resize: bool) -> Result<(), AccessibilityError> {
        let (target_width, target_height) = if resize {
            let (aspect_ratio_width, aspect_ratio_height) = FLOATING_WINDOW_TOGGLE_ASPECT_RATIO
                .lock()
                .width_and_height();
            let target_height = work_area.bottom / 2;
            let target_width = (target_height * aspect_ratio_width) / aspect_ratio_height;
            (target_width, target_height)
        } else {
            let current_rect = Rect::from(MacosApi::window_rect(&self.element)?);
            (current_rect.right, current_rect.bottom)
        };

        let x = work_area.left + ((work_area.right - target_width) / 2);
        let y = work_area.top + ((work_area.bottom - target_height) / 2);

        self.set_position(&Rect {
            left: x,
            top: y,
            right: target_width,
            bottom: target_height,
        })
    }

    pub fn move_to_area(
        &self,
        current_area: &Rect,
        target_area: &Rect,
    ) -> Result<(), AccessibilityError> {
        let current_rect = Rect::from(MacosApi::window_rect(&self.element)?);
        let x_diff = target_area.left - current_area.left;
        let y_diff = target_area.top - current_area.top;
        let x_ratio = f32::abs((target_area.right as f32) / (current_area.right as f32));
        let y_ratio = f32::abs((target_area.bottom as f32) / (current_area.bottom as f32));
        let window_relative_x = current_rect.left - current_area.left;
        let window_relative_y = current_rect.top - current_area.top;
        let corrected_relative_x = (window_relative_x as f32 * x_ratio) as i32;
        let corrected_relative_y = (window_relative_y as f32 * y_ratio) as i32;
        let window_x = current_area.left + corrected_relative_x;
        let window_y = current_area.top + corrected_relative_y;
        let left = x_diff + window_x;
        let top = y_diff + window_y;

        let corrected_width = (current_rect.right as f32 * x_ratio) as i32;
        let corrected_height = (current_rect.bottom as f32 * y_ratio) as i32;

        let new_rect = Rect {
            left,
            top,
            right: corrected_width,
            bottom: corrected_height,
        };

        // TODO: figure out what to do about maximized windows on macOS
        // let is_maximized = &new_rect == target_area;
        // if is_maximized {
        //     windows_api::WindowsApi::unmaximize_window(self.hwnd);
        //     let animation_enabled = ANIMATION_ENABLED_PER_ANIMATION.lock();
        //     let move_enabled = animation_enabled
        //         .get(&MovementRenderDispatcher::PREFIX)
        //         .is_some_and(|v| *v);
        //     drop(animation_enabled);
        //
        //     if move_enabled || ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst) {
        //         let anim_count = ANIMATION_MANAGER
        //             .lock()
        //             .count_in_progress(MovementRenderDispatcher::PREFIX);
        //         self.set_position(&new_rect, true)?;
        //         let hwnd = self.hwnd;
        //         // Wait for the animation to finish before maximizing the window again, otherwise
        //         // we would be maximizing the window on the current monitor anyway
        //         thread::spawn(move || {
        //             let mut new_anim_count = ANIMATION_MANAGER
        //                 .lock()
        //                 .count_in_progress(MovementRenderDispatcher::PREFIX);
        //             let mut max_wait = 2000; // Max waiting time. No one will be using an animation longer than 2s, right? RIGHT??? WHY?
        //             while new_anim_count > anim_count && max_wait > 0 {
        //                 thread::sleep(Duration::from_millis(10));
        //                 new_anim_count = ANIMATION_MANAGER
        //                     .lock()
        //                     .count_in_progress(MovementRenderDispatcher::PREFIX);
        //                 max_wait -= 1;
        //             }
        //             windows_api::WindowsApi::maximize_window(hwnd);
        //         });
        //     } else {
        //         self.set_position(&new_rect, true)?;
        //         windows_api::WindowsApi::maximize_window(self.hwnd);
        //     }
        // } else {
        self.set_position(&new_rect)?;
        // }

        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub fn should_manage(
        &self,
        event: Option<WindowManagerEvent>,
        debug: &mut RuleDebug,
    ) -> eyre::Result<bool> {
        if !self.is_valid() {
            return Ok(false);
        }

        debug.is_window = true;

        // let rect = Rect::from(MacosApi::window_rect(&self.element).unwrap_or_default());
        //
        // if rect.right < MINIMUM_WIDTH.load(Ordering::SeqCst) {
        //     return Ok(false);
        // }
        //
        // debug.has_minimum_width = true;
        //
        // if rect.bottom < MINIMUM_HEIGHT.load(Ordering::SeqCst) {
        //     return Ok(false);
        // }
        //
        // debug.has_minimum_height = true;

        match self.title() {
            None => {
                if TITLELESS_APPLICATIONS
                    .lock()
                    .contains(&self.exe().unwrap_or_default())
                {
                    debug.matches_titleless_applications = self.exe();
                } else {
                    return Ok(false);
                }
            }
            Some(title) => {
                // Raycast is dumb and reports an empty string as a title
                if title.is_empty() {
                    if TITLELESS_APPLICATIONS
                        .lock()
                        .contains(&self.exe().unwrap_or_default())
                    {
                        debug.matches_titleless_applications = self.exe();
                    } else {
                        return Ok(false);
                    }
                } else {
                    debug.has_title = true;
                }
            }
        }

        // let is_cloaked = self.is_cloaked().unwrap_or_default();
        //
        // debug.is_cloaked = is_cloaked;

        // let mut allow_cloaked = false;

        // if let Some(event) = event {
        //     if matches!(
        //         event,
        //         WindowManagerEvent::Hide(_, _) | WindowManagerEvent::Cloak(_, _)
        //     ) {
        //         allow_cloaked = true;
        //     }
        // }

        // debug.allow_cloaked = allow_cloaked;

        // match (allow_cloaked, is_cloaked) {
        //     // If allowing cloaked windows, we don't need to check the cloaked status
        //     (true, _) |
        //     // If not allowing cloaked windows, we need to ensure the window is not cloaked
        //     (false, false) => {
        let title = if debug.matches_titleless_applications.is_some() {
            self.exe()
        } else {
            self.title()
        };

        if let (Some(title), Some(exe_name), Some(role), Some(subrole), Some(path)) =
            (title, self.exe(), self.role(), self.subrole(), self.path())
        {
            debug.title = Some(title.clone());
            debug.exe_name = Some(exe_name.clone());
            debug.role = Some(role.clone());
            debug.subrole = Some(subrole.clone());
            debug.path = Some(path.to_string_lossy().to_string());
            // calls for styles can fail quite often for events with windows that aren't really "windows"
            // since we have moved up calls of should_manage to the beginning of the process_event handler,
            // we should handle failures here gracefully to be able to continue the execution of process_event
            // if let (Ok(style), Ok(ex_style)) = (&self.style(), &self.ex_style()) {
            //     debug.window_style = Some(*style);
            //     debug.extended_window_style = Some(*ex_style);
            let eligible = window_is_eligible(
                self.id,
                &title,
                &exe_name,
                &[&role, &subrole],
                &path.to_string_lossy(),
                event,
                debug,
            );
            // debug.should_manage = eligible;
            return Ok(eligible);
            // }
        }
        // }
        //     _ => {}
        // }
        Ok(false)
    }
}

pub struct AdhocWindow;

impl AdhocWindow {
    pub fn process_id(element: &CFRetained<AXUIElement>) -> Option<i32> {
        let mut process_id = 0;

        unsafe {
            element.pid(NonNull::from_mut(&mut process_id));
        }

        if process_id != 0 {
            Some(process_id)
        } else {
            None
        }
    }

    pub fn exe(element: &CFRetained<AXUIElement>) -> Option<String> {
        let parent =
            AccessibilityApi::copy_attribute_value::<AXUIElement>(element, kAXParentAttribute)?;

        AccessibilityApi::copy_attribute_value::<CFString>(&parent, kAXTitleAttribute)
            .map(|s| s.to_string())
    }

    pub fn role(element: &CFRetained<AXUIElement>) -> Option<String> {
        AccessibilityApi::copy_attribute_value::<CFString>(element, kAXRoleAttribute)
            .map(|s| s.to_string())
    }

    pub fn subrole(element: &CFRetained<AXUIElement>) -> Option<String> {
        AccessibilityApi::copy_attribute_value::<CFString>(element, kAXSubroleAttribute)
            .map(|s| s.to_string())
    }

    pub fn title(element: &CFRetained<AXUIElement>) -> Option<String> {
        AccessibilityApi::copy_attribute_value::<CFString>(element, kAXTitleAttribute)
            .map(|s| s.to_string())
    }

    pub fn minimize(element: &CFRetained<AXUIElement>) -> Result<(), AccessibilityError> {
        let cf_boolean = CFBoolean::new(true);
        let value = &**cf_boolean;
        AccessibilityApi::set_attribute_cf_value(element, kAXMinimizedAttribute, value)
    }

    pub fn close(element: &CFRetained<AXUIElement>) -> Result<(), AccessibilityError> {
        if let Some(close_button) =
            AccessibilityApi::copy_attribute_value::<AXUIElement>(element, kAXCloseButtonAttribute)
        {
            AccessibilityApi::perform_action(&close_button, kAXPressAction)
        } else {
            Ok(())
        }
    }

    #[tracing::instrument(skip_all)]
    pub fn hide(
        id: u32,
        element: &CFRetained<AXUIElement>,
        hiding_position: WindowHidingPosition,
    ) -> Result<(), AccessibilityError> {
        let mut window_restore_positions = WINDOW_RESTORE_POSITIONS.lock();
        if let Entry::Vacant(entry) = window_restore_positions.entry(id) {
            let rect = MacosApi::window_rect(element)?;
            if let Some(monitor_size) = CoreGraphicsApi::display_bounds_for_window_rect(rect) {
                entry.insert(rect);
                drop(window_restore_positions);

                // I don't love this, but it's basically what Aerospace does in lieu of an actual "Hide" API
                let hidden_rect = hidden_frame(hiding_position, monitor_size, rect.size);

                tracing::debug!(
                    "hiding window with id {id} and setting restore point to {},{}",
                    rect.origin.x,
                    rect.origin.y,
                );

                AccessibilityApi::set_attribute_ax_value(
                    element,
                    kAXPositionAttribute,
                    AXValueType::CGPoint,
                    hidden_rect.origin,
                )?;

                AccessibilityApi::set_attribute_ax_value(
                    element,
                    kAXSizeAttribute,
                    AXValueType::CGSize,
                    hidden_rect.size,
                )?;
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub fn restore(id: u32, element: &CFRetained<AXUIElement>) -> Result<(), AccessibilityError> {
        let mut should_remove_restore_position = false;
        let mut window_restore_positions = WINDOW_RESTORE_POSITIONS.lock();
        if let Some(cg_rect) = window_restore_positions.get(&id) {
            tracing::debug!("restoring window with id {id} to {cg_rect:?}",);

            AccessibilityApi::set_attribute_ax_value(
                element,
                kAXPositionAttribute,
                AXValueType::CGPoint,
                cg_rect.origin,
            )?;

            AccessibilityApi::set_attribute_ax_value(
                element,
                kAXSizeAttribute,
                AXValueType::CGSize,
                cg_rect.size,
            )?;

            should_remove_restore_position = true;
        }

        if should_remove_restore_position {
            window_restore_positions.remove(&id);
        }

        Ok(())
    }

    pub fn raise(element: &CFRetained<AXUIElement>) -> Result<(), AccessibilityError> {
        let cf_boolean = CFBoolean::new(true);
        let value = &**cf_boolean;
        AccessibilityApi::set_attribute_cf_value(element, kAXMainAttribute, value)?;
        AccessibilityApi::set_attribute_cf_value(element, kAXFocusedAttribute, value)
    }
}

impl From<&CFDictionary> for WindowInfo {
    fn from(value: &CFDictionary) -> Self {
        unsafe {
            Self {
                name: cf_dictionary_value::<CFString>(value, kCGWindowName)
                    .map(|s| s.as_ref().to_string())
                    .and_then(|s| if s.is_empty() { None } else { Some(s) }),
                owner_pid: cf_dictionary_value::<CFNumber>(value, kCGWindowOwnerPID)
                    .and_then(|s| s.as_ref().as_i32())
                    .expect("window must have an owner process id"),
                owner_name: cf_dictionary_value::<CFString>(value, kCGWindowOwnerName)
                    .map(|s| s.as_ref().to_string())
                    .expect("window must have an owner name"),
                alpha: cf_dictionary_value::<CFNumber>(value, kCGWindowAlpha)
                    .and_then(|s| s.as_ref().as_f32())
                    .expect("window must have an alpha value"),

                bounds: if let Some(dict) =
                    cf_dictionary_value::<CFDictionary>(value, kCGWindowBounds).as_ref()
                {
                    WindowBounds::from(dict.as_ref())
                } else {
                    Default::default()
                },
            }
        }
    }
}

#[derive(Default, Debug, Copy, Clone)]
pub struct WindowBounds {
    pub height: f32,
    pub width: f32,
    pub x: f32,
    pub y: f32,
}

impl From<&CFDictionary> for WindowBounds {
    fn from(value: &CFDictionary) -> Self {
        unsafe {
            Self {
                height: cf_dictionary_value::<CFNumber>(
                    value,
                    &CFString::from_static_str("Height"),
                )
                .and_then(|val| val.as_ref().as_f32())
                .unwrap_or_default(),
                width: cf_dictionary_value::<CFNumber>(value, &CFString::from_static_str("Width"))
                    .and_then(|val| val.as_ref().as_f32())
                    .unwrap_or_default(),
                x: cf_dictionary_value::<CFNumber>(value, &CFString::from_static_str("X"))
                    .and_then(|val| val.as_ref().as_f32())
                    .unwrap_or_default(),
                y: cf_dictionary_value::<CFNumber>(value, &CFString::from_static_str("Y"))
                    .and_then(|val| val.as_ref().as_f32())
                    .unwrap_or_default(),
            }
        }
    }
}

#[derive(Copy, Clone, Debug, Display, EnumString, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
/// Aspect ratio for temporarily floating windows
pub enum AspectRatio {
    /// A predefined aspect ratio
    #[cfg_attr(feature = "schemars", schemars(title = "Predefined"))]
    Predefined(PredefinedAspectRatio),
    /// A custom W:H aspect ratio
    #[cfg_attr(feature = "schemars", schemars(title = "Custom"))]
    Custom(i32, i32),
}

impl Default for AspectRatio {
    fn default() -> Self {
        AspectRatio::Predefined(PredefinedAspectRatio::default())
    }
}

#[derive(Copy, Clone, Debug, Default, Display, EnumString, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Predefined aspect ratio
pub enum PredefinedAspectRatio {
    /// 21:9
    Ultrawide,
    /// 16:9
    Widescreen,
    /// 4:3
    #[default]
    Standard,
}

impl AspectRatio {
    pub fn width_and_height(self) -> (i32, i32) {
        match self {
            AspectRatio::Predefined(predefined) => match predefined {
                PredefinedAspectRatio::Ultrawide => (21, 9),
                PredefinedAspectRatio::Widescreen => (16, 9),
                PredefinedAspectRatio::Standard => (4, 3),
            },
            AspectRatio::Custom(w, h) => (w, h),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RuleDebug {
    pub should_manage: bool,
    pub is_window: bool,
    // pub has_minimum_width: bool,
    // pub has_minimum_height: bool,
    pub has_title: bool,
    // pub is_cloaked: bool,
    // pub allow_cloaked: bool,
    // pub allow_layered_transparency: bool,
    // pub window_style: Option<WindowStyle>,
    // pub extended_window_style: Option<ExtendedWindowStyle>,
    pub title: Option<String>,
    pub exe_name: Option<String>,
    pub role: Option<String>,
    pub subrole: Option<String>,
    pub path: Option<String>,
    pub matches_permaignore_class: Option<String>,
    pub matches_ignore_identifier: Option<MatchingRule>,
    pub matches_managed_override: Option<MatchingRule>,
    // pub matches_layered_whitelist: Option<MatchingRule>,
    pub matches_floating_applications: Option<MatchingRule>,
    pub matches_titleless_applications: Option<String>,
    // pub matches_wsl2_gui: Option<String>,
    // pub matches_no_titlebar: Option<MatchingRule>,
}

#[allow(clippy::too_many_arguments)]
fn window_is_eligible(
    _window_id: u32,
    title: &str,
    exe_name: &str,
    classes: &[&str],
    path: &str,
    // style: &WindowStyle,
    // ex_style: &ExtendedWindowStyle,
    _event: Option<WindowManagerEvent>,
    debug: &mut RuleDebug,
) -> bool {
    {
        let permaignore_classes = PERMAIGNORE_CLASSES.lock();
        for class in classes {
            if permaignore_classes.contains(&class.to_string()) {
                debug.matches_permaignore_class = Some(class.to_string());
                return false;
            }
        }
    }

    let regex_identifiers = REGEX_IDENTIFIERS.lock();

    let ignore_identifiers = IGNORE_IDENTIFIERS.lock();
    let should_ignore = if let Some(rule) = should_act(
        title,
        exe_name,
        classes,
        path,
        &ignore_identifiers,
        &regex_identifiers,
    ) {
        debug.matches_ignore_identifier = Some(rule);
        true
    } else {
        false
    };

    let manage_identifiers = MANAGE_IDENTIFIERS.lock();
    let managed_override = if let Some(rule) = should_act(
        title,
        exe_name,
        classes,
        path,
        &manage_identifiers,
        &regex_identifiers,
    ) {
        debug.matches_managed_override = Some(rule);
        true
    } else {
        false
    };

    let floating_identifiers = FLOATING_APPLICATIONS.lock();
    if let Some(rule) = should_act(
        title,
        exe_name,
        classes,
        path,
        &floating_identifiers,
        &regex_identifiers,
    ) {
        debug.matches_floating_applications = Some(rule);
    }

    if should_ignore && !managed_override {
        return false;
    }

    // let layered_whitelist = LAYERED_WHITELIST.lock();
    // let mut allow_layered = if let Some(rule) = should_act(
    //     title,
    //     exe_name,
    //     class,
    //     path,
    //     &layered_whitelist,
    //     &regex_identifiers,
    // ) {
    //     debug.matches_layered_whitelist = Some(rule);
    //     true
    // } else {
    //     false
    // };
    //
    // let known_layered_hwnds = transparency_manager::known_hwnds();

    // allow_layered = if known_layered_hwnds.contains(&hwnd)
    //     // we always want to process hide events for windows with transparency, even on other
    //     // monitors, because we don't want to be left with ghost tiles
    //     || matches!(event, Some(WindowManagerEvent::Hide(_, _)))
    // {
    //     debug.allow_layered_transparency = true;
    //     true
    // } else {
    //     allow_layered
    // };
    //
    // let allow_wsl2_gui = {
    //     let wsl2_ui_processes = WSL2_UI_PROCESSES.lock();
    //     let allow = wsl2_ui_processes.contains(exe_name);
    //     if allow {
    //         debug.matches_wsl2_gui = Some(exe_name.clone())
    //     }
    //
    //     allow
    // };

    // let titlebars_removed = NO_TITLEBAR.lock();
    // let allow_titlebar_removed = if let Some(rule) = should_act(
    //     title,
    //     exe_name,
    //     class,
    //     path,
    //     &titlebars_removed,
    //     &regex_identifiers,
    // ) {
    //     debug.matches_no_titlebar = Some(rule);
    //     true
    // } else {
    //     false
    // };
    //
    // {
    //     let slow_application_identifiers = SLOW_APPLICATION_IDENTIFIERS.lock();
    //     let should_sleep = should_act(
    //         title,
    //         exe_name,
    //         class,
    //         path,
    //         &slow_application_identifiers,
    //         &regex_identifiers,
    //     )
    //         .is_some();
    //
    //     if should_sleep {
    //         std::thread::sleep(Duration::from_millis(
    //             SLOW_APPLICATION_COMPENSATION_TIME.load(Ordering::SeqCst),
    //         ));
    //     }
    // }

    // TODO: not sure about this new manage by default base case for macOS
    true
}

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub fn should_act(
    title: &str,
    exe_name: &str,
    classes: &[&str],
    path: &str,
    identifiers: &[MatchingRule],
    regex_identifiers: &HashMap<String, Regex>,
) -> Option<MatchingRule> {
    let mut matching_rule = None;
    for rule in identifiers {
        match rule {
            MatchingRule::Simple(identifier) => {
                if should_act_individual(
                    title,
                    exe_name,
                    classes,
                    path,
                    identifier,
                    regex_identifiers,
                ) {
                    matching_rule = Some(rule.clone());
                };
            }
            MatchingRule::Composite(identifiers) => {
                let mut composite_results = vec![];
                for identifier in identifiers {
                    composite_results.push(should_act_individual(
                        title,
                        exe_name,
                        classes,
                        path,
                        identifier,
                        regex_identifiers,
                    ));
                }

                if composite_results.iter().all(|&x| x) {
                    matching_rule = Some(rule.clone());
                }
            }
        }
    }

    matching_rule
}

pub fn should_act_individual(
    title: &str,
    exe_name: &str,
    classes: &[&str],
    path: &str,
    identifier: &IdWithIdentifier,
    regex_identifiers: &HashMap<String, Regex>,
) -> bool {
    let mut should_act = false;

    let mut identifier = identifier.clone();
    identifier.id = identifier.id.replace(".exe", "");

    match identifier.matching_strategy {
        None | Some(MatchingStrategy::Legacy) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.starts_with(&identifier.id) || title.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.starts_with(&identifier.id) || class.ends_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.eq(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::Equals) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.eq(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.eq(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::DoesNotEqual) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if !title.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if !class.eq(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if !exe_name.eq(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if !path.eq(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::StartsWith) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.starts_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::DoesNotStartWith) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if !title.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if !class.starts_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if !exe_name.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if !path.starts_with(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::EndsWith) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.ends_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::DoesNotEndWith) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if !title.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if !class.ends_with(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if !exe_name.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if !path.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::Contains) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.contains(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if class.contains(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if exe_name.contains(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if path.contains(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::DoesNotContain) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if !title.contains(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                for class in classes {
                    if !class.contains(&identifier.id) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if !exe_name.contains(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if !path.contains(&identifier.id) {
                    should_act = true;
                }
            }
        },
        Some(MatchingStrategy::Regex) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if let Some(re) = regex_identifiers.get(&identifier.id)
                    && re.is_match(title)
                {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                if let Some(re) = regex_identifiers.get(&identifier.id) {
                    for class in classes {
                        if re.is_match(class) {
                            should_act = true;
                        }
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if let Some(re) = regex_identifiers.get(&identifier.id)
                    && re.is_match(exe_name)
                {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Path => {
                if let Some(re) = regex_identifiers.get(&identifier.id)
                    && re.is_match(path)
                {
                    should_act = true;
                }
            }
        },
    }

    should_act
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_dragged_before_a_drag_begins() {
        let drag = Drag::default();

        assert_eq!(drag.window(|| true), None);
        assert_eq!(drag.window(|| false), None);
    }

    #[test]
    fn the_button_is_not_asked_about_when_there_is_no_drag() {
        let drag = Drag::default();

        assert_eq!(
            drag.window(|| panic!("nothing was being dragged, so there is nothing to ask")),
            None
        );
    }

    #[test]
    fn the_window_of_a_drag_is_dragged_for_as_long_as_the_button_is_down() {
        let drag = Drag::default();
        drag.begin(42);

        assert_eq!(drag.window(|| true), Some(42));
        assert_eq!(drag.window(|| true), Some(42));
    }

    #[test]
    fn nothing_is_dragged_once_the_drag_has_ended() {
        let drag = Drag::default();
        drag.begin(42);
        drag.end();

        assert_eq!(drag.window(|| true), None);
    }

    #[test]
    fn a_button_that_went_up_without_the_end_being_seen_leaves_nothing_dragged() {
        // the tap that sees the button go up can be turned off, and then nothing says so
        let drag = Drag::default();
        drag.begin(42);

        assert_eq!(drag.window(|| false), None);

        // and it is not dragged again by the next time that the button is down
        assert_eq!(drag.window(|| true), None);
    }

    #[test]
    fn a_drag_that_began_is_not_forgotten_by_one_that_ended_before() {
        let drag = Drag::default();
        drag.begin(42);
        drag.end();
        drag.begin(7);

        assert_eq!(drag.window(|| true), Some(7));
    }

    #[test]
    fn a_window_that_a_step_could_not_write_to_is_sent_again_by_the_next() {
        // an id that nothing else in the tests uses: what was sent is shared by all of them
        let window = Window::from(3_800_000_001);
        let rect = Rect {
            left: 10,
            top: 20,
            right: 300,
            bottom: 200,
        };

        // there is no window of that id to write to, which is a write that fails
        assert!(window.set_position_for_step(0, &rect).is_err());
        assert_eq!(
            SENT.lock().get(&window.id),
            None,
            "it was not sent anywhere, so it is not where it was sent last"
        );

        // and the same step again is not left alone, as it would be for a window that took it
        assert!(window.set_position_for_step(0, &rect).is_err());
    }

    #[test]
    fn a_window_that_is_sent_where_it_was_sent_last_is_left_alone() {
        let window = Window::from(3_800_000_002);
        let rect = Rect {
            left: 10,
            top: 20,
            right: 300,
            bottom: 200,
        };

        SENT.lock().insert(window.id, rect);

        assert!(window.set_position_for_step(0, &rect).is_ok());
        assert_eq!(SENT.lock().get(&window.id), Some(&rect));
    }

    #[test]
    fn the_window_that_is_dragged_is_not_sent_anywhere() {
        let window = Window::from(3_800_000_003);
        let rect = Rect::default();

        assert!(window.set_position_for_step(window.id, &rect).is_ok());
        assert_eq!(SENT.lock().get(&window.id), None);
    }
}
