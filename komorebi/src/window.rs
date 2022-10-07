use std::convert::TryFrom;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Write as _;

use color_eyre::eyre::anyhow;
use color_eyre::Result;
use schemars::JsonSchema;
use serde::ser::Error;
use serde::ser::SerializeStruct;
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
use crate::BORDER_OVERFLOW_IDENTIFIERS;
use crate::FLOAT_IDENTIFIERS;
use crate::HIDDEN_HWNDS;
use crate::HIDING_BEHAVIOUR;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::WSL2_UI_PROCESSES;

#[derive(Debug, Clone, Copy, JsonSchema)]
pub struct Window {
    pub(crate) hwnd: isize,
}

impl Display for Window {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut display = format!("(hwnd: {}", self.hwnd);

        if let Ok(title) = self.title() {
            write!(display, ", title: {}", title)?;
        }

        if let Ok(exe) = self.exe() {
            write!(display, ", exe: {}", exe)?;
        }

        if let Ok(class) = self.class() {
            write!(display, ", class: {}", class)?;
        }

        write!(display, ")")?;

        write!(f, "{}", display)
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

    pub fn center(&mut self, work_area: &Rect, invisible_borders: &Rect) -> Result<()> {
        let half_width = work_area.right / 2;
        let half_weight = work_area.bottom / 2;

        self.set_position(
            &Rect {
                left: work_area.left + ((work_area.right - half_width) / 2),
                top: work_area.top + ((work_area.bottom - half_weight) / 2),
                right: half_width,
                bottom: half_weight,
            },
            invisible_borders,
            true,
        )
    }

    pub fn set_position(
        &mut self,
        layout: &Rect,
        invisible_borders: &Rect,
        top: bool,
    ) -> Result<()> {
        let mut rect = *layout;
        let mut should_remove_border = true;

        let border_overflows = BORDER_OVERFLOW_IDENTIFIERS.lock();
        if border_overflows.contains(&self.title()?)
            || border_overflows.contains(&self.exe()?)
            || border_overflows.contains(&self.class()?)
        {
            should_remove_border = false;
        }

        if should_remove_border {
            // Remove the invisible borders
            rect.left -= invisible_borders.left;
            rect.top -= invisible_borders.top;
            rect.right += invisible_borders.right;
            rect.bottom += invisible_borders.bottom;
        }

        WindowsApi::position_window(self.hwnd(), &rect, top)
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

        WindowsApi::restore_window(self.hwnd());
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

    pub fn raise(self) {
        // Attach komorebi thread to Window thread
        let (_, window_thread_id) = WindowsApi::window_thread_process_id(self.hwnd());
        let current_thread_id = WindowsApi::current_thread_id();

        // This can be allowed to fail if a window doesn't have a message queue or if a journal record
        // hook has been installed
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-attachthreadinput#remarks
        match WindowsApi::attach_thread_input(current_thread_id, window_thread_id, true) {
            Ok(()) => {}
            Err(error) => {
                tracing::error!(
                    "could not attach to window thread input processing mechanism, but continuing execution of raise(): {}",
                    error
                );
            }
        };

        // Raise Window to foreground
        match WindowsApi::set_foreground_window(self.hwnd()) {
            Ok(_) => {}
            Err(error) => {
                tracing::error!(
                    "could not set as foreground window, but continuing execution of raise(): {}",
                    error
                );
            }
        };

        // This isn't really needed when the above command works as expected via AHK
        match WindowsApi::set_focus(self.hwnd()) {
            Ok(_) => {}
            Err(error) => {
                tracing::error!(
                    "could not set focus, but continuing execution of raise(): {}",
                    error
                );
            }
        };

        match WindowsApi::attach_thread_input(current_thread_id, window_thread_id, false) {
            Ok(()) => {}
            Err(error) => {
                tracing::error!(
                    "could not detach from window thread input processing mechanism, but continuing execution of raise(): {}",
                    error
                );
            }
        };
    }

    pub fn focus(self, mouse_follows_focus: bool) -> Result<()> {
        // Attach komorebi thread to Window thread
        let (_, window_thread_id) = WindowsApi::window_thread_process_id(self.hwnd());
        let current_thread_id = WindowsApi::current_thread_id();

        // This can be allowed to fail if a window doesn't have a message queue or if a journal record
        // hook has been installed
        // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-attachthreadinput#remarks
        match WindowsApi::attach_thread_input(current_thread_id, window_thread_id, true) {
            Ok(()) => {}
            Err(error) => {
                tracing::error!(
                    "could not attach to window thread input processing mechanism, but continuing execution of focus(): {}",
                    error
                );
            }
        };

        // Raise Window to foreground
        let mut foregrounded = false;
        while !foregrounded {
            match WindowsApi::set_foreground_window(self.hwnd()) {
                Ok(_) => {
                    foregrounded = true;
                }
                Err(error) => {
                    tracing::error!(
                        "could not set as foreground window, but continuing execution of focus(): {}",
                        error
                    );
                }
            };
        }

        // Center cursor in Window
        if mouse_follows_focus {
            WindowsApi::center_cursor_in_rect(&WindowsApi::window_rect(self.hwnd())?)?;
        }

        // This isn't really needed when the above command works as expected via AHK
        match WindowsApi::set_focus(self.hwnd()) {
            Ok(_) => {}
            Err(error) => {
                tracing::error!(
                    "could not set focus, but continuing execution of focus(): {}",
                    error
                );
            }
        };

        match WindowsApi::attach_thread_input(current_thread_id, window_thread_id, false) {
            Ok(()) => {}
            Err(error) => {
                tracing::error!(
                    "could not detach from window thread input processing mechanism, but continuing execution of focus(): {}",
                    error
                );
            }
        };

        Ok(())
    }

    #[allow(dead_code)]
    pub fn update_style(self, style: WindowStyle) -> Result<()> {
        WindowsApi::update_style(self.hwnd(), isize::try_from(style.bits())?)
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

    pub fn exe(self) -> Result<String> {
        let (process_id, _) = WindowsApi::window_thread_process_id(self.hwnd());
        WindowsApi::exe(WindowsApi::process_handle(process_id)?)
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

    #[tracing::instrument(fields(exe, title))]
    pub fn should_manage(self, event: Option<WindowManagerEvent>) -> Result<bool> {
        if let Some(WindowManagerEvent::MonitorPoll(_, _)) = event {
            return Ok(true);
        }

        #[allow(clippy::question_mark)]
        if self.title().is_err() {
            return Ok(false);
        }

        let is_cloaked = self.is_cloaked()?;

        let mut allow_cloaked = false;
        if let Some(WindowManagerEvent::Hide(_, _)) = event {
            allow_cloaked = true;
        }

        match (allow_cloaked, is_cloaked) {
            // If allowing cloaked windows, we don't need to check the cloaked status
            (true, _) |
            // If not allowing cloaked windows, we need to ensure the window is not cloaked
            (false, false) => {
                if let (Ok(title), Ok(exe_name), Ok(class)) = (self.title(), self.exe(), self.class()) {
                    return Ok(window_is_eligible(&title, &exe_name, &class, self.style()?, self.ex_style()?, event));
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
    style: WindowStyle,
    ex_style: ExtendedWindowStyle,
    event: Option<WindowManagerEvent>,
) -> bool {
    let mut should_float = false;
    let mut matched_identifier = None;

    {
        let float_identifiers = FLOAT_IDENTIFIERS.lock();
        for identifier in float_identifiers.iter() {
            if title.starts_with(identifier) || title.ends_with(identifier) {
                should_float = true;
                matched_identifier = Option::from(ApplicationIdentifier::Title);
            }

            if class.starts_with(identifier) || class.ends_with(identifier) {
                should_float = true;
                matched_identifier = Option::from(ApplicationIdentifier::Class);
            }

            if identifier == exe_name {
                should_float = true;
                matched_identifier = Option::from(ApplicationIdentifier::Exe);
            }
        }
    };

    let managed_override = {
        let manage_identifiers = MANAGE_IDENTIFIERS.lock();
        matched_identifier.map_or_else(
            || {
                manage_identifiers.contains(exe_name)
                    || manage_identifiers.contains(class)
                    || manage_identifiers.contains(title)
            },
            |matched_identifier| match matched_identifier {
                ApplicationIdentifier::Exe => manage_identifiers.contains(exe_name),
                ApplicationIdentifier::Class => manage_identifiers.contains(class),
                ApplicationIdentifier::Title => manage_identifiers.contains(title),
            },
        )
    };

    if should_float && !managed_override {
        return false;
    }

    let allow_layered = {
        let layered_whitelist = LAYERED_WHITELIST.lock();
        layered_whitelist.contains(exe_name)
            || layered_whitelist.contains(class)
            || layered_whitelist.contains(title)
    };

    let allow_wsl2_gui = {
        let wsl2_ui_processes = WSL2_UI_PROCESSES.lock();
        wsl2_ui_processes.contains(exe_name)
    };

    if (allow_wsl2_gui || style.contains(WindowStyle::CAPTION) && ex_style.contains(ExtendedWindowStyle::WINDOWEDGE))
                        && !ex_style.contains(ExtendedWindowStyle::DLGMODALFRAME)
                        // Get a lot of dupe events coming through that make the redrawing go crazy
                        // on FocusChange events if I don't filter out this one. But, if we are
                        // allowing a specific layered window on the whitelist (like Steam), it should
                        // pass this check
                        && (allow_layered || !ex_style.contains(ExtendedWindowStyle::LAYERED))
        || managed_override
    {
        return true;
    } else if event.is_some() {
        tracing::debug!("ignoring (exe: {}, title: {})", exe_name, title);
    }

    false
}
