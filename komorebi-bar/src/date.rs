use crate::widget::BarWidget;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::Ui;

#[derive(Copy, Clone, Debug)]
pub struct DateConfig {
    pub enable: bool,
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

#[derive(Copy, Clone, Debug)]
pub enum DateFormat {
    MonthDateYear,
    YearMonthDate,
    DateMonthYear,
    DayDateMonthYear,
}

impl DateFormat {
    pub fn next(&mut self) {
        match self {
            DateFormat::MonthDateYear => *self = Self::YearMonthDate,
            DateFormat::YearMonthDate => *self = Self::DateMonthYear,
            DateFormat::DateMonthYear => *self = Self::DayDateMonthYear,
            DateFormat::DayDateMonthYear => *self = Self::MonthDateYear,
        };
    }

    fn fmt_string(&self) -> String {
        match self {
            DateFormat::MonthDateYear => String::from("%D"),
            DateFormat::YearMonthDate => String::from("%F"),
            DateFormat::DateMonthYear => String::from("%v"),
            DateFormat::DayDateMonthYear => String::from("%A %e %B %Y"),
        }
    }
}

#[derive(Copy, Clone, Debug)]
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
            ui.add_space(10.0);
        }
    }
}
