use crate::widget::BarWidget;
use crate::WIDGET_SPACING;
use eframe::egui::text::LayoutJob;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::TextFormat;
use eframe::egui::TextStyle;
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
    fn output(&mut self) -> String {
        chrono::Local::now()
            .format(&self.format.fmt_string())
            .to_string()
    }
}

impl BarWidget for Time {
    fn render(&mut self, ctx: &Context, ui: &mut Ui) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                let font_id = ctx
                    .style()
                    .text_styles
                    .get(&TextStyle::Body)
                    .cloned()
                    .unwrap_or_else(FontId::default);

                let mut layout_job = LayoutJob::simple(
                    egui_phosphor::regular::CLOCK.to_string(),
                    font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat::simple(font_id, ctx.style().visuals.text_color()),
                );

                if ui
                    .add(
                        Label::new(layout_job)
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
