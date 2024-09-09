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
pub struct TimeConfig {
    /// Enable the Time widget
    pub enable: bool,
    /// Set the Time format
    pub format: TimeFormat,
}

impl From<TimeConfig> for Time {
    fn from(value: TimeConfig) -> Self {
        Self {
            enable: value.enable,
            format: value.format,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum TimeFormat {
    /// Twelve-hour format (with seconds)
    TwelveHour,
    /// Twenty-four-hour format (with seconds)
    TwentyFourHour,
    /// Custom format (https://docs.rs/chrono/latest/chrono/format/strftime/index.html)
    Custom(String),
}

impl TimeFormat {
    pub fn toggle(&mut self) {
        match self {
            TimeFormat::TwelveHour => *self = TimeFormat::TwentyFourHour,
            TimeFormat::TwentyFourHour => *self = TimeFormat::TwelveHour,
            _ => {}
        };
    }

    fn fmt_string(&self) -> String {
        match self {
            TimeFormat::TwelveHour => String::from("%l:%M:%S %p"),
            TimeFormat::TwentyFourHour => String::from("%T"),
            TimeFormat::Custom(format) => format.to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Time {
    pub enable: bool,
    pub format: TimeFormat,
}

impl Time {
    fn output(&mut self) -> Vec<String> {
        vec![chrono::Local::now()
            .format(&self.format.fmt_string())
            .to_string()]
    }
}

impl BarWidget for Time {
    fn render(&mut self, _ctx: &Context, ui: &mut Ui) {
        if self.enable {
            for output in self.output() {
                if ui
                    .add(
                        Label::new(format!("{} {}", egui_phosphor::regular::CLOCK, output))
                            .selectable(false)
                            .sense(Sense::click()),
                    )
                    .clicked()
                {
                    self.format.toggle()
                }
            }

            // TODO: make spacing configurable
            ui.add_space(WIDGET_SPACING);
        }
    }
}
