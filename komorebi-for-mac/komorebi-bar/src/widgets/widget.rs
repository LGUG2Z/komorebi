use crate::widgets::applications::Applications;
use crate::widgets::battery::Battery;
use crate::widgets::cpu::Cpu;
use crate::widgets::date::Date;
use crate::widgets::komorebi::Komorebi;
use crate::widgets::memory::Memory;
use crate::widgets::network::Network;
use crate::widgets::storage::Storage;
use crate::widgets::time::Time;
use crate::widgets::update::Update;
use eframe::egui::Context;
use eframe::egui::Ui;

pub use komorebi_bar_core::widgets::widget::BarWidget;
pub use komorebi_bar_core::widgets::widget::WidgetConfig;
use komorebi_bar_core::render::RenderConfig;

/// Stands in for a widget that macOS cannot run; `WidgetConfig::enabled` is false for those, so
/// this is never built from a configuration that the bar loads.
struct Unsupported;

impl BarWidget for Unsupported {
    fn render(&mut self, _ctx: &Context, _ui: &mut Ui, _config: &mut RenderConfig) {}
}

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
            WidgetConfig::Komorebi(config) => Box::new(Komorebi::from(config)),
            WidgetConfig::Memory(config) => Box::new(Memory::from(*config)),
            WidgetConfig::Network(config) => Box::new(Network::from(*config)),
            WidgetConfig::Storage(config) => Box::new(Storage::from(*config)),
            WidgetConfig::Time(config) => Box::new(Time::from(config.clone())),
            WidgetConfig::Update(config) => Box::new(Update::from(*config)),
            WidgetConfig::Keyboard(_) | WidgetConfig::Media(_) | WidgetConfig::Systray(_) => {
                Box::new(Unsupported)
            }
        }
    }
}
