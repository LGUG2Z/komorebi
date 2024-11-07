use crate::config::LabelPrefix;
use crate::widget::BarWidget;
use crate::widget::RenderConfig;
use crate::WIDGET_SPACING;
use eframe::egui::text::LayoutJob;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::TextFormat;
use eframe::egui::TextStyle;
use eframe::egui::Ui;
use eframe::egui::WidgetText;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DateConfig {
    /// Enable the Date widget
    pub enable: bool,
    /// Set the Date format
    pub format: DateFormat,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<DateConfig> for Date {
    fn from(value: DateConfig) -> Self {
        Self {
            enable: value.enable,
            format: value.format,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
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
    label_prefix: LabelPrefix,
}

impl Date {
    fn output(&mut self) -> String {
        chrono::Local::now()
            .format(&self.format.fmt_string())
            .to_string()
    }
}

impl BarWidget for Date {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, mut config: RenderConfig) {
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
                            egui_phosphor::regular::CALENDAR_DOTS.to_string()
                        }
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
                    font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                if let LabelPrefix::Text | LabelPrefix::IconAndText = self.label_prefix {
                    output.insert_str(0, "DATE: ");
                }

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat::simple(font_id, ctx.style().visuals.text_color()),
                );

                config.grouping.apply_on_widget(ui, |ui| {
                    if ui
                        .add(
                            Label::new(WidgetText::LayoutJob(layout_job.clone()))
                                .selectable(false)
                                .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        self.format.next()
                    }
                });
            }

            ui.add_space(WIDGET_SPACING);
        }
    }
}
