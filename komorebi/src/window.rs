use crate::com::SetCloak;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Write as _;
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
    pub(crate) hwnd: isize,
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
    pub const fn hwnd(self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn center(&self, work_area: &Rect) -> Result<()> {
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

    pub fn set_position(&self, layout: &Rect, top: bool) -> Result<()> {
        let rect = *layout;
        WindowsApi::position_window(self.hwnd(), &rect, top)
    }

    pub fn is_maximized(self) -> bool {
        WindowsApi::is_zoomed(self.hwnd())
    }

    pub fn is_miminized(self) -> bool {
        WindowsApi::is_iconic(self.hwnd())
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

    #[tracing::instrument(fields(exe, title))]
    pub fn should_manage(self, event: Option<WindowManagerEvent>) -> Result<bool> {
        #[allow(clippy::question_mark)]
        if self.title().is_err() {
            return Ok(false);
        }

        let is_cloaked = self.is_cloaked()?;

        let mut allow_cloaked = false;

        if let Some(event) = event {
            if matches!(
                event,
                WindowManagerEvent::Hide(_, _) | WindowManagerEvent::Cloak(_, _)
            ) {
                allow_cloaked = true;
            }
        }

        match (allow_cloaked, is_cloaked) {
            // If allowing cloaked windows, we don't need to check the cloaked status
            (true, _) |
            // If not allowing cloaked windows, we need to ensure the window is not cloaked
            (false, false) => {
                if let (Ok(title), Ok(exe_name), Ok(class), Ok(path)) = (self.title(), self.exe(), self.class(), self.path()) {
                    // calls for styles can fail quite often for events with windows that aren't really "windows"
                    // since we have moved up calls of should_manage to the beginning of the process_event handler,
                    // we should handle failures here gracefully to be able to continue the execution of process_event
                    if let (Ok(style), Ok(ex_style)) = (&self.style(), &self.ex_style()) {
                        return Ok(window_is_eligible(&title, &exe_name, &class, &path, style, ex_style, event));
                    }
                }
            }
            _ => {}
        }

        Ok(false)
    }
}

fn window_is_eligible(
    title: &String,
    exe_name: &String,
    class: &String,
    path: &str,
    style: &WindowStyle,
    ex_style: &ExtendedWindowStyle,
    event: Option<WindowManagerEvent>,
) -> bool {
    {
        let permaignore_classes = PERMAIGNORE_CLASSES.lock();
        if permaignore_classes.contains(class) {
            return false;
        }
    }

    let regex_identifiers = REGEX_IDENTIFIERS.lock();

    let float_identifiers = FLOAT_IDENTIFIERS.lock();
    let should_float = should_act(
        title,
        exe_name,
        class,
        path,
        &float_identifiers,
        &regex_identifiers,
    );

    let manage_identifiers = MANAGE_IDENTIFIERS.lock();
    let managed_override = should_act(
        title,
        exe_name,
        class,
        path,
        &manage_identifiers,
        &regex_identifiers,
    );

    if should_float && !managed_override {
        return false;
    }

    let layered_whitelist = LAYERED_WHITELIST.lock();
    let allow_layered = should_act(
        title,
        exe_name,
        class,
        path,
        &layered_whitelist,
        &regex_identifiers,
    );

    // TODO: might need this for transparency
    // let allow_layered = true;

    let allow_wsl2_gui = {
        let wsl2_ui_processes = WSL2_UI_PROCESSES.lock();
        wsl2_ui_processes.contains(exe_name)
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
) -> bool {
    let mut should_act = false;
    for identifier in identifiers {
        match identifier {
            MatchingRule::Simple(identifier) => {
                if should_act_individual(
                    title,
                    exe_name,
                    class,
                    path,
                    identifier,
                    regex_identifiers,
                ) {
                    should_act = true
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
                    should_act = true;
                }
            }
        }
    }

    should_act
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
