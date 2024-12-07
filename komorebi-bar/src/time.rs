use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widget::BarWidget;
use eframe::egui::text::LayoutJob;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::Label;
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
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<TimeConfig> for Time {
    fn from(value: TimeConfig) -> Self {
        Self {
            enable: value.enable,
            format: value.format,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
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
    label_prefix: LabelPrefix,
}

impl Time {
    fn output(&mut self) -> String {
        chrono::Local::now()
            .format(&self.format.fmt_string())
            .to_string()
    }
}

impl BarWidget for Time {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let mut output = self.output();
            if !output.is_empty() {
                let font_id = ctx
                    .style()
                    .text_styles
                    .get(&TextStyle::Body)
                    .cloned()
                    .unwrap_or_else(FontId::default);

                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::CLOCK.to_string()
                        }
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
                    font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                if let LabelPrefix::Text | LabelPrefix::IconAndText = self.label_prefix {
                    output.insert_str(0, "TIME: ");
                }

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat::simple(font_id, ctx.style().visuals.text_color()),
                );

                config.apply_on_widget(false, ui, |ui| {
                    if SelectableFrame::new(false)
                        .show(ui, |ui| ui.add(Label::new(layout_job).selectable(false)))
                        .clicked()
                    {
                        self.format.toggle()
                    }
                });
            }
        }
    }
}
