use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::widgets::widget::BarWidget;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::WidgetText;
use eframe::egui::text::LayoutJob;
use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;
use std::time::Instant;
use windows::Win32::Globalization::LCIDToLocaleName;
use windows::Win32::Globalization::LOCALE_ALLOW_NEUTRAL_NAMES;
use windows::Win32::System::SystemServices::LOCALE_NAME_MAX_LENGTH;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

const DEFAULT_DATA_REFRESH_INTERVAL: u64 = 1;
const ERROR_TEXT: &str = "Error";

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KeyboardConfig {
    /// Enable the Input widget
    pub enable: bool,
    /// Data refresh interval (default: 1 second)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<KeyboardConfig> for Keyboard {
    fn from(value: KeyboardConfig) -> Self {
        let data_refresh_interval = value
            .data_refresh_interval
            .unwrap_or(DEFAULT_DATA_REFRESH_INTERVAL);

        Self {
            enable: value.enable,
            data_refresh_interval,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::IconAndText),
            last_updated: Instant::now(),
            lang_name: get_lang(),
        }
    }
}

pub struct Keyboard {
    pub enable: bool,
    data_refresh_interval: u64,
    label_prefix: LabelPrefix,
    last_updated: Instant,
    lang_name: String,
}

/// Retrieves the name of the active keyboard layout for the current foreground window.
///
/// This function determines the active keyboard layout by querying the system for the
/// foreground window's thread ID and its associated keyboard layout. It then attempts
/// to retrieve the locale name corresponding to the keyboard layout.
///
/// # Failure Cases
///
/// This function can fail in two distinct scenarios:
///
/// 1. **Failure to Retrieve the Locale Name**:
///    If the system fails to retrieve the locale name (e.g., due to an invalid or unsupported
///    language identifier), the function will return `Err(())`.
///
/// 2. **Invalid UTF-16 Characters in the Locale Name**:
///    If the retrieved locale name contains invalid UTF-16 sequences, the conversion to a Rust
///    `String` will fail, and the function will return `Err(())`.
///
/// # Returns
///
/// - `Ok(String)`: The name of the active keyboard layout as a valid UTF-8 string.
/// - `Err(())`: Indicates that the function failed to retrieve the locale name or encountered
///   invalid UTF-16 characters during conversion.
fn get_active_keyboard_layout() -> Result<String, ()> {
    let foreground_window_tid = unsafe { GetWindowThreadProcessId(GetForegroundWindow(), None) };
    let lcid = unsafe { GetKeyboardLayout(foreground_window_tid) };

    // Extract the low word (language identifier) from the keyboard layout handle.
    let lang_id = (lcid.0 as u32) & 0xFFFF;
    let mut locale_name_buffer = [0; LOCALE_NAME_MAX_LENGTH as usize];
    let char_count = unsafe {
        LCIDToLocaleName(
            lang_id,
            Some(&mut locale_name_buffer),
            LOCALE_ALLOW_NEUTRAL_NAMES,
        )
    };

    match char_count {
        0 => Err(()),
        _ => String::from_utf16(&locale_name_buffer[..char_count as usize]).map_err(|_| ()),
    }
}

/// Retrieves the name of the active keyboard layout or a fallback error message.
///
/// # Behavior
///
/// - **Success Case**:
///   If [`get_active_keyboard_layout`] succeeds, this function returns the retrieved keyboard
///   layout name as a `String`.
///
/// - **Failure Case**:
///   If [`get_active_keyboard_layout`] fails, this function returns the value of `ERROR_TEXT`
///   as a fallback message. This ensures that the function always returns a valid `String`,
///   even in error scenarios.
///
/// # Returns
///
/// A `String` representing either:
/// - The name of the active keyboard layout, or
/// - The fallback error message (`ERROR_TEXT`) if the layout name cannot be retrieved.
fn get_lang() -> String {
    get_active_keyboard_layout()
        .map(|l| l.trim_end_matches('\0').to_string())
        .unwrap_or_else(|_| ERROR_TEXT.to_string())
}

impl Keyboard {
    fn output(&mut self) -> String {
        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(self.data_refresh_interval) {
            self.last_updated = now;
            self.lang_name = get_lang();
        }

        match self.label_prefix {
            LabelPrefix::Text | LabelPrefix::IconAndText => format!("KB: {}", self.lang_name),
            LabelPrefix::None | LabelPrefix::Icon => self.lang_name.clone(),
        }
    }
}

impl BarWidget for Keyboard {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::KEYBOARD.to_string()
                        }
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
                    config.icon_font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat {
                        font_id: config.text_font_id.clone(),
                        color: ctx.style().visuals.text_color(),
                        valign: Align::Center,
                        ..Default::default()
                    },
                );

                config.apply_on_widget(true, ui, |ui| {
                    ui.add(Label::new(WidgetText::LayoutJob(layout_job.clone())).selectable(false))
                });
            }
        }
    }
}
