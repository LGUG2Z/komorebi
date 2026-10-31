//! [`Window`], the window manager's handle to a window on the desktop.
//!
//! A window is a `Copy` value that carries only the compositor's id for it. Everything else about
//! the window (its title, its class, where it is) is asked of the
//! [`Backend`](crate::backend::Backend) when it is needed, and everything done to it (a move, the
//! focus, a close) is a command to the backend. Nothing is cached, so a window that changed its
//! title, or closed, is seen as it is now.
//!
//! [`Window::should_manage`] decides whether komorebi manages a window: it has to be a normal
//! application window that is alive, and then the ignore and manage rules decide
//! ([`should_act`], [`should_act_individual`]), writing what they found to a [`RuleDebug`].
//!
//! [`Window::hide`] takes a window out of sight, which is what a stack does with every window but
//! the one in front, and records it in [`struct@HIDDEN_WINDOWS`] so that the window manager does
//! not mistake what the compositor reports about it for the user's doing.
//!
//! The windows the user is resizing with the pointer are recorded too
//! ([`live_resize_begins`]): [`Window::set_position`] does not move those until the user lets go.

use crate::FLOATING_APPLICATIONS;
use crate::FLOATING_WINDOW_TOGGLE_ASPECT_RATIO;
use crate::HIDDEN_WINDOWS;
use crate::IGNORE_IDENTIFIERS;
use crate::MANAGE_IDENTIFIERS;
use crate::PERMAIGNORE_CLASSES;
use crate::REGEX_IDENTIFIERS;
use crate::backend::backend;
use crate::core::ApplicationIdentifier;
use crate::core::Rect;
pub use komorebi_config::aspect_ratio::AspectRatio;
pub use komorebi_config::aspect_ratio::PredefinedAspectRatio;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::window_manager_event::WindowManagerEvent;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use serde::ser::SerializeStruct;
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Write;
use std::path::PathBuf;

/// The windows the user is resizing by dragging an edge of: komorebi does not move them while they
/// are (the other tiles follow the drag, and the window is put in its tile when they let go)
static LIVE_RESIZED_WINDOWS: parking_lot::Mutex<Vec<u32>> = parking_lot::Mutex::new(Vec::new());

/// Records that the user has taken hold of an edge of this window: [`Window::set_position`] leaves
/// it alone until [`live_resize_ends`]
pub fn live_resize_begins(id: u32) {
    let mut windows = LIVE_RESIZED_WINDOWS.lock();
    if !windows.contains(&id) {
        windows.push(id);
    }
}

/// Records that the user has let go of the edge of this window they were dragging
pub fn live_resize_ends(id: u32) {
    LIVE_RESIZED_WINDOWS.lock().retain(|w| *w != id);
}

/// Whether the user has hold of an edge of any window, whose neighbours are on their way to
/// where the layout puts them for as long as they do
pub fn live_resize_in_progress() -> bool {
    !LIVE_RESIZED_WINDOWS.lock().is_empty()
}

fn is_live_resized(id: u32) -> bool {
    LIVE_RESIZED_WINDOWS.lock().contains(&id)
}

/// A handle to a window on the desktop: just its id.
///
/// It is `Copy`, events carry it by value, and everything else (title, class, geometry, ...) is
/// asked of the [`crate::backend::Backend`] when needed. The id is the compositor's
/// ([`WindowInfo::id`](crate::backend::WindowInfo::id)), and stays the same for the life of the
/// window.
#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
pub struct Window {
    pub id: u32,
}

impl From<u32> for Window {
    fn from(id: u32) -> Self {
        Self { id }
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowDetails {
    pub title: String,
    pub exe: String,
    pub class: String,
    pub path: String,
}

impl From<&Window> for WindowDetails {
    fn from(value: &Window) -> Self {
        Self::from(*value)
    }
}

impl From<Window> for WindowDetails {
    fn from(value: Window) -> Self {
        Self {
            title: value.title().unwrap_or_default(),
            exe: value.exe().unwrap_or_default(),
            class: value.class().unwrap_or_default(),
            path: value
                .path()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        }
    }
}

impl Display for Window {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut display = format!("(window_id: {}", self.id);

        if let Some(info) = self.info() {
            write!(display, ", process_id: {}", info.pid)?;
            write!(display, ", title: {}", info.title)?;
            write!(display, ", exe: {}", info.exe)?;
            write!(display, ", class: {}", info.class)?;
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
        let mut state = serializer.serialize_struct("Window", 3)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("rect", &self.rect().unwrap_or_default())?;
        state.serialize_field("details", &WindowDetails::from(*self))?;
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
    class: String,
    path: String,
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
    /// What the compositor knows about this window, including for a few moments after it closed
    pub fn info(self) -> Option<crate::backend::WindowInfo> {
        backend().window(self.id)
    }

    /// Whether the window still exists
    pub fn is_valid(self) -> bool {
        backend().is_window(self.id)
    }

    pub fn is_focused(self) -> bool {
        backend().foreground_window() == Some(self.id)
    }

    pub fn is_minimized(self) -> bool {
        self.info().is_some_and(|i| i.minimized)
    }

    pub fn title(self) -> Option<String> {
        self.info().map(|i| i.title)
    }

    pub fn exe(self) -> Option<String> {
        self.info().map(|i| i.exe)
    }

    /// The Wayland `app_id` (or X11 `WM_CLASS`)
    pub fn class(self) -> Option<String> {
        self.info().map(|i| i.class)
    }

    pub fn path(self) -> Option<PathBuf> {
        self.info().map(|i| PathBuf::from(i.path))
    }

    pub fn process_id(self) -> i32 {
        self.info().map_or(0, |i| i.pid)
    }

    /// The window's current frame geometry, as last reported by the compositor
    pub fn rect(self) -> eyre::Result<Rect> {
        backend()
            .window_rect(self.id)
            .ok_or_eyre("the window does not exist")
    }

    /// Asks the compositor to move and resize the window to `rect`; where the window ends up is
    /// [`Window::rect`], since a client may refuse a size
    ///
    /// A window whose edge the user is dragging ([`live_resize_begins`]) is not moved: it is the
    /// user's until they let go.
    ///
    /// A fullscreen window is left where it is, and the call succeeds.
    pub fn set_position(self, rect: &Rect) -> eyre::Result<()> {
        // the window the user is dragging an edge of is theirs until they let go
        if is_live_resized(self.id) {
            return Ok(());
        }

        // A fullscreen window covers the monitor on purpose: moving it would take it out of
        // fullscreen, or be ignored. It goes back to its tile when it leaves fullscreen.
        if self.is_fullscreen() {
            tracing::debug!("not moving window {}: it is fullscreen", self.id);
            return Ok(());
        }

        backend().set_rect(self.id, rect)
    }

    /// Returns true if and only if the window is fullscreen
    pub fn is_fullscreen(self) -> bool {
        backend().window(self.id).is_some_and(|w| w.fullscreen)
    }

    /// Asks the compositor to draw the window with this opacity, 255 being not see-through at all
    pub fn set_opacity(self, alpha: u8) -> eyre::Result<()> {
        backend().set_opacity(self.id, alpha)
    }

    pub fn focus(self, mouse_follows_focus: bool) -> eyre::Result<()> {
        backend().focus(self.id, mouse_follows_focus)
    }

    /// Have the compositor maximize the window (the work area, panels respected)
    ///
    /// # Errors
    ///
    /// The backend does not know the window, or refuses to maximize it.
    pub fn maximize(self) -> eyre::Result<()> {
        backend().maximize(self.id, true)
    }

    /// Have the compositor give the window back the frame it had before it was maximized
    ///
    /// # Errors
    ///
    /// The backend does not know the window, or refuses to restore it.
    pub fn unmaximize(self) -> eyre::Result<()> {
        backend().maximize(self.id, false)
    }

    pub fn raise(self) -> eyre::Result<()> {
        backend().raise(self.id)
    }

    pub fn close(self) -> eyre::Result<()> {
        backend().close(self.id)
    }

    /// Minimizes the window, as the user asking for it would. [`Window::hide`] is what komorebi
    /// does to keep the back of a stack out of the way.
    pub fn minimize(self) -> eyre::Result<()> {
        backend().minimize(self.id)
    }

    pub fn unminimize(self) -> eyre::Result<()> {
        backend().unminimize(self.id)
    }

    /// Takes the window out of sight ([`Backend::hide`](crate::backend::Backend::hide)): the back
    /// of a stack, which on KDE goes to a virtual desktop nobody looks at
    ///
    /// The id is recorded in [`struct@HIDDEN_WINDOWS`] first, so that what the compositor reports
    /// about the window because of this (a move to another desktop) is not mistaken for something
    /// the user did. A backend that cannot hide the window is logged and not returned: the window
    /// stays where it is, and stays recorded as hidden until [`Window::restore`].
    pub fn hide(self) {
        HIDDEN_WINDOWS.lock().insert(self.id);
        if let Err(error) = backend().hide(self.id) {
            tracing::warn!("failed to hide window {}: {error}", self.id);
        }
    }

    /// Brings a window [hidden](Window::hide) back, and takes it out of
    /// [`struct@HIDDEN_WINDOWS`]. A failure is logged, as for [`Window::hide`].
    pub fn restore(self) {
        HIDDEN_WINDOWS.lock().remove(&self.id);
        if let Err(error) = backend().restore(self.id) {
            tracing::warn!("failed to restore window {}: {error}", self.id);
        }
    }

    /// Moves the window to the middle of `work_area`, keeping its size, or with `resize` making it
    /// half as high as the work area and as wide as the aspect ratio of floating windows says
    ///
    /// # Errors
    ///
    /// The backend does not know the window, or refuses to move it.
    pub fn center(&mut self, work_area: &Rect, resize: bool) -> eyre::Result<()> {
        let (target_width, target_height) = if resize {
            let (aspect_ratio_width, aspect_ratio_height) = FLOATING_WINDOW_TOGGLE_ASPECT_RATIO
                .lock()
                .width_and_height();
            let target_height = work_area.bottom / 2;
            let target_width = (target_height * aspect_ratio_width) / aspect_ratio_height;
            (target_width, target_height)
        } else {
            let current_rect = self.rect()?;
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

    /// Moves the window from one work area to another, keeping where it is in the area and how
    /// much of it it takes, in proportion: a floating window that goes to another monitor
    pub fn move_to_area(&self, current_area: &Rect, target_area: &Rect) -> eyre::Result<()> {
        let current_rect = self.rect()?;
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

        self.set_position(&Rect {
            left,
            top,
            right: corrected_width,
            bottom: corrected_height,
        })
    }

    /// Whether the window is one komorebi should tile: a window that is still alive, that the
    /// compositor considers a normal application window, as opposed to a dialog, a splash screen,
    /// a utility window or a dock ([`WindowInfo::normal`](crate::backend::WindowInfo::normal)),
    /// and that no ignore rule covers, or that a manage rule rescues
    ///
    /// What was looked at and which rules matched is written to `debug`, which is how
    /// `DebugWindow` answers. A window the compositor has no record of is not managed. Windows
    /// that are minimized when komorebi starts are not picked up either
    /// ([`load_workspace_information`](crate::desktop::load_workspace_information)).
    ///
    /// # Errors
    ///
    /// None: every path returns `Ok`.
    #[tracing::instrument(skip_all)]
    pub fn should_manage(
        &self,
        event: Option<WindowManagerEvent>,
        debug: &mut RuleDebug,
    ) -> eyre::Result<bool> {
        let Some(info) = self.info() else {
            return Ok(false);
        };

        if !info.alive {
            return Ok(false);
        }

        // dialogs, splash screens, utility windows, docks and the like are not for tiling
        if !info.normal {
            return Ok(false);
        }

        debug.is_window = true;

        // windows are often announced before their title is set; a TitleUpdate re-evaluates them
        debug.has_title = !info.title.is_empty();

        debug.title = Some(info.title.clone());
        debug.exe_name = Some(info.exe.clone());
        debug.class = Some(info.class.clone());
        debug.path = Some(info.path.clone());

        let eligible = window_is_eligible(
            self.id,
            &info.title,
            &info.exe,
            &[&info.class],
            &info.path,
            event,
            debug,
        );

        debug.should_manage = eligible;
        Ok(eligible)
    }
}

/// Why [`Window::should_manage`] decided as it did: what it read of the window, and the rule of
/// each kind that matched it
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
    pub class: Option<String>,
    pub path: Option<String>,
    pub matches_permaignore_class: Option<String>,
    pub matches_ignore_identifier: Option<MatchingRule>,
    pub matches_managed_override: Option<MatchingRule>,
    // pub matches_layered_whitelist: Option<MatchingRule>,
    pub matches_floating_applications: Option<MatchingRule>,
    // pub matches_wsl2_gui: Option<String>,
    // pub matches_no_titlebar: Option<MatchingRule>,
}

/// Whether the rules let a window be managed: not when its class is one of
/// [`struct@PERMAIGNORE_CLASSES`], and not when an ignore rule matches it and no manage rule
/// does. Every rule that matches is written to `debug`, the float rules too, which do not decide
/// whether the window is managed.
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

    // Every place that holds two of the rule lists takes them in this order: floating, transparency,
    // ignore, manage, regex. A thread that takes two in another order can wait for one that holds
    // what it needs, and both wait for ever.
    let floating_identifiers = FLOATING_APPLICATIONS.lock();
    let ignore_identifiers = IGNORE_IDENTIFIERS.lock();
    let manage_identifiers = MANAGE_IDENTIFIERS.lock();
    let regex_identifiers = REGEX_IDENTIFIERS.lock();
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

    true
}

/// Returns the last of `identifiers` that matches a window with this title, executable, classes
/// and path, if one does: a simple rule matches when its one condition does
/// ([`should_act_individual`]), and a composite rule when all of its conditions do
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

/// Returns true if and only if the condition `identifier` holds for a window with this title,
/// executable, classes and path
///
/// The condition compares the title, the executable, any of the classes or the path with its id,
/// by its matching strategy. Without one, or with `Legacy`, a title or a class matches when it
/// starts or ends with the id, and an executable or a path when it is the id. A `Regex` id is
/// looked up in `regex_identifiers`, and an id with no compiled regex there matches nothing. A
/// `.exe` in the id is ignored.
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
    use crate::backend::fake::Command;
    use crate::backend::fake::FakeBackend;
    use crate::backend::with_backend;
    use std::sync::Arc;

    fn fake_with(id: u32) -> Arc<FakeBackend> {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(id, "foot", "foot", "a terminal");
        fake
    }

    #[test]
    fn a_window_is_what_the_backend_says_it_is() -> eyre::Result<()> {
        let fake = fake_with(201);
        fake.set_foreground(Some(201));

        with_backend(fake, || {
            let window = Window::from(201);

            assert!(window.is_valid());
            assert!(window.is_focused());
            assert!(!window.is_minimized());
            assert_eq!(window.title().as_deref(), Some("a terminal"));
            assert_eq!(window.exe().as_deref(), Some("foot"));
            assert_eq!(window.class().as_deref(), Some("foot"));
            assert_eq!(window.path(), Some(PathBuf::from("/usr/bin/foot")));
            assert_eq!(window.process_id(), 1201);
            assert_eq!(
                window.rect()?,
                Rect {
                    left: 100,
                    top: 100,
                    right: 800,
                    bottom: 600,
                }
            );
            assert_eq!(
                window.to_string(),
                "(window_id: 201, process_id: 1201, title: a terminal, exe: foot, class: foot)"
            );
            Ok(())
        })
    }

    #[test]
    fn a_window_the_backend_does_not_know_has_no_details() {
        let fake = Arc::new(FakeBackend::with_one_monitor());

        with_backend(fake, || {
            let window = Window::from(202);

            assert!(!window.is_valid());
            assert_eq!(window.title(), None);
            assert_eq!(window.process_id(), 0);
            assert!(window.rect().is_err());
            assert_eq!(window.to_string(), "(window_id: 202)");
        });
    }

    #[test]
    fn every_change_to_a_window_is_a_command_to_the_backend() -> eyre::Result<()> {
        let fake = fake_with(203);
        let tile = Rect {
            left: 0,
            top: 0,
            right: 960,
            bottom: 1050,
        };

        with_backend(fake.clone(), || {
            let window = Window::from(203);

            window.set_position(&tile)?;
            window.focus(false)?;
            window.raise()?;
            window.minimize()?;
            window.unminimize()?;
            window.hide();
            window.restore();
            window.maximize()?;
            window.unmaximize()?;
            window.close()
        })?;

        assert_eq!(
            fake.commands(),
            [
                Command::SetRect(203, tile),
                Command::Focus(203, false),
                Command::Raise(203),
                Command::Minimize(203),
                Command::Unminimize(203),
                Command::Hide(203),
                Command::Restore(203),
                Command::Maximize(203, true),
                Command::Maximize(203, false),
                Command::Close(203),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_window_komorebi_hid_is_recorded_until_it_is_restored() {
        let fake = fake_with(209);

        with_backend(fake.clone(), || {
            let window = Window::from(209);

            window.hide();
            assert!(HIDDEN_WINDOWS.lock().contains(&209));
            assert!(fake.is_hidden(209));

            window.restore();
            assert!(!HIDDEN_WINDOWS.lock().contains(&209));
            assert!(!fake.is_hidden(209));
        });
    }

    #[test]
    fn a_window_the_backend_cannot_hide_is_recorded_all_the_same() {
        let fake = Arc::new(FakeBackend::with_one_monitor());

        with_backend(fake.clone(), || {
            Window::from(210).hide();

            assert!(HIDDEN_WINDOWS.lock().contains(&210));
            assert!(!fake.is_hidden(210));

            Window::from(210).restore();
            assert!(!HIDDEN_WINDOWS.lock().contains(&210));
        });
    }

    #[test]
    fn a_fullscreen_window_is_not_moved_and_is_again_when_it_is_not_fullscreen() -> eyre::Result<()>
    {
        let fake = fake_with(211);
        let tile = Rect {
            left: 0,
            top: 0,
            right: 960,
            bottom: 1050,
        };

        with_backend(fake.clone(), || {
            let window = Window::from(211);
            assert!(!window.is_fullscreen());

            fake.set_fullscreen(211, true);
            assert!(window.is_fullscreen());
            window.set_position(&tile)?;
            assert_ne!(window.rect()?, tile);
            assert!(fake.commands().is_empty());

            fake.set_fullscreen(211, false);
            assert!(!window.is_fullscreen());
            window.set_position(&tile)?;
            assert_eq!(window.rect()?, tile);
            Ok(())
        })
    }

    #[test]
    fn only_a_normal_window_that_is_alive_is_managed() {
        let fake = Arc::new(FakeBackend::with_one_monitor());
        fake.open_window(205, "foot", "foot", "a terminal");
        fake.open_window(206, "foot", "foot", "a dialog");
        fake.set_normal(206, false);
        fake.open_window(207, "foot", "foot", "closed");
        fake.close_window_now(207);

        with_backend(fake, || {
            let should_manage =
                |id| Window::from(id).should_manage(None, &mut RuleDebug::default());
            assert!(should_manage(205).unwrap());
            assert!(!should_manage(206).unwrap());
            assert!(!should_manage(207).unwrap());
            assert!(!should_manage(208).unwrap());
        });
    }

    #[test]
    fn a_window_serializes_with_what_the_backend_knows_about_it() -> eyre::Result<()> {
        let fake = fake_with(204);

        let json = with_backend(fake, || serde_json::to_value(Window::from(204)))?;

        assert_eq!(json["id"], 204);
        assert_eq!(json["rect"]["right"], 800);
        assert_eq!(json["details"]["title"], "a terminal");
        assert_eq!(json["details"]["path"], "/usr/bin/foot");
        Ok(())
    }

    fn condition(
        kind: ApplicationIdentifier,
        id: &str,
        matching_strategy: Option<MatchingStrategy>,
    ) -> IdWithIdentifier {
        IdWithIdentifier {
            kind,
            id: id.to_string(),
            matching_strategy,
        }
    }

    /// Whether `condition` holds for a Kate window editing notes.txt
    fn holds_for_kate(condition: &IdWithIdentifier, regexes: &HashMap<String, Regex>) -> bool {
        should_act_individual(
            "notes.txt — Kate",
            "kate",
            &["org.kde.kate"],
            "/usr/bin/kate",
            condition,
            regexes,
        )
    }

    /// Whether the condition holds for the Kate window, with no compiled regex
    fn holds(kind: ApplicationIdentifier, id: &str, strategy: Option<MatchingStrategy>) -> bool {
        holds_for_kate(&condition(kind, id, strategy), &HashMap::new())
    }

    #[test]
    fn without_a_strategy_a_title_or_a_class_matches_at_either_end_and_an_executable_whole() {
        use ApplicationIdentifier::*;

        assert!(holds(Title, "Kate", None));
        assert!(holds(Title, "notes", None));
        assert!(!holds(Title, "txt", None));
        assert!(holds(Class, "kate", None));
        assert!(holds(Exe, "kate.exe", None));
        assert!(!holds(Exe, "kat", None));
        assert!(holds(Path, "/usr/bin/kate", None));
    }

    #[test]
    fn a_strategy_says_how_the_id_is_compared_and_a_regex_needs_its_compiled_form() {
        use ApplicationIdentifier::*;
        use MatchingStrategy::*;

        assert!(!holds(Class, "kate", Some(Equals)));
        assert!(holds(Class, "org.kde.kate", Some(Equals)));
        assert!(holds(Title, "txt", Some(Contains)));
        assert!(holds(Exe, "foot", Some(DoesNotEqual)));
        assert!(holds(Path, "/usr/", Some(StartsWith)));

        let regex = condition(Title, "^notes\\.", Some(Regex));
        assert!(!holds_for_kate(&regex, &HashMap::new()));
        let compiled = HashMap::from([(
            String::from("^notes\\."),
            regex::Regex::new("^notes\\.").unwrap(),
        )]);
        assert!(holds_for_kate(&regex, &compiled));
    }

    #[test]
    fn a_composite_rule_needs_every_condition_and_the_last_rule_that_matches_is_the_one() {
        use ApplicationIdentifier::*;
        let kate = condition(Class, "org.kde.kate", Some(MatchingStrategy::Equals));
        let notes = condition(Title, "notes", None);
        let other = condition(Title, "other", None);
        let rules = [
            MatchingRule::Composite(vec![kate.clone(), other]),
            MatchingRule::Simple(kate.clone()),
            MatchingRule::Composite(vec![kate, notes]),
        ];

        let matched = should_act(
            "notes.txt — Kate",
            "kate",
            &["org.kde.kate"],
            "/usr/bin/kate",
            &rules,
            &HashMap::new(),
        );

        assert_eq!(matched, Some(rules[2].clone()));
        assert_eq!(
            should_act("x", "x", &["x"], "x", &rules[..1], &HashMap::new()),
            None
        );
    }
}
