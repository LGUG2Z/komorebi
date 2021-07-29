use std::convert::TryFrom;
use std::fmt::Display;
use std::fmt::Formatter;

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;

use bindings::Windows::Win32::Foundation::HWND;
use komorebi_core::Rect;

use crate::styles::GwlExStyle;
use crate::styles::GwlStyle;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api::WindowsApi;
use crate::FLOAT_CLASSES;
use crate::FLOAT_EXES;
use crate::FLOAT_TITLES;
use crate::LAYERED_EXE_WHITELIST;

#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub(crate) hwnd: isize,
    pub(crate) original_style: GwlStyle,
}

impl Display for Window {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut display = format!("(hwnd: {}", self.hwnd);

        if let Ok(title) = self.title() {
            display.push_str(&format!(", title: {}", title));
        }

        if let Ok(exe) = self.exe() {
            display.push_str(&format!(", exe: {}", exe));
        }

        if let Ok(class) = self.class() {
            display.push_str(&format!(", class: {}", class));
        }

        display.push(')');

        write!(f, "{}", display)
    }
}

impl Window {
    pub const fn hwnd(&self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn set_position(&mut self, layout: &Rect) -> Result<()> {
        WindowsApi::set_window_pos(self.hwnd(), layout)
    }

    pub fn hide(&self) {
        WindowsApi::hide_window(self.hwnd());
    }

    pub fn restore(&self) {
        WindowsApi::restore_window(self.hwnd());
    }

    pub fn focus(&self) -> Result<()> {
        // Attach komorebi thread to Window thread
        let (_, window_thread_id) = WindowsApi::window_thread_process_id(self.hwnd());
        let current_thread_id = WindowsApi::current_thread_id();
        WindowsApi::attach_thread_input(current_thread_id, window_thread_id, true)?;

        // Raise Window to foreground
        WindowsApi::set_foreground_window(self.hwnd())?;

        // Center cursor in Window
        WindowsApi::center_cursor_in_rect(&WindowsApi::window_rect(self.hwnd())?)?;

        // This isn't really needed when the above command works as expected via AHK
        WindowsApi::set_focus(self.hwnd())
    }

    pub fn update_style(&self, style: GwlStyle) -> Result<()> {
        WindowsApi::update_style(self.hwnd(), isize::try_from(style.bits())?)
    }

    pub fn restore_style(&self) -> Result<()> {
        self.update_style(self.original_style)
    }

    pub fn remove_border(&self) -> Result<()> {
        let mut style = self.style()?;
        style.remove(GwlStyle::BORDER);
        self.update_style(style)
    }

    pub fn add_border(&self) -> Result<()> {
        let mut style = self.style()?;
        style.insert(GwlStyle::BORDER);
        self.update_style(style)
    }

    pub fn remove_padding_and_title_bar(&self) -> Result<()> {
        let mut style = self.style()?;
        style.remove(GwlStyle::THICKFRAME);
        style.remove(GwlStyle::CAPTION);
        self.update_style(style)
    }

    pub fn add_padding_padding_and_title_bar(&self) -> Result<()> {
        let mut style = self.style()?;
        style.insert(GwlStyle::THICKFRAME);
        style.insert(GwlStyle::CAPTION);
        self.update_style(style)
    }

    pub fn style(&self) -> Result<GwlStyle> {
        let bits = u32::try_from(WindowsApi::gwl_style(self.hwnd())?)?;
        GwlStyle::from_bits(bits).context("there is no gwl style")
    }

    pub fn ex_style(&self) -> Result<GwlExStyle> {
        let bits = u32::try_from(WindowsApi::gwl_ex_style(self.hwnd())?)?;
        GwlExStyle::from_bits(bits).context("there is no gwl style")
    }

    pub fn title(&self) -> Result<String> {
        WindowsApi::window_text_w(self.hwnd())
    }

    pub fn exe(&self) -> Result<String> {
        let (process_id, _) = WindowsApi::window_thread_process_id(self.hwnd());
        WindowsApi::exe(WindowsApi::process_handle(process_id)?)
    }

    pub fn class(&self) -> Result<String> {
        WindowsApi::real_window_class_w(self.hwnd())
    }

    pub fn is_cloaked(&self) -> Result<bool> {
        WindowsApi::is_window_cloaked(self.hwnd())
    }

    pub fn is_window(self) -> bool {
        WindowsApi::is_window(self.hwnd())
    }

    pub fn should_manage(&self, event: Option<WindowManagerEvent>) -> Result<bool> {
        let classes = FLOAT_CLASSES.lock().unwrap();
        let exes = FLOAT_EXES.lock().unwrap();
        let titles = FLOAT_TITLES.lock().unwrap();

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
                if let (Ok(title), Ok(exe_name)) = (self.title(), self.exe()) {
                    if titles.contains(&title) {
                        return Ok(false);
                    }

                    if exes.contains(&exe_name) {
                        return Ok(false);
                    }

                    if let Ok(class) = self.class() {
                        if classes.contains(&class) {
                            return Ok(false);
                        }
                    }

                    let allow_layered = LAYERED_EXE_WHITELIST.lock().unwrap().contains(&exe_name);

                    let style = self.style()?;
                    let ex_style = self.ex_style()?;

                    if style.contains(GwlStyle::CAPTION)
                        && ex_style.contains(GwlExStyle::WINDOWEDGE)
                        && !ex_style.contains(GwlExStyle::DLGMODALFRAME)
                        // Get a lot of dupe events coming through that make the redrawing go crazy
                        // on FocusChange events if I don't filter out this one. But, if we are
                        // allowing a specific layered window on the whitelist (like Steam), it should
                        // pass this check
                        && (allow_layered || !ex_style.contains(GwlExStyle::LAYERED))
                    {
                        Ok(true)
                    } else {
                        if let Some(event) = event {
                            tracing::debug!("ignoring window: {} (event: {})", self, event);
                        }

                        Ok(false)
                    }
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        }
    }
}
