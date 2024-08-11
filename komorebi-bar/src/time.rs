use crate::widget::BarWidget;

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
    pub fn new(enable: bool, format: TimeFormat) -> Self {
        Self { enable, format }
    }
}

impl BarWidget for Time {
    fn output(&mut self) -> Vec<String> {
        vec![chrono::Local::now()
            .format(&self.format.fmt_string())
            .to_string()]
    }
}
