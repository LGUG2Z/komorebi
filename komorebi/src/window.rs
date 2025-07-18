use crate::animation::lerp::Lerp;
use crate::animation::prefix::new_animation_key;
use crate::animation::prefix::AnimationPrefix;
use crate::animation::AnimationEngine;
use crate::animation::RenderDispatcher;
use crate::animation::ANIMATION_DURATION_GLOBAL;
use crate::animation::ANIMATION_DURATION_PER_ANIMATION;
use crate::animation::ANIMATION_ENABLED_GLOBAL;
use crate::animation::ANIMATION_ENABLED_PER_ANIMATION;
use crate::animation::ANIMATION_MANAGER;
use crate::animation::ANIMATION_STYLE_GLOBAL;
use crate::animation::ANIMATION_STYLE_PER_ANIMATION;
use crate::border_manager;
use crate::com::SetCloak;
use crate::core::config_generation::IdWithIdentifier;
use crate::core::config_generation::MatchingRule;
use crate::core::config_generation::MatchingStrategy;
use crate::core::ApplicationIdentifier;
use crate::core::HidingBehaviour;
use crate::core::Rect;
use crate::focus_manager;
use crate::stackbar_manager;
use crate::styles::ExtendedWindowStyle;
use crate::styles::WindowStyle;
use crate::transparency_manager;
use crate::window_manager_event::WindowManagerEvent;
use crate::windows_api;
use crate::windows_api::WindowsApi;
use crate::AnimationStyle;
use crate::FLOATING_APPLICATIONS;
use crate::FLOATING_WINDOW_TOGGLE_ASPECT_RATIO;
use crate::HIDDEN_HWNDS;
use crate::HIDING_BEHAVIOUR;
use crate::IGNORE_IDENTIFIERS;
use crate::LAYERED_WHITELIST;
use crate::MANAGE_IDENTIFIERS;
use crate::NO_TITLEBAR;
use crate::PERMAIGNORE_CLASSES;
use crate::REGEX_IDENTIFIERS;
use crate::SLOW_APPLICATION_COMPENSATION_TIME;
use crate::SLOW_APPLICATION_IDENTIFIERS;
use crate::WSL2_UI_PROCESSES;
use color_eyre::eyre;
use color_eyre::Result;
use crossbeam_utils::atomic::AtomicConsume;
use regex::Regex;
use serde::ser::SerializeStruct;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Write as _;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::time::Instant;
use strum::Display;
use strum::EnumString;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::UI::Input::KeyboardAndMouse::MOUSEEVENTF_LEFTDOWN;
use windows::Win32::UI::Input::KeyboardAndMouse::MOUSEEVENTF_LEFTUP;
use windows::Win32::UI::WindowsAndMessaging::PeekMessageA;

pub static MINIMUM_WIDTH: AtomicI32 = AtomicI32::new(0);
pub static MINIMUM_HEIGHT: AtomicI32 = AtomicI32::new(0);

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Window {
    pub hwnd: isize,
}

impl From<isize> for Window {
    fn from(value: isize) -> Self {
        Self { hwnd: value }
    }
}

impl From<HWND> for Window {
    fn from(value: HWND) -> Self {
        Self {
            hwnd: value.0 as isize,
        }
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
                .unwrap_or_else(|_| String::from("could not get window title")),
        )?;
        state.serialize_field(
            "exe",
            &self
                .exe()
                .unwrap_or_else(|_| String::from("could not get window exe")),
        )?;
        state.serialize_field(
            "class",
            &self
                .class()
                .unwrap_or_else(|_| String::from("could not get window class")),
        )?;
        state.serialize_field(
            "rect",
            &WindowsApi::window_rect(self.hwnd).unwrap_or_default(),
        )?;
        state.end()
    }
}

struct MovementRenderDispatcher {
    hwnd: isize,
    start_rect: Rect,
    target_rect: Rect,
    top: bool,
    style: AnimationStyle,
}

impl MovementRenderDispatcher {
    const PREFIX: AnimationPrefix = AnimationPrefix::Movement;

    pub fn new(
        hwnd: isize,
        start_rect: Rect,
        target_rect: Rect,
        top: bool,
        style: AnimationStyle,
    ) -> Self {
        Self {
            hwnd,
            start_rect,
            target_rect,
            top,
            style,
        }
    }
}

impl RenderDispatcher for MovementRenderDispatcher {
    fn get_animation_key(&self) -> String {
        new_animation_key(MovementRenderDispatcher::PREFIX, self.hwnd.to_string())
    }

    fn pre_render(&self) -> Result<()> {
        stackbar_manager::STACKBAR_TEMPORARILY_DISABLED.store(true, Ordering::SeqCst);
        stackbar_manager::send_notification();

        WindowsApi::send_enter_size_move(self.hwnd)?;

        Ok(())
    }

    fn render(&self, progress: f64) -> Result<()> {
        let new_rect = self.start_rect.lerp(self.target_rect, progress, self.style);

        // WindowsApi::pump_messages()?;
        unsafe { DwmFlush() }?;
        // WindowsApi::send_set_redraw(self.hwnd, false)?;
        // we don't check WINDOW_HANDLING_BEHAVIOUR here because animations
        // are always run on a separate thread
        // WindowsApi::position_window(self.hwnd, &new_rect, false, true)?;
        WindowsApi::move_window(self.hwnd, &new_rect, true)?;
        // WindowsApi::invalidate_rect(self.hwnd, None, true);

        // WindowsApi::update_window(self.hwnd)?;
        // WindowsApi::send_set_redraw(self.hwnd, true)?;
        // WindowsApi::send_display_change(self.hwnd)?;
        // WindowsApi::send_size(self.hwnd, new_rect.right as u32, new_rect.bottom as u32)?;
        WindowsApi::redraw_window(self.hwnd);
        // WindowsApi::pump_messages()?;
        // WindowsApi::send_paint_sync(self.hwnd)?;
        Ok(())
    }

    fn post_render(&self) -> Result<()> {
        // we don't add the async_window_pos flag here because animations
        // are always run on a separate thread
        WindowsApi::position_window(self.hwnd, &self.target_rect, self.top, false)?;
        WindowsApi::send_exit_size_move(self.hwnd)?;
        if ANIMATION_MANAGER
            .lock()
            .count_in_progress(MovementRenderDispatcher::PREFIX)
            == 0
        {
            if WindowsApi::foreground_window().unwrap_or_default() == self.hwnd {
                focus_manager::send_notification(self.hwnd)
            }

            stackbar_manager::STACKBAR_TEMPORARILY_DISABLED.store(false, Ordering::SeqCst);

            stackbar_manager::send_notification();
            transparency_manager::send_notification();
        }

        Ok(())
    }
}

struct TransparencyRenderDispatcher {
    hwnd: isize,
    start_opacity: u8,
    target_opacity: u8,
    style: AnimationStyle,
    is_opaque: bool,
}

impl TransparencyRenderDispatcher {
    const PREFIX: AnimationPrefix = AnimationPrefix::Transparency;

    pub fn new(
        hwnd: isize,
        is_opaque: bool,
        start_opacity: u8,
        target_opacity: u8,
        style: AnimationStyle,
    ) -> Self {
        Self {
            hwnd,
            start_opacity,
            target_opacity,
            style,
            is_opaque,
        }
    }
}

impl RenderDispatcher for TransparencyRenderDispatcher {
    fn get_animation_key(&self) -> String {
        new_animation_key(TransparencyRenderDispatcher::PREFIX, self.hwnd.to_string())
    }

    fn pre_render(&self) -> Result<()> {
        //transparent
        if !self.is_opaque {
            let window = Window::from(self.hwnd);
            let mut ex_style = window.ex_style()?;
            ex_style.insert(ExtendedWindowStyle::LAYERED);
            window.update_ex_style(&ex_style)?;
        }

        Ok(())
    }

    fn render(&self, progress: f64) -> Result<()> {
        WindowsApi::set_transparent(
            self.hwnd,
            self.start_opacity
                .lerp(self.target_opacity, progress, self.style),
        )
    }

    fn post_render(&self) -> Result<()> {
        //opaque
        if self.is_opaque {
            let window = Window::from(self.hwnd);
            let mut ex_style = window.ex_style()?;
            ex_style.remove(ExtendedWindowStyle::LAYERED);
            window.update_ex_style(&ex_style)?;
        }

        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Display, EnumString, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum AspectRatio {
    /// A predefined aspect ratio
    Predefined(PredefinedAspectRatio),
    /// A custom W:H aspect ratio
    Custom(i32, i32),
}

impl Default for AspectRatio {
    fn default() -> Self {
        AspectRatio::Predefined(PredefinedAspectRatio::default())
    }
}

#[derive(Copy, Clone, Debug, Default, Display, EnumString, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

impl Window {
    pub const fn hwnd(self) -> HWND {
        HWND(windows_api::as_ptr!(self.hwnd))
    }

    pub fn move_to_area(&mut self, current_area: &Rect, target_area: &Rect) -> Result<()> {
        let current_rect = WindowsApi::window_rect(self.hwnd)?;
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

        let is_maximized = &new_rect == target_area;
        if is_maximized {
            windows_api::WindowsApi::unmaximize_window(self.hwnd);
            let animation_enabled = ANIMATION_ENABLED_PER_ANIMATION.lock();
            let move_enabled = animation_enabled
                .get(&MovementRenderDispatcher::PREFIX)
                .is_some_and(|v| *v);
            drop(animation_enabled);

            if move_enabled || ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst) {
                let anim_count = ANIMATION_MANAGER
                    .lock()
                    .count_in_progress(MovementRenderDispatcher::PREFIX);
                self.set_position(&new_rect, true)?;
                let hwnd = self.hwnd;
                // Wait for the animation to finish before maximizing the window again, otherwise
                // we would be maximizing the window on the current monitor anyway
                thread::spawn(move || {
                    let mut new_anim_count = ANIMATION_MANAGER
                        .lock()
                        .count_in_progress(MovementRenderDispatcher::PREFIX);
                    let mut max_wait = 2000; // Max waiting time. No one will be using an animation longer than 2s, right? RIGHT??? WHY?
                    while new_anim_count > anim_count && max_wait > 0 {
                        thread::sleep(Duration::from_millis(10));
                        new_anim_count = ANIMATION_MANAGER
                            .lock()
                            .count_in_progress(MovementRenderDispatcher::PREFIX);
                        max_wait -= 1;
                    }
                    windows_api::WindowsApi::maximize_window(hwnd);
                });
            } else {
                self.set_position(&new_rect, true)?;
                windows_api::WindowsApi::maximize_window(self.hwnd);
            }
        } else {
            self.set_position(&new_rect, true)?;
        }

        Ok(())
    }

    pub fn center(&mut self, work_area: &Rect, resize: bool) -> Result<()> {
        let (target_width, target_height) = if resize {
            let (aspect_ratio_width, aspect_ratio_height) = FLOATING_WINDOW_TOGGLE_ASPECT_RATIO
                .lock()
                .width_and_height();
            let target_height = work_area.bottom / 2;
            let target_width = (target_height * aspect_ratio_width) / aspect_ratio_height;
            (target_width, target_height)
        } else {
            let current_rect = WindowsApi::window_rect(self.hwnd)?;
            (current_rect.right, current_rect.bottom)
        };

        let x = work_area.left + ((work_area.right - target_width) / 2);
        let y = work_area.top + ((work_area.bottom - target_height) / 2);

        self.set_position(
            &Rect {
                left: x,
                top: y,
                right: target_width,
                bottom: target_height,
            },
            true,
        )
    }

    pub fn set_position(&self, layout: &Rect, top: bool) -> Result<()> {
        let window_rect = WindowsApi::window_rect(self.hwnd)?;

        if window_rect.eq(layout) {
            return Ok(());
        }

        let animation_enabled = ANIMATION_ENABLED_PER_ANIMATION.lock();
        let move_enabled = animation_enabled.get(&MovementRenderDispatcher::PREFIX);

        if move_enabled.is_some_and(|enabled| *enabled)
            || ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst)
        {
            let duration = Duration::from_millis(
                *ANIMATION_DURATION_PER_ANIMATION
                    .lock()
                    .get(&MovementRenderDispatcher::PREFIX)
                    .unwrap_or(&ANIMATION_DURATION_GLOBAL.load(Ordering::SeqCst)),
            );
            let style = *ANIMATION_STYLE_PER_ANIMATION
                .lock()
                .get(&MovementRenderDispatcher::PREFIX)
                .unwrap_or(&ANIMATION_STYLE_GLOBAL.lock());

            let render_dispatcher =
                MovementRenderDispatcher::new(self.hwnd, window_rect, *layout, top, style);

            AnimationEngine::animate(render_dispatcher, duration)
        } else {
            WindowsApi::position_window(self.hwnd, layout, top, true)
        }
    }

    pub fn is_maximized(self) -> bool {
        WindowsApi::is_zoomed(self.hwnd)
    }

    pub fn is_miminized(self) -> bool {
        WindowsApi::is_iconic(self.hwnd)
    }

    pub fn is_visible(self) -> bool {
        WindowsApi::is_window_visible(self.hwnd)
    }

    pub fn hide_with_border(self, hide_border: bool) {
        let mut programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();
        if !programmatically_hidden_hwnds.contains(&self.hwnd) {
            programmatically_hidden_hwnds.push(self.hwnd);
        }

        let hiding_behaviour = HIDING_BEHAVIOUR.lock();
        match *hiding_behaviour {
            HidingBehaviour::Hide => WindowsApi::hide_window(self.hwnd),
            HidingBehaviour::Minimize => WindowsApi::minimize_window(self.hwnd),
            HidingBehaviour::Cloak => SetCloak(self.hwnd(), 1, 2),
        }
        if hide_border {
            border_manager::hide_border(self.hwnd);
        }
    }

    pub fn hide(self) {
        self.hide_with_border(true);
    }

    pub fn restore_with_border(self, restore_border: bool) {
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
                WindowsApi::restore_window(self.hwnd);
            }
            HidingBehaviour::Cloak => SetCloak(self.hwnd(), 1, 0),
        }
        if restore_border {
            border_manager::show_border(self.hwnd);
        }
    }

    pub fn restore(self) {
        self.restore_with_border(true);
    }

    pub fn minimize(self) {
        let exe = self.exe().unwrap_or_default();
        if !exe.contains("komorebi-bar") {
            WindowsApi::minimize_window(self.hwnd);
        }
    }

    pub fn close(self) -> Result<()> {
        WindowsApi::close_window(self.hwnd)
    }

    pub fn maximize(self) {
        let mut programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();
        if let Some(idx) = programmatically_hidden_hwnds
            .iter()
            .position(|&hwnd| hwnd == self.hwnd)
        {
            programmatically_hidden_hwnds.remove(idx);
        }

        WindowsApi::maximize_window(self.hwnd);
    }

    pub fn unmaximize(self) {
        let mut programmatically_hidden_hwnds = HIDDEN_HWNDS.lock();
        if let Some(idx) = programmatically_hidden_hwnds
            .iter()
            .position(|&hwnd| hwnd == self.hwnd)
        {
            programmatically_hidden_hwnds.remove(idx);
        }

        WindowsApi::unmaximize_window(self.hwnd);
    }

    pub fn focus(self, mouse_follows_focus: bool) -> Result<()> {
        // If the target window is already focused, do nothing.
        if let Ok(ihwnd) = WindowsApi::foreground_window() {
            if ihwnd == self.hwnd {
                // Center cursor in Window
                if mouse_follows_focus {
                    WindowsApi::center_cursor_in_rect(&WindowsApi::window_rect(self.hwnd)?)?;
                }

                return Ok(());
            }
        }

        WindowsApi::raise_and_focus_window(self.hwnd)?;

        // Center cursor in Window
        if mouse_follows_focus {
            WindowsApi::center_cursor_in_rect(&WindowsApi::window_rect(self.hwnd)?)?;
        }

        Ok(())
    }

    pub fn is_focused(self) -> bool {
        WindowsApi::foreground_window().unwrap_or_default() == self.hwnd
    }

    pub fn transparent(self) -> Result<()> {
        let animation_enabled = ANIMATION_ENABLED_PER_ANIMATION.lock();
        let transparent_enabled = animation_enabled.get(&TransparencyRenderDispatcher::PREFIX);

        if transparent_enabled.is_some_and(|enabled| *enabled)
            || ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst)
        {
            let duration = Duration::from_millis(
                *ANIMATION_DURATION_PER_ANIMATION
                    .lock()
                    .get(&TransparencyRenderDispatcher::PREFIX)
                    .unwrap_or(&ANIMATION_DURATION_GLOBAL.load(Ordering::SeqCst)),
            );
            let style = *ANIMATION_STYLE_PER_ANIMATION
                .lock()
                .get(&TransparencyRenderDispatcher::PREFIX)
                .unwrap_or(&ANIMATION_STYLE_GLOBAL.lock());

            let render_dispatcher = TransparencyRenderDispatcher::new(
                self.hwnd,
                false,
                WindowsApi::get_transparent(self.hwnd).unwrap_or(255),
                transparency_manager::TRANSPARENCY_ALPHA.load_consume(),
                style,
            );

            AnimationEngine::animate(render_dispatcher, duration)
        } else {
            let mut ex_style = self.ex_style()?;
            ex_style.insert(ExtendedWindowStyle::LAYERED);
            self.update_ex_style(&ex_style)?;
            WindowsApi::set_transparent(
                self.hwnd,
                transparency_manager::TRANSPARENCY_ALPHA.load_consume(),
            )
        }
    }

    pub fn opaque(self) -> Result<()> {
        let animation_enabled = ANIMATION_ENABLED_PER_ANIMATION.lock();
        let transparent_enabled = animation_enabled.get(&TransparencyRenderDispatcher::PREFIX);

        if transparent_enabled.is_some_and(|enabled| *enabled)
            || ANIMATION_ENABLED_GLOBAL.load(Ordering::SeqCst)
        {
            let duration = Duration::from_millis(
                *ANIMATION_DURATION_PER_ANIMATION
                    .lock()
                    .get(&TransparencyRenderDispatcher::PREFIX)
                    .unwrap_or(&ANIMATION_DURATION_GLOBAL.load(Ordering::SeqCst)),
            );
            let style = *ANIMATION_STYLE_PER_ANIMATION
                .lock()
                .get(&TransparencyRenderDispatcher::PREFIX)
                .unwrap_or(&ANIMATION_STYLE_GLOBAL.lock());

            let render_dispatcher = TransparencyRenderDispatcher::new(
                self.hwnd,
                true,
                WindowsApi::get_transparent(self.hwnd)
                    .unwrap_or(transparency_manager::TRANSPARENCY_ALPHA.load_consume()),
                255,
                style,
            );

            AnimationEngine::animate(render_dispatcher, duration)
        } else {
            let mut ex_style = self.ex_style()?;
            ex_style.remove(ExtendedWindowStyle::LAYERED);
            self.update_ex_style(&ex_style)
        }
    }

    pub fn set_accent(self, colour: u32) -> Result<()> {
        WindowsApi::set_window_accent(self.hwnd, Some(colour))
    }

    pub fn remove_accent(self) -> Result<()> {
        WindowsApi::set_window_accent(self.hwnd, None)
    }

    #[cfg(target_pointer_width = "64")]
    pub fn update_style(self, style: &WindowStyle) -> Result<()> {
        WindowsApi::update_style(self.hwnd, isize::try_from(style.bits())?)
    }

    #[cfg(target_pointer_width = "32")]
    pub fn update_style(self, style: &WindowStyle) -> Result<()> {
        WindowsApi::update_style(self.hwnd, i32::try_from(style.bits())?)
    }

    #[cfg(target_pointer_width = "64")]
    pub fn update_ex_style(self, style: &ExtendedWindowStyle) -> Result<()> {
        WindowsApi::update_ex_style(self.hwnd, isize::try_from(style.bits())?)
    }

    #[cfg(target_pointer_width = "32")]
    pub fn update_ex_style(self, style: &ExtendedWindowStyle) -> Result<()> {
        WindowsApi::update_ex_style(self.hwnd, i32::try_from(style.bits())?)
    }

    pub fn style(self) -> Result<WindowStyle> {
        let bits = u32::try_from(WindowsApi::gwl_style(self.hwnd)?)?;
        Ok(WindowStyle::from_bits_truncate(bits))
    }

    pub fn ex_style(self) -> Result<ExtendedWindowStyle> {
        let bits = u32::try_from(WindowsApi::gwl_ex_style(self.hwnd)?)?;
        Ok(ExtendedWindowStyle::from_bits_truncate(bits))
    }

    pub fn title(self) -> Result<String> {
        WindowsApi::window_text_w(self.hwnd)
    }

    pub fn path(self) -> Result<String> {
        let (process_id, _) = WindowsApi::window_thread_process_id(self.hwnd);
        let handle = WindowsApi::process_handle(process_id)?;
        let path = WindowsApi::exe_path(handle);
        WindowsApi::close_process(handle)?;
        path
    }

    pub fn exe(self) -> Result<String> {
        let (process_id, _) = WindowsApi::window_thread_process_id(self.hwnd);
        let handle = WindowsApi::process_handle(process_id)?;
        let exe = WindowsApi::exe(handle);
        WindowsApi::close_process(handle)?;
        exe
    }

    pub fn process_id(self) -> u32 {
        let (process_id, _) = WindowsApi::window_thread_process_id(self.hwnd);
        process_id
    }

    pub fn class(self) -> Result<String> {
        WindowsApi::real_window_class_w(self.hwnd)
    }

    pub fn is_cloaked(self) -> Result<bool> {
        WindowsApi::is_window_cloaked(self.hwnd)
    }

    pub fn is_window(self) -> bool {
        WindowsApi::is_window(self.hwnd)
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

    /// Raise the window to the top of the Z order, but do not activate or focus
    /// it. Use raise_and_focus_window to activate and focus a window.
    /// It also checks if there is a border attached to this window and if it is
    /// it raises it as well.
    pub fn raise(self) -> Result<()> {
        WindowsApi::raise_window(self.hwnd)?;
        if let Some(border_info) = crate::border_manager::window_border(self.hwnd) {
            WindowsApi::raise_window(border_info.border_hwnd)?;
        }
        Ok(())
    }

    /// Lower the window to the bottom of the Z order, but do not activate or focus
    /// it.
    /// It also checks if there is a border attached to this window and if it is
    /// it lowers it as well.
    pub fn lower(self) -> Result<()> {
        WindowsApi::lower_window(self.hwnd)?;
        if let Some(border_info) = crate::border_manager::window_border(self.hwnd) {
            WindowsApi::lower_window(border_info.border_hwnd)?;
        }
        Ok(())
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

        let rect = WindowsApi::window_rect(self.hwnd).unwrap_or_default();

        if rect.right < MINIMUM_WIDTH.load(Ordering::SeqCst) {
            return Ok(false);
        }

        debug.has_minimum_width = true;

        if rect.bottom < MINIMUM_HEIGHT.load(Ordering::SeqCst) {
            return Ok(false);
        }

        debug.has_minimum_height = true;

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
                        let eligible = window_is_eligible(self.hwnd, &title, &exe_name, &class, &path, style, ex_style, event, debug);
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
    pub has_minimum_width: bool,
    pub has_minimum_height: bool,
    pub has_title: bool,
    pub is_cloaked: bool,
    pub allow_cloaked: bool,
    pub allow_layered_transparency: bool,
    pub window_style: Option<WindowStyle>,
    pub extended_window_style: Option<ExtendedWindowStyle>,
    pub title: Option<String>,
    pub exe_name: Option<String>,
    pub class: Option<String>,
    pub path: Option<String>,
    pub matches_permaignore_class: Option<String>,
    pub matches_ignore_identifier: Option<MatchingRule>,
    pub matches_managed_override: Option<MatchingRule>,
    pub matches_layered_whitelist: Option<MatchingRule>,
    pub matches_floating_applications: Option<MatchingRule>,
    pub matches_wsl2_gui: Option<String>,
    pub matches_no_titlebar: Option<MatchingRule>,
}

#[allow(clippy::too_many_arguments)]
fn window_is_eligible(
    hwnd: isize,
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

    let ignore_identifiers = IGNORE_IDENTIFIERS.lock();
    let should_ignore = if let Some(rule) = should_act(
        title,
        exe_name,
        class,
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

    let floating_identifiers = FLOATING_APPLICATIONS.lock();
    if let Some(rule) = should_act(
        title,
        exe_name,
        class,
        path,
        &floating_identifiers,
        &regex_identifiers,
    ) {
        debug.matches_floating_applications = Some(rule);
    }

    if should_ignore && !managed_override {
        return false;
    }

    let layered_whitelist = LAYERED_WHITELIST.lock();
    let mut allow_layered = if let Some(rule) = should_act(
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

    let known_layered_hwnds = transparency_manager::known_hwnds();

    allow_layered = if known_layered_hwnds.contains(&hwnd)
        // we always want to process hide events for windows with transparency, even on other
        // monitors, because we don't want to be left with ghost tiles
        || matches!(event, Some(WindowManagerEvent::Hide(_, _)))
    {
        debug.allow_layered_transparency = true;
        true
    } else {
        allow_layered
    };

    let allow_wsl2_gui = {
        let wsl2_ui_processes = WSL2_UI_PROCESSES.lock();
        let allow = wsl2_ui_processes.contains(exe_name);
        if allow {
            debug.matches_wsl2_gui = Some(exe_name.clone())
        }

        allow
    };

    let titlebars_removed = NO_TITLEBAR.lock();
    let allow_titlebar_removed = if let Some(rule) = should_act(
        title,
        exe_name,
        class,
        path,
        &titlebars_removed,
        &regex_identifiers,
    ) {
        debug.matches_no_titlebar = Some(rule);
        true
    } else {
        false
    };

    {
        let slow_application_identifiers = SLOW_APPLICATION_IDENTIFIERS.lock();
        let should_sleep = should_act(
            title,
            exe_name,
            class,
            path,
            &slow_application_identifiers,
            &regex_identifiers,
        )
        .is_some();

        if should_sleep {
            std::thread::sleep(Duration::from_millis(
                SLOW_APPLICATION_COMPENSATION_TIME.load(Ordering::SeqCst),
            ));
        }
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
        None | Some(MatchingStrategy::Legacy) => match identifier.kind {
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
