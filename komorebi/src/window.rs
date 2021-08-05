use std::convert::TryFrom;
use std::fmt::Display;
use std::fmt::Formatter;

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use serde::ser::SerializeStruct;
use serde::Serialize;
use serde::Serializer;

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

impl Serialize for Window {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Window", 5)?;
        state.serialize_field("hwnd", &self.hwnd)?;
        state.serialize_field("title", &self.title().expect("could not get window title"))?;
        state.serialize_field("exe", &self.exe().expect("could not get window exe"))?;
        state.serialize_field("class", &self.class().expect("could not get window class"))?;
        state.serialize_field(
            "rect",
            &WindowsApi::window_rect(self.hwnd()).expect("could not get window rect"),
        )?;
        state.end()
    }
}

impl Window {
    pub const fn hwnd(self) -> HWND {
        HWND(self.hwnd)
    }

    pub fn set_position(&mut self, layout: &Rect, top: bool) -> Result<()> {
        // NOTE: This is how the border variable below was calculated; every time this code was
        // run on any window in any position, the generated border was always the same, so I am
        // hard coding the border Rect to avoid two calls to set_window_pos and making the screen
        // flicker on container/window movement. Still not 100% sure if this is DPI-aware.

        // Set the new position first to be able to get the extended frame bounds
        // WindowsApi::set_window_pos(self.hwnd(), layout, false, false)?;
        // let mut rect = WindowsApi::window_rect(self.hwnd())?;

        // Get the extended frame bounds of the new position
        // let frame = WindowsApi::window_rect_with_extended_frame_bounds(self.hwnd())?;

        // Calculate the invisible border diff
        // let border = Rect {
        //     left: frame.left - rect.left,
        //     top: frame.top - rect.top,
        //     right: rect.right - frame.right,
        //     bottom: rect.bottom - frame.bottom,
        // };

        let mut rect = *layout;
        let border = Rect {
            left: 12,
            top: 0,
            right: 24,
            bottom: 12,
        };

        // Remove the invisible border
        rect.left -= border.left;
        rect.top -= border.top;
        rect.right += border.right;
        rect.bottom += border.bottom;

        WindowsApi::position_window(self.hwnd(), &rect, top)
    }

    pub fn hide(self) {
        WindowsApi::hide_window(self.hwnd());
    }

    pub fn restore(self) {
        WindowsApi::restore_window(self.hwnd());
    }

    pub fn focus(self) -> Result<()> {
        // Attach komorebi thread to Window thread
        let (_, window_thread_id) = WindowsApi::window_thread_process_id(self.hwnd());
        let current_thread_id = WindowsApi::current_thread_id();
        WindowsApi::attach_thread_input(current_thread_id, window_thread_id, true)?;

        // Raise Window to foreground
        match WindowsApi::set_foreground_window(self.hwnd()) {
            Ok(_) => {}
            Err(error) => {
                tracing::error!(
                    "could not set as foreground window, but continuing execution of focus(): {}",
                    error
                );
            }
        };

        // Center cursor in Window
        WindowsApi::center_cursor_in_rect(&WindowsApi::window_rect(self.hwnd())?)?;

        // This isn't really needed when the above command works as expected via AHK
        WindowsApi::set_focus(self.hwnd())
    }

    pub fn update_style(self, style: GwlStyle) -> Result<()> {
        WindowsApi::update_style(self.hwnd(), isize::try_from(style.bits())?)
    }

    pub fn style(self) -> Result<GwlStyle> {
        let bits = u32::try_from(WindowsApi::gwl_style(self.hwnd())?)?;
        GwlStyle::from_bits(bits).context("there is no gwl style")
    }

    pub fn ex_style(self) -> Result<GwlExStyle> {
        let bits = u32::try_from(WindowsApi::gwl_ex_style(self.hwnd())?)?;
        GwlExStyle::from_bits(bits).context("there is no gwl style")
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
                        if event.is_some() {
                            tracing::debug!("ignoring (exe: {}, title: {})", exe_name, title);
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
