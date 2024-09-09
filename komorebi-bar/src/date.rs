use crate::widget::BarWidget;
use crate::WIDGET_SPACING;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::Ui;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DateConfig {
    /// Enable the Date widget
    pub enable: bool,
    /// Set the Date format
    pub format: DateFormat,
}

impl From<DateConfig> for Date {
    fn from(value: DateConfig) -> Self {
        Self {
            enable: value.enable,
            format: value.format,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
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

    fn fmt_string(&self) -> String {
        match self {
            DateFormat::MonthDateYear => String::from("%D"),
            DateFormat::YearMonthDate => String::from("%F"),
            DateFormat::DateMonthYear => String::from("%v"),
            DateFormat::DayDateMonthYear => String::from("%A %e %B %Y"),
            DateFormat::Custom(custom) => custom.to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Date {
    pub enable: bool,
    pub format: DateFormat,
}

impl Date {
    fn output(&mut self) -> Vec<String> {
        vec![chrono::Local::now()
            .format(&self.format.fmt_string())
            .to_string()]
    }
}

impl BarWidget for Date {
    fn render(&mut self, _ctx: &Context, ui: &mut Ui) {
        if self.enable {
            for output in self.output() {
                if ui
                    .add(
                        Label::new(format!(
                            "{} {}",
                            egui_phosphor::regular::CALENDAR_DOTS,
                            output
                        ))
                        .selectable(false)
                        .sense(Sense::click()),
                    )
                    .clicked()
                {
                    self.format.next()
                }
            }

            // TODO: make spacing configurable
            ui.add_space(WIDGET_SPACING);
        }
    }
}
