use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::widget::BarWidget;
use chrono::Local;
use chrono_tz::Tz;
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

/// Custom format with additive modifiers for integer format specifiers
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CustomModifiers {
    /// Custom format (https://docs.rs/chrono/latest/chrono/format/strftime/index.html)
    format: String,
    /// Additive modifiers for integer format specifiers (e.g. { "%U": 1 } to increment the zero-indexed week number by 1)
    modifiers: std::collections::HashMap<String, i32>,
}

impl CustomModifiers {
    fn apply(&self, output: &str) -> String {
        let int_formatters = vec![
            "%Y", "%C", "%y", "%m", "%d", "%e", "%w", "%u", "%U", "%W", "%G", "%g", "%V", "%j",
            "%H", "%k", "%I", "%l", "%M", "%S", "%f",
        ];

        let mut modified_output = output.to_string();

        for (modifier, value) in &self.modifiers {
            // check if formatter is integer type
            if !int_formatters.contains(&modifier.as_str()) {
                continue;
            }

            // get the strftime value of modifier
            let formatted_modifier = Local::now().format(modifier).to_string();

            // find the gotten value in the original output
            if let Some(pos) = modified_output.find(&formatted_modifier) {
                let start = pos;
                let end = start + formatted_modifier.len();
                // replace that value with the modified value
                if let Ok(num) = formatted_modifier.parse::<i32>() {
                    modified_output.replace_range(start..end, &(num + value).to_string());
                }
            }
        }

        modified_output
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DateConfig {
    /// Enable the Date widget
    pub enable: bool,
    /// Set the Date format
    pub format: DateFormat,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
    /// TimeZone (https://docs.rs/chrono-tz/latest/chrono_tz/enum.Tz.html)
    ///
    /// Use a custom format to display additional information, i.e.:
    /// ```json
    /// {
    ///     "Date": {
    ///         "enable": true,
    ///         "format": { "Custom": "%D %Z (Tokyo)" },
    ///         "timezone": "Asia/Tokyo"
    ///      }
    ///}
    /// ```
    pub timezone: Option<String>,
}

impl From<DateConfig> for Date {
    fn from(value: DateConfig) -> Self {
        let data_refresh_interval = 1;

        Self {
            enable: value.enable,
            format: value.format,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
            timezone: value.timezone,
            data_refresh_interval,
            last_state: String::new(),
            last_updated: Instant::now()
                .checked_sub(Duration::from_secs(data_refresh_interval))
                .unwrap(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DateFormat {
    /// Month/Date/Year format (09/08/24)
    MonthDateYear,
    /// Year-Month-Date format (2024-09-08)
    YearMonthDate,
    /// Date-Month-Year format (8-Sep-2024)
    DateMonthYear,
    /// Day Date Month Year format (8 September 2024)
    DayDateMonthYear,
    /// Custom format (https://docs.rs/chrono/latest/chrono/format/strftime/index.html)
    Custom(String),
    /// Custom format with modifiers
    CustomModifiers(CustomModifiers),
}

impl DateFormat {
    pub fn next(&mut self) {
        match self {
            DateFormat::MonthDateYear => *self = Self::YearMonthDate,
            DateFormat::YearMonthDate => *self = Self::DateMonthYear,
            DateFormat::DateMonthYear => *self = Self::DayDateMonthYear,
            DateFormat::DayDateMonthYear => *self = Self::MonthDateYear,
            _ => {}
        };
    }

    pub fn fmt_string(&self) -> String {
        match self {
            DateFormat::MonthDateYear => String::from("%D"),
            DateFormat::YearMonthDate => String::from("%F"),
            DateFormat::DateMonthYear => String::from("%v"),
            DateFormat::DayDateMonthYear => String::from("%A %e %B %Y"),
            DateFormat::Custom(custom) => custom.to_string(),
            DateFormat::CustomModifiers(custom) => custom.format.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Date {
    pub enable: bool,
    pub format: DateFormat,
    label_prefix: LabelPrefix,
    timezone: Option<String>,
    data_refresh_interval: u64,
    last_state: String,
    last_updated: Instant,
}

impl Date {
    fn output(&mut self) -> String {
        let mut output = self.last_state.clone();
        let now = Instant::now();

        if now.duration_since(self.last_updated) > Duration::from_secs(self.data_refresh_interval) {
            let formatted = match &self.timezone {
                Some(timezone) => match timezone.parse::<Tz>() {
                    Ok(tz) => Local::now()
                        .with_timezone(&tz)
                        .format(&self.format.fmt_string())
                        .to_string()
                        .trim()
                        .to_string(),
                    Err(_) => format!("Invalid timezone: {timezone}"),
                },
                None => Local::now()
                    .format(&self.format.fmt_string())
                    .to_string()
                    .trim()
                    .to_string(),
            };

            // if custom modifiers are used, apply them
            output = match &self.format {
                DateFormat::CustomModifiers(custom) => custom.apply(&formatted),
                _ => formatted,
            };

            self.last_state.clone_from(&output);
            self.last_updated = now;
        }

        output
    }
}

impl BarWidget for Date {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let mut output = self.output();
            if !output.is_empty() {
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
                                Label::new(WidgetText::LayoutJob(layout_job.clone()))
                                    .selectable(false),
                            )
                        })
                        .clicked()
                    {
                        self.format.next()
                    }
                });
            }
        }
    }
}
