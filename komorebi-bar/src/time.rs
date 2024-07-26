use crate::widget::BarWidget;

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

pub struct Time {
    pub format: TimeFormat,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            format: TimeFormat::TwelveHour,
        }
    }
}

impl BarWidget for Time {
    fn output(&mut self) -> Vec<String> {
        vec![chrono::Local::now()
            .format(&self.format.fmt_string())
            .to_string()]
    }
}
