use crate::widgets::applications::Applications;
use crate::widgets::battery::Battery;
use crate::widgets::cpu::Cpu;
use crate::widgets::date::Date;
use crate::widgets::keyboard::Keyboard;
use crate::widgets::komorebi::Komorebi;
use crate::widgets::media::Media;
use crate::widgets::memory::Memory;
use crate::widgets::network::Network;
use crate::widgets::storage::Storage;
use crate::widgets::systray::Systray;
use crate::widgets::time::Time;
use crate::widgets::update::Update;

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
