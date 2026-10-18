use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
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
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::Globalization::GetDateFormatEx;
use windows::Win32::Globalization::GetUserDefaultLocaleName;
use windows::Win32::System::SystemServices::LOCALE_NAME_MAX_LENGTH;
use windows::core::PCWSTR;

const ERROR_TEXT: &str = "Date format error";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Windows-localized date widget configuration.
pub struct DateLocalizedConfig {
    /// Enable the DateLocalized widget.
    pub enable: bool,
    /// Windows date-picture format.
    pub format: DateLocalizedFormat,
    /// Display label prefix.
    pub label_prefix: Option<LabelPrefix>,
}

impl From<DateLocalizedConfig> for DateLocalized {
    fn from(value: DateLocalizedConfig) -> Self {
        Self {
            enable: value.enable,
            format: value.format,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
            last_state: String::new(),
            last_updated: Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Windows date-picture format.
pub enum DateLocalizedFormat {
    /// Month/Date/Year format.
    MonthDateYear,
    /// Year-Month-Date format.
    YearMonthDate,
    /// Date-Month-Year format.
    DateMonthYear,
    /// Day Date Month Year format.
    DayDateMonthYear,
    /// Custom Windows date-picture format, not a Chrono/strftime format.
    #[cfg_attr(feature = "schemars", schemars(title = "Custom"))]
    Custom(String),
}

impl DateLocalizedFormat {
    fn next(&mut self) {
        match self {
            Self::MonthDateYear => *self = Self::YearMonthDate,
            Self::YearMonthDate => *self = Self::DateMonthYear,
            Self::DateMonthYear => *self = Self::DayDateMonthYear,
            Self::DayDateMonthYear => *self = Self::MonthDateYear,
            Self::Custom(_) => {}
        }
    }

    fn pattern(&self) -> &str {
        match self {
            Self::MonthDateYear => "MM'/'dd'/'yy",
            Self::YearMonthDate => "yyyy'-'MM'-'dd",
            Self::DateMonthYear => "d'-'MMM'-'yyyy",
            Self::DayDateMonthYear => "dddd d MMMM yyyy",
            Self::Custom(pattern) => pattern,
        }
    }
}

pub struct DateLocalized {
    pub enable: bool,
    pub format: DateLocalizedFormat,
    label_prefix: LabelPrefix,
    last_state: String,
    last_updated: Instant,
}

impl DateLocalized {
    fn output(&mut self) -> String {
        if self.last_updated.elapsed() > Duration::from_secs(1) {
            self.last_state = format_windows_date(self.format.pattern())
                .unwrap_or_else(|| ERROR_TEXT.to_string());
            self.last_updated = Instant::now();
        }

        self.last_state.clone()
    }
}

impl BarWidget for DateLocalized {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if !self.enable {
            return;
        }

        let mut output = self.output();
        let mut layout_job = LayoutJob::simple(
            match self.label_prefix {
                LabelPrefix::Icon | LabelPrefix::IconAndText => {
                    egui_phosphor::regular::CALENDAR_DOTS.to_string()
                }
                LabelPrefix::None | LabelPrefix::Text => String::new(),
            },
            config.icon_font_id.clone(),
            ctx.style().visuals.selection.stroke.color,
            100.0,
        );

        if let LabelPrefix::Text | LabelPrefix::IconAndText = self.label_prefix {
            output.insert_str(0, "DATE: ");
        }

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

        config.apply_on_widget(false, ui, |ui| {
            if SelectableFrame::new(false)
                .show(ui, |ui| {
                    ui.add(
                        Label::new(WidgetText::LayoutJob(Arc::from(layout_job.clone())))
                            .selectable(false),
                    )
                })
                .clicked()
            {
                self.format.next();
            }
        });
    }
}

fn format_windows_date(pattern: &str) -> Option<String> {
    let mut locale = [0u16; LOCALE_NAME_MAX_LENGTH as usize];
    let locale_length = unsafe { GetUserDefaultLocaleName(&mut locale) };
    if locale_length == 0 {
        tracing::warn!("failed to get the Windows user locale");
        return None;
    }

    let locale = PCWSTR(locale.as_ptr());
    format_date_with_locale(locale, pattern, None)
}

fn format_date_with_locale(
    locale: PCWSTR,
    pattern: &str,
    date: Option<&SYSTEMTIME>,
) -> Option<String> {
    let mut pattern = pattern.encode_utf16().collect::<Vec<_>>();
    pattern.push(0);
    let pattern = PCWSTR(pattern.as_ptr());
    let mut size = unsafe {
        GetDateFormatEx(
            locale,
            Default::default(),
            date.map(|date| date as *const SYSTEMTIME),
            pattern,
            None,
            PCWSTR::null(),
        )
    };
    if size == 0 {
        tracing::warn!("failed to size Windows localized date");
        return None;
    }

    let mut output = vec![0u16; size as usize];
    size = unsafe {
        GetDateFormatEx(
            locale,
            Default::default(),
            date.map(|date| date as *const SYSTEMTIME),
            pattern,
            Some(&mut output),
            PCWSTR::null(),
        )
    };
    if size == 0 {
        tracing::warn!("failed to format Windows localized date");
        return None;
    }

    String::from_utf16(&output[..size as usize])
        .ok()
        .map(|output| output.trim_end_matches('\0').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::widget::WidgetConfig;

    #[test]
    fn maps_presets_to_windows_patterns() {
        assert_eq!(DateLocalizedFormat::MonthDateYear.pattern(), "MM'/'dd'/'yy");
        assert_eq!(
            DateLocalizedFormat::YearMonthDate.pattern(),
            "yyyy'-'MM'-'dd"
        );
        assert_eq!(
            DateLocalizedFormat::DateMonthYear.pattern(),
            "d'-'MMM'-'yyyy"
        );
        assert_eq!(
            DateLocalizedFormat::DayDateMonthYear.pattern(),
            "dddd d MMMM yyyy"
        );
    }

    #[test]
    fn preserves_custom_pattern() {
        let pattern = DateLocalizedFormat::Custom("dddd, d MMMM yyyy".to_string());
        assert_eq!(pattern.pattern(), "dddd, d MMMM yyyy");
    }

    #[cfg(windows)]
    #[test]
    fn formats_a_windows_date() {
        let locale = [
            b'e' as u16,
            b'n' as u16,
            b'-' as u16,
            b'U' as u16,
            b'S' as u16,
            0,
        ];
        let date = SYSTEMTIME {
            wYear: 2024,
            wMonth: 9,
            wDay: 8,
            ..Default::default()
        };
        let output =
            format_date_with_locale(PCWSTR(locale.as_ptr()), "yyyy'-'MM'-'dd", Some(&date));
        assert_eq!(output.as_deref(), Some("2024-09-08"));
    }

    #[cfg(windows)]
    #[test]
    fn formats_a_czech_windows_date() {
        let locale = [
            b'c' as u16,
            b's' as u16,
            b'-' as u16,
            b'C' as u16,
            b'Z' as u16,
            0,
        ];
        let date = SYSTEMTIME {
            wYear: 2024,
            wMonth: 9,
            wDay: 8,
            ..Default::default()
        };
        let output =
            format_date_with_locale(PCWSTR(locale.as_ptr()), "dddd d MMMM yyyy", Some(&date));
        assert!(output.is_some_and(|output| !output.is_empty()));
    }

    #[test]
    fn date_widgets_can_be_deserialized_together() {
        let widgets = serde_json::from_str::<Vec<WidgetConfig>>(
            r#"[
                {"Date":{"enable":true,"format":"YearMonthDate"}},
                {"DateLocalized":{"enable":true,"format":"DayDateMonthYear"}}
            ]"#,
        )
        .unwrap();

        assert_eq!(widgets.len(), 2);
        assert!(matches!(widgets[0], WidgetConfig::Date(_)));
        assert!(matches!(widgets[1], WidgetConfig::DateLocalized(_)));
    }
}
