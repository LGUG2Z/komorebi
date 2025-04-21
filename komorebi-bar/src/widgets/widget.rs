use crate::render::RenderConfig;
use crate::widgets::applications::Applications;
use crate::widgets::applications::ApplicationsConfig;
use crate::widgets::battery::Battery;
use crate::widgets::battery::BatteryConfig;
use crate::widgets::cpu::Cpu;
use crate::widgets::cpu::CpuConfig;
use crate::widgets::date::Date;
use crate::widgets::date::DateConfig;
use crate::widgets::keyboard::Keyboard;
use crate::widgets::keyboard::KeyboardConfig;
use crate::widgets::komorebi::Komorebi;
use crate::widgets::komorebi::KomorebiConfig;
use crate::widgets::media::Media;
use crate::widgets::media::MediaConfig;
use crate::widgets::memory::Memory;
use crate::widgets::memory::MemoryConfig;
use crate::widgets::network::Network;
use crate::widgets::network::NetworkConfig;
use crate::widgets::storage::Storage;
use crate::widgets::storage::StorageConfig;
use crate::widgets::time::Time;
use crate::widgets::time::TimeConfig;
use crate::widgets::update::Update;
use crate::widgets::update::UpdateConfig;
use eframe::egui::Context;
use eframe::egui::Ui;
use serde::Deserialize;
use serde::Serialize;

pub trait BarWidget {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig);
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum WidgetConfig {
    Applications(ApplicationsConfig),
    Battery(BatteryConfig),
    Cpu(CpuConfig),
    Date(DateConfig),
    Keyboard(KeyboardConfig),
    Komorebi(KomorebiConfig),
    Media(MediaConfig),
    Memory(MemoryConfig),
    Network(NetworkConfig),
    Storage(StorageConfig),
    Time(TimeConfig),
    Update(UpdateConfig),
}

impl WidgetConfig {
    pub fn as_boxed_bar_widget(&self) -> Box<dyn BarWidget> {
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
            WidgetConfig::Time(config) => Box::new(Time::from(config.clone())),
            WidgetConfig::Update(config) => Box::new(Update::from(*config)),
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            WidgetConfig::Applications(config) => config.enable,
            WidgetConfig::Battery(config) => config.enable,
            WidgetConfig::Cpu(config) => config.enable,
            WidgetConfig::Date(config) => config.enable,
            WidgetConfig::Keyboard(config) => config.enable,
            WidgetConfig::Komorebi(config) => {
                config.workspaces.as_ref().is_some_and(|w| w.enable)
                    || config.layout.as_ref().is_some_and(|w| w.enable)
                    || config.focused_container.as_ref().is_some_and(|w| w.enable)
                    || config
                        .configuration_switcher
                        .as_ref()
                        .is_some_and(|w| w.enable)
            }
            WidgetConfig::Media(config) => config.enable,
            WidgetConfig::Memory(config) => config.enable,
            WidgetConfig::Network(config) => config.enable,
            WidgetConfig::Storage(config) => config.enable,
            WidgetConfig::Time(config) => config.enable,
            WidgetConfig::Update(config) => config.enable,
        }
    }
}
