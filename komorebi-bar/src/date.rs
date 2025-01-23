use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widget::BarWidget;
use eframe::egui::text::LayoutJob;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::WidgetText;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Custom {
    Simple(String),
    WithModifiers {
        Custom: String,
        Modifiers: std::collections::HashMap<String, i32>,
    },
}

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
    /// Allow modifiers for any integer formatter
    /// Use format: { Custom: "format_str", "Modifiers": { "formatter": value } }
    Custom(Custom),
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
            DateFormat::Custom(Custom) => match Custom {
                Custom::Simple(fmt) => fmt.clone(),
                Custom::WithModifiers { Custom, .. } => Custom.clone(),
            },
        }
    }

    fn apply_modifiers(&self, output: &str) -> String {
        // contains all strftime formatters that return integers
        let int_formatters = vec!(
            "%Y",
            "%C",
            "%y%",
            "%m",
            "%d",
            "%e",
            "%w",
            "%u",
            "%U",
            "%W",
            "%G",
            "%g",
            "%V",
            "%j",
            "%H",
            "%k",
            "%I",
            "%l",
            "%M",
            "%S",
            "%f"
        );

        match self {
            // unwrap the Custom enum
            DateFormat::Custom(Custom::WithModifiers { Modifiers: modifiers, .. }) => {
                let mut modified_output = output.to_string();

                // iterate over the modifiers
                for (modifier, value) in modifiers {
                    // only run if int formatters are used
                    if !int_formatters.contains(&modifier.as_str()) {
                        continue;
                    }

                    // get the strftime value of modifier
                    let formatted_modifier = chrono::Local::now().format(modifier).to_string();

                    // find the original value in the original output
                    if let Some(pos) = modified_output.find(&formatted_modifier) {
                        let start = pos;
                        let end = start + formatted_modifier.len();
                        // replace the original value with the modified value
                        if let Ok(num) = formatted_modifier.parse::<i32>() {
                            modified_output.replace_range(start..end, &(num + value).to_string());
                        }
                    }
                }
                modified_output
            }
            _ => output.to_string(),
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
        let formatted = chrono::Local::now().format(&self.format.fmt_string()).to_string();

        // if modifiers are present, apply them
        match &self.format {
            DateFormat::Custom { .. } => self.format.apply_modifiers(&formatted),
            _ => formatted,
        }
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
