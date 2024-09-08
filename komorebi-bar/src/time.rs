use crate::widget::BarWidget;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::Ui;

#[derive(Copy, Clone, Debug)]
pub struct TimeConfig {
    pub enable: bool,
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

#[derive(Copy, Clone, Debug)]
pub enum TimeFormat {
    TwelveHour,
    TwentyFourHour,
}

impl TimeFormat {
    pub fn toggle(&mut self) {
        match self {
            TimeFormat::TwelveHour => *self = TimeFormat::TwentyFourHour,
            TimeFormat::TwentyFourHour => *self = TimeFormat::TwelveHour,
        };
    }

    fn fmt_string(&self) -> String {
        match self {
            TimeFormat::TwelveHour => String::from("%l:%M:%S %p"),
            TimeFormat::TwentyFourHour => String::from("%T"),
        }
    }
}

#[derive(Copy, Clone, Debug)]
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
            ui.add_space(10.0);
        }
    }
}
