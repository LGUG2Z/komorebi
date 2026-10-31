use crate::windows::widgets::applications::Applications;
use crate::windows::widgets::battery::Battery;
use crate::windows::widgets::cpu::Cpu;
use crate::windows::widgets::date::Date;
use crate::windows::widgets::keyboard::Keyboard;
use crate::windows::widgets::komorebi::Komorebi;
use crate::windows::widgets::media::Media;
use crate::windows::widgets::memory::Memory;
use crate::windows::widgets::network::Network;
use crate::windows::widgets::storage::Storage;
use crate::windows::widgets::systray::Systray;
use crate::windows::widgets::time::Time;
use crate::windows::widgets::update::Update;

pub use komorebi_bar_core::widgets::widget::BarWidget;
pub use komorebi_bar_core::widgets::widget::WidgetConfig;

/// Turns a widget configuration into the widget that renders it.
pub trait WidgetConfigExt {
    fn as_boxed_bar_widget(&self) -> Box<dyn BarWidget>;
}

impl WidgetConfigExt for WidgetConfig {
    fn as_boxed_bar_widget(&self) -> Box<dyn BarWidget> {
        match self {
            WidgetConfig::Applications(config) => Box::new(Applications::from(config)),
            WidgetConfig::Battery(config) => Box::new(Battery::from(*config)),
            WidgetConfig::Cpu(config) => Box::new(Cpu::from(*config)),
            WidgetConfig::Date(config) => Box::new(Date::from(config.clone())),
            WidgetConfig::Keyboard(config) => Box::new(Keyboard::from(*config)),
            WidgetConfig::Komorebi(config) => Box::new(Komorebi::from(config)),
            WidgetConfig::Media(config) => Box::new(Media::from(*config)),
            WidgetConfig::Memory(config) => Box::new(Memory::from(*config)),
            WidgetConfig::Network(config) => Box::new(Network::from(*config)),
            WidgetConfig::Storage(config) => Box::new(Storage::from(*config)),
            WidgetConfig::Systray(config) => Box::new(Systray::from(config)),
            WidgetConfig::Time(config) => Box::new(Time::from(config.clone())),
            WidgetConfig::Update(config) => Box::new(Update::from(*config)),
        }
    }
}
