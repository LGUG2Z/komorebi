use crate::com::SetCloak;
use crate::winevent_listener;
use crate::ANIMATION_DURATION;
use crate::ANIMATION_ENABLED;
use crate::ANIMATION_MANAGER;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Write as _;
use std::sync::atomic::Ordering;
use std::time::Duration;

use color_eyre::eyre;
use color_eyre::eyre::anyhow;
use color_eyre::Result;
use komorebi_core::config_generation::IdWithIdentifier;
use komorebi_core::config_generation::MatchingRule;
use komorebi_core::config_generation::MatchingStrategy;
use regex::Regex;
use schemars::JsonSchema;
use serde::ser::Error;
use serde::ser::SerializeStruct;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use windows::Win32::Foundation::HWND;

use komorebi_core::ApplicationIdentifier;
use komorebi_core::HidingBehaviour;
use komorebi_core::Rect;

use crate::animation::Animation;
use crate::styles::ExtendedWindowStyle;
use crate::styles::WindowStyle;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::FLOAT_IDENTIFIERS;
use crate::HIDDEN_HWNDS;
use crate::HIDING_BEHAVIOUR;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::NO_TITLEBAR;
use crate::PERMAIGNORE_CLASSES;
use crate::REGEX_IDENTIFIERS;
use crate::WSL2_UI_PROCESSES;

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub struct Window {
    pub hwnd: isize,
    animation: Animation,
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WindowDetails {
    pub title: String,
    pub exe: String,
    pub class: String,
}

impl TryFrom<Window> for WindowDetails {
    type Error = eyre::ErrReport;

    fn try_from(value: Window) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            title: value.title()?,
            exe: value.exe()?,
            class: value.class()?,
        })
    }
}

impl Display for Window {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut display = format!("(hwnd: {}", self.hwnd);

        if let Ok(title) = self.title() {
            write!(display, ", title: {title}")?;
        }

        if let Ok(exe) = self.exe() {
            write!(display, ", exe: {exe}")?;
        }

        if let Ok(class) = self.class() {
            write!(display, ", class: {class}")?;
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
        let mut state = serializer.serialize_struct("Window", 5)?;
        state.serialize_field("hwnd", &self.hwnd)?;
        state.serialize_field(
            "title",
            &self
                .title()
                .map_err(|_| S::Error::custom("could not get window title"))?,
        )?;
        state.serialize_field(
            "exe",
            &self
                .exe()
                .map_err(|_| S::Error::custom("could not get window exe"))?,
        )?;
        state.serialize_field(
            "class",
            &self
                .class()
                .map_err(|_| S::Error::custom("could not get window class"))?,
        )?;
        state.serialize_field(
            "rect",
            &WindowsApi::window_rect(self.hwnd())
                .map_err(|_| S::Error::custom("could not get window rect"))?,
        )?;
        state.end()
    }
}

impl Window {
    // for instantiation of animation struct
    pub fn new(hwnd: isize) -> Self {
        Self {
            hwnd,
            animation: Animation::new(hwnd),
        }
    }

    pub const fn hwnd(self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn center(&mut self, work_area: &Rect) -> Result<()> {
        let half_width = work_area.right / 2;
        let half_weight = work_area.bottom / 2;

        self.set_position(
            &Rect {
                left: work_area.left + ((work_area.right - half_width) / 2),
                top: work_area.top + ((work_area.bottom - half_weight) / 2),
                right: half_width,
                bottom: half_weight,
            },
            true,
        )
    }

    pub fn animate_position(&self, layout: &Rect, top: bool) -> Result<()> {
        let hwnd = self.hwnd();
        let curr_rect = WindowsApi::window_rect(hwnd).unwrap();

        if curr_rect.left == layout.left
            && curr_rect.top == layout.top
            && curr_rect.bottom == layout.bottom
            && curr_rect.right == layout.right
        {
            WindowsApi::position_window(hwnd, layout, top)
        } else {
            let target_rect = *layout;
            let duration = Duration::from_millis(ANIMATION_DURATION.load(Ordering::SeqCst));
            let mut animation = self.animation;

            let self_copied = *self;
            std::thread::spawn(move || {
                animation.animate(duration, |progress: f64| {
                    let new_rect = Animation::lerp_rect(&curr_rect, &target_rect, progress);
                    if progress < 1.0 {
                        // using MoveWindow because it runs faster than SetWindowPos
                        // so animation have more fps and feel smoother
                        WindowsApi::move_window(hwnd, &new_rect, true)?;
                        WindowsApi::invalidate_rect(hwnd, None, false);
                    } else {
                        WindowsApi::position_window(hwnd, &new_rect, top)?;

                        if WindowsApi::foreground_window()? == self_copied.hwnd {
                            winevent_listener::event_tx()
                                .send(WindowManagerEvent::UpdateFocusedWindowBorder(self_copied))?;
                        }
                    }

                    Ok(())
                })
            });

            Ok(())
        }
    }

    pub fn set_position(&mut self, layout: &Rect, top: bool) -> Result<()> {
        let rect = *layout;
        if ANIMATION_ENABLED.load(Ordering::SeqCst) {
            if ANIMATION_MANAGER.lock().in_progress(self.hwnd) {
                self.animation.cancel();
            }

            self.animate_position(&rect, top)
        } else {
            WindowsApi::position_window(self.hwnd(), &rect, top)
        }
    }

    pub fn is_maximized(self) -> bool {
        WindowsApi::is_zoomed(self.hwnd())
    }

    pub fn is_miminized(self) -> bool {
        WindowsApi::is_iconic(self.hwnd())
    }

    pub fn is_visible(self) -> bool {
        WindowsApi::is_window_visible(self.hwnd())
    }

    pub fn hide(self) {
        let mut programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();
        if !programmatically_hidden_hwnds.contains(&self.hwnd) {
            programmatically_hidden_hwnds.push(self.hwnd);
        }

        let hiding_behaviour = HIDING_BEHAVIOUR.lock();
        match *hiding_behaviour {
            HidingBehaviour::Hide => WindowsApi::hide_window(self.hwnd()),
            HidingBehaviour::Minimize => WindowsApi::minimize_window(self.hwnd()),
            HidingBehaviour::Cloak => SetCloak(self.hwnd(), 1, 2),
        }
    }

    pub fn restore(self) {
        let mut programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();
        if let Some(idx) = programmatically_hidden_hwnds
            .iter()
            .position(|&hwnd| hwnd == self.hwnd)
        {
            programmatically_hidden_hwnds.remove(idx);
        }

        let hiding_behaviour = HIDING_BEHAVIOUR.lock();
        match *hiding_behaviour {
            HidingBehaviour::Hide | HidingBehaviour::Minimize => {
                WindowsApi::restore_window(self.hwnd());
            }
            HidingBehaviour::Cloak => SetCloak(self.hwnd(), 1, 0),
        }
    }

    pub fn minimize(self) {
        WindowsApi::minimize_window(self.hwnd());
    }

    pub fn close(self) -> Result<()> {
        WindowsApi::close_window(self.hwnd())
    }

    pub fn maximize(self) {
        let mut programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();
        if let Some(idx) = programmatically_hidden_hwnds
            .iter()
            .position(|&hwnd| hwnd == self.hwnd)
        {
            programmatically_hidden_hwnds.remove(idx);
        }

        WindowsApi::maximize_window(self.hwnd());
    }

    pub fn unmaximize(self) {
        let mut programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();
        if let Some(idx) = programmatically_hidden_hwnds
            .iter()
            .position(|&hwnd| hwnd == self.hwnd)
        {
            programmatically_hidden_hwnds.remove(idx);
        }

        WindowsApi::unmaximize_window(self.hwnd());
    }

    pub fn focus(self, mouse_follows_focus: bool) -> Result<()> {
        // If the target window is already focused, do nothing.
        if let Ok(ihwnd) = WindowsApi::foreground_window() {
            if HWND(ihwnd) == self.hwnd() {
                return Ok(());
            }
        }

        WindowsApi::raise_and_focus_window(self.hwnd())?;

        // Center cursor in Window
        if mouse_follows_focus {
            WindowsApi::center_cursor_in_rect(&WindowsApi::window_rect(self.hwnd())?)?;
        }

        Ok(())
    }

    pub fn transparent(self) -> Result<()> {
        let mut ex_style = self.ex_style()?;
        ex_style.insert(ExtendedWindowStyle::LAYERED);
        self.update_ex_style(&ex_style)?;
        WindowsApi::set_transparent(self.hwnd())
    }

    pub fn opaque(self) -> Result<()> {
        let mut ex_style = self.ex_style()?;
        ex_style.remove(ExtendedWindowStyle::LAYERED);
        self.update_ex_style(&ex_style)
    }

    #[allow(dead_code)]
    pub fn update_style(self, style: &WindowStyle) -> Result<()> {
        WindowsApi::update_style(self.hwnd(), isize::try_from(style.bits())?)
    }

    pub fn update_ex_style(self, style: &ExtendedWindowStyle) -> Result<()> {
        WindowsApi::update_ex_style(self.hwnd(), isize::try_from(style.bits())?)
    }

    pub fn style(self) -> Result<WindowStyle> {
        let bits = u32::try_from(WindowsApi::gwl_style(self.hwnd())?)?;
        WindowStyle::from_bits(bits).ok_or_else(|| anyhow!("there is no gwl style"))
    }

    pub fn ex_style(self) -> Result<ExtendedWindowStyle> {
        let bits = u32::try_from(WindowsApi::gwl_ex_style(self.hwnd())?)?;
        ExtendedWindowStyle::from_bits(bits).ok_or_else(|| anyhow!("there is no gwl style"))
    }

    pub fn title(self) -> Result<String> {
        WindowsApi::window_text_w(self.hwnd())
    }

    pub fn path(self) -> Result<String> {
        let (process_id, _) = WindowsApi::window_thread_process_id(self.hwnd());
        let handle = WindowsApi::process_handle(process_id)?;
        let path = WindowsApi::exe_path(handle);
        WindowsApi::close_process(handle)?;
        path
    }

    pub fn exe(self) -> Result<String> {
        let (process_id, _) = WindowsApi::window_thread_process_id(self.hwnd());
        let handle = WindowsApi::process_handle(process_id)?;
        let exe = WindowsApi::exe(handle);
        WindowsApi::close_process(handle)?;
        exe
    }

    pub fn class(self) -> Result<String> {
        WindowsApi::real_window_class_w(self.hwnd())
    }

    pub fn is_cloaked(self) -> Result<bool> {
        WindowsApi::is_window_cloaked(self.hwnd())
    }

    pub fn is_window(self) -> bool {
        WindowsApi::is_window(self.hwnd())
    }

    pub fn remove_title_bar(self) -> Result<()> {
        let mut style = self.style()?;
        style.remove(WindowStyle::CAPTION);
        style.remove(WindowStyle::THICKFRAME);
        self.update_style(&style)
    }

    pub fn add_title_bar(self) -> Result<()> {
        let mut style = self.style()?;
        style.insert(WindowStyle::CAPTION);
        style.insert(WindowStyle::THICKFRAME);
        self.update_style(&style)
    }

    #[tracing::instrument(fields(exe, title), skip(debug))]
    pub fn should_manage(
        self,
        event: Option<WindowManagerEvent>,
        debug: &mut RuleDebug,
    ) -> Result<bool> {
        if !self.is_window() {
            return Ok(false);
        }

        debug.is_window = true;

        if self.title().is_err() {
            return Ok(false);
        }

        debug.has_title = true;

        let is_cloaked = self.is_cloaked().unwrap_or_default();

        debug.is_cloaked = is_cloaked;

        let mut allow_cloaked = false;

        if let Some(event) = event {
            if matches!(
                event,
                WindowManagerEvent::Hide(_, _) | WindowManagerEvent::Cloak(_, _)
            ) {
                allow_cloaked = true;
            }
        }

        debug.allow_cloaked = allow_cloaked;

        match (allow_cloaked, is_cloaked) {
            // If allowing cloaked windows, we don't need to check the cloaked status
            (true, _) |
            // If not allowing cloaked windows, we need to ensure the window is not cloaked
            (false, false) => {
                if let (Ok(title), Ok(exe_name), Ok(class), Ok(path)) = (self.title(), self.exe(), self.class(), self.path()) {
                    debug.title = Some(title.clone());
                    debug.exe_name = Some(exe_name.clone());
                    debug.class = Some(class.clone());
                    debug.path = Some(path.clone());
                    // calls for styles can fail quite often for events with windows that aren't really "windows"
                    // since we have moved up calls of should_manage to the beginning of the process_event handler,
                    // we should handle failures here gracefully to be able to continue the execution of process_event
                    if let (Ok(style), Ok(ex_style)) = (&self.style(), &self.ex_style()) {
                        debug.window_style = Some(*style);
                        debug.extended_window_style = Some(*ex_style);
                        let eligible = window_is_eligible(&title, &exe_name, &class, &path, style, ex_style, event, debug);
                        debug.should_manage = eligible;
                        return Ok(eligible);
                    }
                }
            }
            _ => {}
        }

        Ok(false)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RuleDebug {
    pub should_manage: bool,
    pub is_window: bool,
    pub has_title: bool,
    pub is_cloaked: bool,
    pub allow_cloaked: bool,
    pub window_style: Option<WindowStyle>,
    pub extended_window_style: Option<ExtendedWindowStyle>,
    pub title: Option<String>,
    pub exe_name: Option<String>,
    pub class: Option<String>,
    pub path: Option<String>,
    pub matches_permaignore_class: Option<String>,
    pub matches_float_identifier: Option<MatchingRule>,
    pub matches_managed_override: Option<MatchingRule>,
    pub matches_layered_whitelist: Option<MatchingRule>,
    pub matches_wsl2_gui: Option<String>,
    pub matches_no_titlebar: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn window_is_eligible(
    title: &String,
    exe_name: &String,
    class: &String,
    path: &str,
    style: &WindowStyle,
    ex_style: &ExtendedWindowStyle,
    event: Option<WindowManagerEvent>,
    debug: &mut RuleDebug,
) -> bool {
    {
        let permaignore_classes = PERMAIGNORE_CLASSES.lock();
        if permaignore_classes.contains(class) {
            debug.matches_permaignore_class = Some(class.clone());
            return false;
        }
    }

    let regex_identifiers = REGEX_IDENTIFIERS.lock();

    let float_identifiers = FLOAT_IDENTIFIERS.lock();
    let should_float = if let Some(rule) = should_act(
        title,
        exe_name,
        class,
        path,
        &float_identifiers,
        &regex_identifiers,
    ) {
        debug.matches_float_identifier = Some(rule);
        true
    } else {
        false
    };

    let manage_identifiers = MANAGE_IDENTIFIERS.lock();
    let managed_override = if let Some(rule) = should_act(
        title,
        exe_name,
        class,
        path,
        &manage_identifiers,
        &regex_identifiers,
    ) {
        debug.matches_managed_override = Some(rule);
        true
    } else {
        false
    };

    if should_float && !managed_override {
        return false;
    }

    let layered_whitelist = LAYERED_WHITELIST.lock();
    let allow_layered = if let Some(rule) = should_act(
        title,
        exe_name,
        class,
        path,
        &layered_whitelist,
        &regex_identifiers,
    ) {
        debug.matches_layered_whitelist = Some(rule);
        true
    } else {
        false
    };

    // TODO: might need this for transparency
    // let allow_layered = true;

    let allow_wsl2_gui = {
        let wsl2_ui_processes = WSL2_UI_PROCESSES.lock();
        let allow = wsl2_ui_processes.contains(exe_name);
        if allow {
            debug.matches_wsl2_gui = Some(exe_name.clone())
        }

        allow
    };

    let allow_titlebar_removed = {
        let titlebars_removed = NO_TITLEBAR.lock();
        titlebars_removed.contains(exe_name)
    };

    if exe_name.contains("firefox") {
        std::thread::sleep(Duration::from_millis(10));
    }

    if (allow_wsl2_gui || allow_titlebar_removed || style.contains(WindowStyle::CAPTION) && ex_style.contains(ExtendedWindowStyle::WINDOWEDGE))
                        && !ex_style.contains(ExtendedWindowStyle::DLGMODALFRAME)
                        // Get a lot of dupe events coming through that make the redrawing go crazy
                        // on FocusChange events if I don't filter out this one. But, if we are
                        // allowing a specific layered window on the whitelist (like Steam), it should
                        // pass this check
                        && (allow_layered || !ex_style.contains(ExtendedWindowStyle::LAYERED))
        || managed_override
    {
        return true;
    } else if let Some(event) = event {
        tracing::debug!(
            "ignoring (exe: {}, title: {}, event: {})",
            exe_name,
            title,
            event
        );
    }

    false
}

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub fn should_act(
    title: &str,
    exe_name: &str,
    class: &str,
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
                    class,
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
                        class,
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
    class: &str,
    path: &str,
    identifier: &IdWithIdentifier,
    regex_identifiers: &HashMap<String, Regex>,
) -> bool {
    let mut should_act = false;

    match identifier.matching_strategy {
        None => {
            panic!("there is no matching strategy identified for this rule");
        }
        Some(MatchingStrategy::Legacy) => match identifier.kind {
            ApplicationIdentifier::Title => {
                if title.starts_with(&identifier.id) || title.ends_with(&identifier.id) {
                    should_act = true;
                }
            }
            ApplicationIdentifier::Class => {
                if class.starts_with(&identifier.id) || class.ends_with(&identifier.id) {
                    should_act = true;
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
                if class.eq(&identifier.id) {
                    should_act = true;
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
                if !class.eq(&identifier.id) {
                    should_act = true;
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
                if class.starts_with(&identifier.id) {
                    should_act = true;
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
                if !class.starts_with(&identifier.id) {
                    should_act = true;
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
                if class.ends_with(&identifier.id) {
                    should_act = true;
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
                if !class.ends_with(&identifier.id) {
                    should_act = true;
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
                if class.contains(&identifier.id) {
                    should_act = true;
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
                if !class.contains(&identifier.id) {
                    should_act = true;
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
                if let Some(re) = regex_identifiers.get(&identifier.id) {
                    if re.is_match(title) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Class => {
                if let Some(re) = regex_identifiers.get(&identifier.id) {
                    if re.is_match(class) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Exe => {
                if let Some(re) = regex_identifiers.get(&identifier.id) {
                    if re.is_match(exe_name) {
                        should_act = true;
                    }
                }
            }
            ApplicationIdentifier::Path => {
                if let Some(re) = regex_identifiers.get(&identifier.id) {
                    if re.is_match(path) {
                        should_act = true;
                    }
                }
            }
        },
    }

    should_act
}
